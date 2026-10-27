//! Directory orchestration, bounded parallel processing, and report/log finalization.
use crate::{
    cli::Args,
    config::{Config, HeuristicsConfig},
    detection::detect_border,
    fits_image::read_image,
    logger::Logger,
    model::{FileOutcome, OutcomeKind},
    report,
    writer::write_products,
};
use anyhow::{Context, bail};
use chrono::Utc;
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
use walkdir::WalkDir;

#[derive(Debug)]
pub struct RunError {
    code: i32,
    message: String,
}
impl RunError {
    pub fn exit_code(&self) -> i32 {
        self.code
    }
}
impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for RunError {}
#[derive(Debug)]
struct BatchFailure {
    count: usize,
    run_id: String,
}
impl std::fmt::Display for BatchFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} file(s) failed; see reports/{}/summary.json",
            self.count, self.run_id
        )
    }
}
impl std::error::Error for BatchFailure {}

pub fn run(args: Args) -> Result<(), RunError> {
    match run_inner(args) {
        Ok(()) => Ok(()),
        Err(error) => {
            let code = if error.downcast_ref::<BatchFailure>().is_some() {
                1
            } else {
                2
            };
            Err(RunError {
                code,
                message: error.to_string(),
            })
        }
    }
}

fn run_inner(args: Args) -> anyhow::Result<()> {
    let started = Instant::now();
    let input = fs::canonicalize(&args.input_dir).with_context(|| {
        format!(
            "input directory {} does not exist",
            args.input_dir.display()
        )
    })?;
    if !input.is_dir() {
        bail!("input path is not a directory");
    }
    fs::create_dir_all(&args.output_dir)
        .with_context(|| format!("creating output directory {}", args.output_dir.display()))?;
    let output = fs::canonicalize(&args.output_dir)?;
    if input.starts_with(&output) || output.starts_with(&input) {
        bail!("input and output directories must not overlap");
    }
    let (mut config, config_text) = Config::load(args.config.as_deref())?;
    let (heuristics, heuristics_text) = HeuristicsConfig::load(args.heuristics.as_deref())?;
    if let Some(threads) = args.threads {
        config.processing.threads = threads;
    }
    if let Some(mib) = args.memory_budget_mib {
        config.processing.memory_budget_mib = mib;
    }
    if config.processing.memory_budget_mib < config.processing.maximum_image_mib {
        bail!("memory budget must be >= maximum image workspace");
    }
    if args.recursive {
        config.processing.recursive = true;
    }
    let source_config_hash = format!("{:x}", Sha256::digest(config_text.as_bytes()));
    let config_hash = format!("{:x}", Sha256::digest(serde_json::to_vec(&config)?));
    let source_heuristics_hash = format!("{:x}", Sha256::digest(heuristics_text.as_bytes()));
    let heuristics_hash = format!("{:x}", Sha256::digest(serde_json::to_vec(&heuristics)?));
    let requested = if config.processing.threads == 0 {
        rayon::current_num_threads().max(1)
    } else {
        config.processing.threads.max(1)
    };
    let memory_limited =
        (config.processing.memory_budget_mib / config.processing.maximum_image_mib).max(1);
    let workers = requested.min(memory_limited);
    let run_id = Utc::now().format("%Y%m%dT%H%M%S%.9fZ").to_string();
    let mut logger = Logger::create(&output, &run_id)?;
    logger.event(&run_id,"run_started",serde_json::json!({"input":input,"output":output,"source_config_sha256":source_config_hash,"effective_config_sha256":config_hash,
        "source_heuristics_sha256":source_heuristics_hash,"effective_heuristics_sha256":heuristics_hash,
        "version":env!("CARGO_PKG_VERSION"),"threads":config.processing.threads,"effective_config":config,"effective_heuristics":heuristics,"recursive":config.processing.recursive,"dry_run":args.dry_run}))?;
    let mut paths = discover(&input, config.processing.recursive)?;
    paths.sort();
    logger.event(&run_id,"discovery_complete",serde_json::json!({"candidate_fits_files":paths.len(),"workers":workers,"memory_budget_mib":config.processing.memory_budget_mib,"maximum_image_mib":config.processing.maximum_image_mib}))?;
    let config = Arc::new(config);
    let heuristics = Arc::new(heuristics);
    let logger = Arc::new(Mutex::new(logger));
    let logger_failed = Arc::new(AtomicBool::new(false));
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(workers)
        .build()?;
    let mut outcomes: Vec<FileOutcome> = pool.install(|| {
        paths
            .par_iter()
            .map(|path| {
                let outcome = process_one(
                    path,
                    &input,
                    &output,
                    &config,
                    &heuristics,
                    args.dry_run,
                    args.overwrite,
                );
                match logger.lock() {
                    Ok(mut log) => {
                        if log.event(&run_id, "file_finished", &outcome).is_err() {
                            logger_failed.store(true, Ordering::Relaxed)
                        }
                    }
                    Err(_) => logger_failed.store(true, Ordering::Relaxed),
                }
                outcome
            })
            .collect()
    });
    outcomes.sort_by(|a, b| a.input.cmp(&b.input));
    report::write(&output, &run_id, &outcomes)?;
    if logger_failed.load(Ordering::Relaxed) {
        bail!("activity logger failed while recording file events; see completed summary report")
    };
    let clipped = outcomes
        .iter()
        .filter(|o| matches!(o.status, OutcomeKind::Clipped))
        .count();
    let kept = outcomes
        .iter()
        .filter(|o| matches!(o.status, OutcomeKind::Kept))
        .count();
    let ignored = outcomes
        .iter()
        .filter(|o| matches!(o.status, OutcomeKind::Ignored))
        .count();
    let errors = outcomes
        .iter()
        .filter(|o| matches!(o.status, OutcomeKind::Error))
        .count();
    if let Ok(mut log) = logger.lock() {
        log.event(
            &run_id,
            "run_finished",
            serde_json::json!({"total":outcomes.len(),"clipped":clipped,
        "kept":kept,"ignored":ignored,"errors":errors,"elapsed_ms":started.elapsed().as_millis()}),
        )?;
    }
    println!("Run {run_id}: {clipped} clipped, {kept} kept, {ignored} ignored, {errors} errors");
    if errors > 0 {
        return Err(BatchFailure {
            count: errors,
            run_id: run_id.clone(),
        }
        .into());
    }
    Ok(())
}

