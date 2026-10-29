//! Directory orchestration, bounded parallel processing, and report/log finalization.
use crate::{
    cli::Args,
    config::{Config, HeuristicsConfig},
    durability::sync_parent,
    fits_image::{is_gzip_compressed, read_image},
    logger::Logger,
    model::{FileOutcome, OutcomeKind},
    output_layout::{OutputLayout, ProductCategory},
    report,
    workers::effective_worker_count,
};

mod decoded;
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
    summary_path: PathBuf,
}
#[derive(Clone, Copy)]
struct ProductOptions {
    dry_run: bool,
    overwrite: bool,
    png_preview: bool,
    reason_mask: bool,
}
impl std::fmt::Display for BatchFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} file(s) failed; see {}",
            self.count,
            self.summary_path.display()
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
    let (mut config, config_text) = Config::load(args.config.as_deref())?;
    let (heuristics, heuristics_text) = HeuristicsConfig::load(args.heuristics.as_deref())?;
    if let Some(threads) = args.threads {
        config.processing.threads = threads;
    }
    if let Some(mib) = args.memory_budget_mib {
        config.processing.memory_budget_mib = mib;
    }
    if let Some(mode) = args.mode {
        config.processing.mode = mode;
    }
    if args.png && !config.processing.mode.is_crop() {
        bail!(
            "--png requires a crop mode (set processing.mode = \"crop\" or \"crop_masked\", or pass --mode crop)"
        );
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
    let executable_sha256 = executable_sha256()?;
    let available_cores = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    let workers = effective_worker_count(
        config.processing.threads,
        available_cores,
        config.processing.memory_budget_mib,
        config.processing.maximum_image_mib,
    );
    let run_id = Utc::now().format("%Y%m%dT%H%M%S%.9fZ").to_string();
    let layout = Arc::new(OutputLayout::prepare(
        &args.output_dir,
        &input,
        &run_id,
        args.flat_output,
        args.sanitize_output,
    )?);
    let run_dir = layout.run().to_path_buf();
    let mut logger = Logger::create(&run_dir, &run_id)?;
    logger.event(&run_id,"run_started",serde_json::json!({"input":input,"output_root":layout.root(),"run_directory":&run_dir,"source_config_sha256":source_config_hash,"effective_config_sha256":config_hash,
        "source_heuristics_sha256":source_heuristics_hash,"effective_heuristics_sha256":heuristics_hash,
        "version":env!("RUST_FITS_AUTO_CROPPING_VERSION"),"algorithm_version":heuristics.heuristics.algorithm_version(),"git_revision":env!("RUST_FITS_GIT_REVISION"),"git_dirty":env!("RUST_FITS_GIT_DIRTY")=="true","executable_sha256":executable_sha256,"threads":config.processing.threads,"effective_config":config,"effective_heuristics":heuristics,"mode":config.processing.mode,"recursive":config.processing.recursive,"dry_run":args.dry_run,"reason_mask":args.reason_mask}))?;
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
                    &layout,
                    &config,
                    &heuristics,
                    ProductOptions {
                        dry_run: args.dry_run,
                        overwrite: args.overwrite,
                        png_preview: args.png,
                        reason_mask: args.reason_mask,
                    },
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
    // Mask FITS files are implementation sidecars, not retained run products.
    // Delete the trees before writing reports so they never advertise dangling
    // paths after the command has exited.
    layout.remove_transient_mask_directories()?;
    for outcome in &mut outcomes {
        outcome.output_mask = None;
        outcome.output_reason_mask = None;
    }
    report::write(
        &run_dir,
        &run_id,
        &outcomes,
        args.dry_run,
        report::ReportIdentity {
            algorithm_version: heuristics.heuristics.algorithm_version(),
            executable_sha256: &executable_sha256,
            source_config_sha256: &source_config_hash,
            effective_config_sha256: &config_hash,
            source_heuristics_sha256: &source_heuristics_hash,
            effective_heuristics_sha256: &heuristics_hash,
        },
    )?;
    if logger_failed.load(Ordering::Relaxed) {
        bail!("activity logger failed while recording file events; see completed summary report")
    };
    let clipped = outcomes
        .iter()
        .filter(|o| matches!(o.status, OutcomeKind::Clipped))
        .count();
    let proposed = outcomes
        .iter()
        .filter(|o| matches!(o.status, OutcomeKind::Proposed))
        .count();
    let kept = outcomes
        .iter()
        .filter(|o| matches!(o.status, OutcomeKind::Kept))
        .count();
    let ignored = outcomes
        .iter()
        .filter(|o| matches!(o.status, OutcomeKind::Ignored))
        .count();
    let output_images = outcomes
        .iter()
        .filter(|outcome| outcome.output_image.is_some())
        .count();
    let errors = outcomes
        .iter()
        .filter(|o| matches!(o.status, OutcomeKind::Error))
        .count();
    if let Ok(mut log) = logger.lock() {
        log.event(
            &run_id,
            "run_finished",
            serde_json::json!({"total":outcomes.len(),"output_images":output_images,"clipped":clipped,"proposed":proposed,
        "kept":kept,"ignored":ignored,"errors":errors,"dry_run":args.dry_run,"elapsed_ms":started.elapsed().as_millis()}),
        )?;
    }
    println!(
        "Run {run_id}: {output_images}/{} input images published, {clipped} clipped, {proposed} proposed, {kept} kept, {ignored} ignored, {errors} errors",
        outcomes.len()
    );
    if errors > 0 {
        return Err(BatchFailure {
            count: errors,
            summary_path: run_dir.join("summary.json"),
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

fn executable_sha256() -> anyhow::Result<String> {
    let path = std::env::current_exe().context("locating current executable")?;
    let bytes =
        fs::read(&path).with_context(|| format!("hashing executable {}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn process_one(
    path: &Path,
    input: &Path,
    layout: &OutputLayout,
    config: &Config,
    heuristics: &HeuristicsConfig,
    options: ProductOptions,
) -> FileOutcome {
    let timer = Instant::now();
    let rel = path.strip_prefix(input).unwrap_or(path);
    if rel.to_string_lossy().to_ascii_lowercase().ends_with(".gz")
        || rel.to_string_lossy().to_ascii_lowercase().ends_with(".fz")
        || is_gzip_compressed(path).unwrap_or(false)
    {
        let mut outcome = FileOutcome::new(
            rel.to_path_buf(),
            OutcomeKind::Ignored,
            "compressed FITS is not supported by this build",
        );
        outcome.reason_code = "compressed_ignored";
        if !options.dry_run {
            publish_passthrough_image(path, rel, layout, options.overwrite, &mut outcome);
        }
        return finish(outcome, timer);
    }
    let read_timer = Instant::now();
    let read_result = read_image(path, config.processing.maximum_image_mib);
    let read_ms = elapsed_ms(read_timer);
    let result = match read_result {
        Ok(Err(reason)) => {
            let reason_code = eligibility_reason_code(&reason);
            let mut outcome = FileOutcome::new(
                rel.to_path_buf(),
                OutcomeKind::Ignored,
                format!("ineligible image: {}", eligibility_reason(reason)),
            );
            outcome.reason_code = reason_code;
            if !options.dry_run {
                publish_passthrough_image(path, rel, layout, options.overwrite, &mut outcome);
            }
            outcome
        }
        Err(error) => {
            let mut outcome =
                FileOutcome::new(rel.to_path_buf(), OutcomeKind::Error, format!("{error:#}"));
            outcome.reason_code = "input_read_error";
            if !options.dry_run {
                publish_passthrough_image(path, rel, layout, options.overwrite, &mut outcome);
            }
            outcome
        }
        Ok(Ok(image)) => {
            decoded::process_decoded(path, rel, image, layout, config, heuristics, options)
        }
    };
    let mut result = result;
    result.stage_ms.read = read_ms;
    finish(result, timer)
}

fn elapsed_ms(timer: Instant) -> f64 {
    timer.elapsed().as_secs_f64() * 1_000.0
}

fn publish_passthrough_image(
    input_path: &Path,
    relative: &Path,
    layout: &OutputLayout,
    overwrite: bool,
    outcome: &mut FileOutcome,
) {
    let image_path = match layout.product_path(ProductCategory::Image, relative) {
        Ok(path) => path,
        Err(error) => {
            outcome.status = OutcomeKind::Error;
            outcome.reason_code = "output_path_error";
            outcome
                .reason
                .push_str(&format!("; could not create pass-through path: {error}"));
            return;
        }
    };
    if !overwrite && path_exists(&image_path) {
        outcome.status = OutcomeKind::Error;
        outcome.reason_code = "output_exists_error";
        outcome
            .reason
            .push_str("; pass-through output already exists");
        return;
    }
    if overwrite && let Err(error) = remove_existing_product(&image_path) {
        outcome.status = OutcomeKind::Error;
        outcome.reason_code = "output_cleanup_error";
        outcome
            .reason
            .push_str(&format!("; could not replace pass-through output: {error}"));
        return;
    }
    if let Err(error) = create_kept_symlink(input_path, &image_path) {
        outcome.status = OutcomeKind::Error;
        outcome.reason_code = "input_fallback_symlink_error";
        outcome
            .reason
            .push_str(&format!("; could not preserve input by symlink: {error:#}"));
        return;
    }
    outcome.output_image = Some(image_path);
    outcome
        .reason
        .push_str("; source preserved unchanged by symlink");
}

fn create_kept_symlink(from: &Path, to: &Path) -> anyhow::Result<()> {
    let parent = to.parent().context("output path has no parent")?;
    let target = fs::canonicalize(from)
        .with_context(|| format!("resolving kept input {}", from.display()))?;
    let temporary = tempfile::tempdir_in(parent)?;
    let temporary_link = temporary.path().join("kept-image");
    create_file_symlink(&target, &temporary_link)?;
    fs::rename(&temporary_link, to)
        .with_context(|| format!("publishing kept-image symlink {}", to.display()))?;
    sync_parent(to)?;
    Ok(())
}

#[cfg(unix)]
fn create_file_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn create_file_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}
fn finish(mut outcome: FileOutcome, timer: Instant) -> FileOutcome {
    if matches!(outcome.status, OutcomeKind::Error) && !outcome.reason_code.ends_with("_error") {
        outcome.reason_code = "processing_error";
    }
    outcome.elapsed_ms = timer.elapsed().as_millis();
    outcome
}

fn path_exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn remove_existing_product(path: &Path) -> anyhow::Result<()> {
    if path_exists(path) {
        fs::remove_file(path)
            .with_context(|| format!("removing stale product {}", path.display()))?;
        sync_parent(path)?;
    }
    Ok(())
}

fn eligibility_reason(reason: crate::fits_image::Eligibility) -> String {
    match reason {
        crate::fits_image::Eligibility::MultiHdu(n) => {
            format!("contains {n} HDUs; only one-HDU files are supported")
        }
        crate::fits_image::Eligibility::Not2d => "primary HDU is not a 2D image".into(),
        crate::fits_image::Eligibility::EmptyImage => "image has zero pixels".into(),
        crate::fits_image::Eligibility::IntegerPrecisionLoss => {
            "integer sample span exceeds exact f64 analysis precision".into()
        }
    }
}

fn eligibility_reason_code(reason: &crate::fits_image::Eligibility) -> &'static str {
    match reason {
        crate::fits_image::Eligibility::MultiHdu(_) => "multi_hdu_ignored",
        crate::fits_image::Eligibility::Not2d => "not_2d_ignored",
        crate::fits_image::Eligibility::EmptyImage => "empty_image_ignored",
        crate::fits_image::Eligibility::IntegerPrecisionLoss => "integer_precision_ignored",
    }
}