fn discover(input: &Path, recursive: bool) -> anyhow::Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    for entry in WalkDir::new(input)
        .follow_links(false)
        .min_depth(1)
        .max_depth(if recursive { usize::MAX } else { 1 })
    {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if [
            ".fits.gz", ".fit.gz", ".fts.gz", ".fits.fz", ".fit.fz", ".fts.fz", ".fits", ".fit",
            ".fts", ".fz",
        ]
        .iter()
        .any(|ext| name.ends_with(ext))
        {
            found.push(entry.into_path());
        }
    }
    Ok(found)
}

fn process_one(
    path: &Path,
    input: &Path,
    output: &Path,
    config: &Config,
    heuristics: &HeuristicsConfig,
    dry: bool,
    overwrite: bool,
) -> FileOutcome {
    let timer = Instant::now();
    let rel = path.strip_prefix(input).unwrap_or(path);
    if rel.to_string_lossy().to_ascii_lowercase().ends_with(".gz")
        || rel.to_string_lossy().to_ascii_lowercase().ends_with(".fz")
    {
        return finish(
            FileOutcome::new(
                rel.to_path_buf(),
                OutcomeKind::Ignored,
                "compressed FITS is not supported by this build",
            ),
            timer,
        );
    }
    let result = match read_image(path, config.processing.maximum_image_mib) {
        Ok(Err(reason)) => FileOutcome::new(
            rel.to_path_buf(),
            OutcomeKind::Ignored,
            format!("ineligible image: {}", eligibility_reason(reason)),
        ),
        Err(error) => FileOutcome::new(rel.to_path_buf(), OutcomeKind::Error, format!("{error:#}")),
        Ok(Ok(image)) => {
            let detection = detect_border(
                &image.values,
                image.width,
                image.height,
                &heuristics.heuristics,
            );
            let mut outcome = FileOutcome::new(
                rel.to_path_buf(),
                if detection.accepted {
                    OutcomeKind::Clipped
                } else {
                    OutcomeKind::Kept
                },
                detection.reason.clone(),
            );
            outcome.width = Some(image.width);
            outcome.height = Some(image.height);
            outcome.proposed_mask_fraction = detection.candidate_fraction;
            outcome.candidate_tiles = detection.candidate_tiles;
            outcome.score = detection.score;
            outcome.evidence_ratio = detection.evidence_ratio;
            outcome.artifact_benefit = detection.artifact_benefit;
            outcome.valid_pixel_loss = detection.valid_pixel_loss;
            outcome.net_utility = detection.net_utility;
            if detection.accepted {
                let n = detection.mask.iter().filter(|v| **v).count();
                outcome.masked_pixels = n;
                outcome.mask_fraction = n as f64 / (image.width * image.height) as f64;
                if !dry {
                    let image_path = output.join("images").join(rel);
                    let mask_path = output.join("masks").join(rel);
                    if let Some(p) = image_path.parent()
                        && let Err(e) = fs::create_dir_all(p)
                    {
                        outcome.status = OutcomeKind::Error;
                        outcome.reason = e.to_string();
                        return finish(outcome, timer);
                    }
                    if let Some(p) = mask_path.parent()
                        && let Err(e) = fs::create_dir_all(p)
                    {
                        outcome.status = OutcomeKind::Error;
                        outcome.reason = e.to_string();
                        return finish(outcome, timer);
                    }
                    if !overwrite && (image_path.exists() || mask_path.exists()) {
                        outcome.status = OutcomeKind::Error;
                        outcome.reason = "output exists; pass --overwrite".into();
                        return finish(outcome, timer);
                    }
                    if let Err(e) =
                        write_products(path, &image_path, &mask_path, &image, &detection.mask)
                    {
                        outcome.status = OutcomeKind::Error;
                        outcome.reason = e.to_string();
                        return finish(outcome, timer);
                    }
                    outcome.output_image = Some(image_path);
                    outcome.output_mask = Some(mask_path);
                }
            } else if !dry {
                let image_path = output.join("images").join(rel);
                if !overwrite && image_path.exists() {
                    outcome.status = OutcomeKind::Error;
                    outcome.reason = "output exists; pass --overwrite".into();
                    return finish(outcome, timer);
                }
                if let Some(p) = image_path.parent()
                    && let Err(e) = fs::create_dir_all(p)
                {
                    outcome.status = OutcomeKind::Error;
                    outcome.reason = e.to_string();
                    return finish(outcome, timer);
                }
                if let Err(e) = copy_atomic(path, &image_path) {
                    outcome.status = OutcomeKind::Error;
                    outcome.reason = e.to_string();
                    return finish(outcome, timer);
                }
                outcome.output_image = Some(image_path);
            }
            outcome
        }
    };
    finish(result, timer)
}

fn copy_atomic(from: &Path, to: &Path) -> anyhow::Result<()> {
    let parent = to.parent().context("output path has no parent")?;
    let tmp = tempfile::NamedTempFile::new_in(parent)?;
    fs::copy(from, tmp.path())?;
    tmp.persist(to).map_err(|e| e.error)?;
    Ok(())
}
fn finish(mut outcome: FileOutcome, timer: Instant) -> FileOutcome {
    outcome.elapsed_ms = timer.elapsed().as_millis();
    outcome
}

fn eligibility_reason(reason: crate::fits_image::Eligibility) -> String {
    match reason {
        crate::fits_image::Eligibility::MultiHdu(n) => {
            format!("contains {n} HDUs; only one-HDU files are supported")
        }
        crate::fits_image::Eligibility::Not2d => "primary HDU is not a 2D image".into(),
        crate::fits_image::Eligibility::EmptyImage => "image has zero pixels".into(),
    }
}
