//! Directory orchestration, bounded parallel processing, and report/log finalization.
use crate::{
    cli::Args,
    config::{Config, HeuristicsConfig, ProcessingMode},
    crop::{CropRegion, largest_clean_region},
    detection::detect_border,
    fits_image::{NoUnusedIntegerCode, is_gzip_compressed, read_image},
    logger::Logger,
    model::{FileOutcome, OutcomeKind},
    output_layout::{OutputLayout, ProductCategory},
    preview::write_crop_preview,
    report,
    writer::{write_cropped_products, write_products},
};
use anyhow::{Context, bail};
use chrono::Utc;
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
use tempfile::NamedTempFile;
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
#[derive(Clone, Copy)]
struct ProductOptions {
    dry_run: bool,
    overwrite: bool,
    png_preview: bool,
}
impl std::fmt::Display for BatchFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} file(s) failed; see runs/{}/summary.json",
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
    if args.png && !matches!(config.processing.mode, ProcessingMode::Crop) {
        bail!("--png requires crop mode (set processing.mode = \"crop\" or pass --mode crop)");
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
    let requested = if config.processing.threads == 0 {
        rayon::current_num_threads().max(1)
    } else {
        config.processing.threads.max(1)
    };
    let memory_limited =
        (config.processing.memory_budget_mib / config.processing.maximum_image_mib).max(1);
    let workers = requested.min(memory_limited);
    let run_id = Utc::now().format("%Y%m%dT%H%M%S%.9fZ").to_string();
    let layout = Arc::new(OutputLayout::prepare(&args.output_dir, &input, &run_id)?);
    let run_dir = layout.run().to_path_buf();
    let mut logger = Logger::create(&run_dir, &run_id)?;
    logger.event(&run_id,"run_started",serde_json::json!({"input":input,"output_root":layout.root(),"run_directory":&run_dir,"source_config_sha256":source_config_hash,"effective_config_sha256":config_hash,
        "source_heuristics_sha256":source_heuristics_hash,"effective_heuristics_sha256":heuristics_hash,
        "version":env!("RUST_FITS_AUTO_CROPPING_VERSION"),"git_revision":env!("RUST_FITS_GIT_REVISION"),"git_dirty":env!("RUST_FITS_GIT_DIRTY")=="true","executable_sha256":executable_sha256,"threads":config.processing.threads,"effective_config":config,"effective_heuristics":heuristics,"mode":config.processing.mode,"recursive":config.processing.recursive,"dry_run":args.dry_run}))?;
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
                    &run_id,
                    &layout,
                    &config,
                    &heuristics,
                    ProductOptions {
                        dry_run: args.dry_run,
                        overwrite: args.overwrite,
                        png_preview: args.png,
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
    report::write(
        &run_dir,
        &run_id,
        &outcomes,
        args.dry_run,
        report::ReportIdentity {
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
    let errors = outcomes
        .iter()
        .filter(|o| matches!(o.status, OutcomeKind::Error))
        .count();
    if let Ok(mut log) = logger.lock() {
        log.event(
            &run_id,
            "run_finished",
            serde_json::json!({"total":outcomes.len(),"clipped":clipped,"proposed":proposed,
        "kept":kept,"ignored":ignored,"errors":errors,"dry_run":args.dry_run,"elapsed_ms":started.elapsed().as_millis()}),
        )?;
    }
    println!(
        "Run {run_id}: {clipped} clipped, {proposed} proposed, {kept} kept, {ignored} ignored, {errors} errors"
    );
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

fn executable_sha256() -> anyhow::Result<String> {
    let path = std::env::current_exe().context("locating current executable")?;
    let bytes =
        fs::read(&path).with_context(|| format!("hashing executable {}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn process_one(
    path: &Path,
    input: &Path,
    run_id: &str,
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
        return finish(outcome, timer);
    }
    let result = match read_image(path, config.processing.maximum_image_mib) {
        Ok(Err(reason)) => {
            let reason_code = eligibility_reason_code(&reason);
            let mut outcome = FileOutcome::new(
                rel.to_path_buf(),
                OutcomeKind::Ignored,
                format!("ineligible image: {}", eligibility_reason(reason)),
            );
            outcome.reason_code = reason_code;
            outcome
        }
        Err(error) => {
            let mut outcome =
                FileOutcome::new(rel.to_path_buf(), OutcomeKind::Error, format!("{error:#}"));
            outcome.reason_code = "input_read_error";
            outcome
        }
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
            outcome.reason_code = detection.reason_code;
            outcome.width = Some(image.width);
            outcome.height = Some(image.height);
            outcome.source_sha256 = Some(image.source_sha256.clone());
            outcome.original_undefined_pixels =
                image.values.iter().filter(|v| !v.is_finite()).count();
            outcome.proposed_mask_fraction = detection.candidate_fraction;
            outcome.candidate_mask_pixels = detection.candidate_mask_pixels;
            outcome.candidate_tiles = detection.candidate_tiles;
            outcome.candidate_components = detection.candidate_components;
            outcome.accepted_components = detection.accepted_components;
            outcome.score = detection.score;
            outcome.evidence_ratio = detection.evidence_ratio;
            outcome.artifact_benefit = detection.artifact_benefit;
            outcome.valid_pixel_loss = detection.valid_pixel_loss;
            outcome.net_utility = detection.net_utility;
            if !image.metadata_warnings.is_empty() {
                outcome.reason.push_str("; ");
                outcome.reason.push_str(&image.metadata_warnings.join("; "));
            }
            if detection.accepted {
                let n = detection.mask.iter().filter(|v| **v).count();
                outcome.proposed_mask_pixels = n;
                let crop = if matches!(config.processing.mode, ProcessingMode::Crop)
                    && image.crop_wcs_unsupported.is_none()
                {
                    largest_clean_region(&detection.mask, image.width, image.height)
                } else {
                    None
                };
                let mut crop_accepted = false;
                if matches!(config.processing.mode, ProcessingMode::Crop) && crop.is_none() {
                    outcome.status = OutcomeKind::Kept;
                    outcome.reason_code = if image.crop_wcs_unsupported.is_some() {
                        "crop_unsupported_wcs"
                    } else {
                        "crop_geometry_unavailable"
                    };
                    let reason = image.crop_wcs_unsupported.as_ref().map_or_else(
                        || {
                            "no positive-area rectangle excludes every detected artifact pixel"
                                .into()
                        },
                        |reason| format!("crop declined: {reason}"),
                    );
                    outcome.reason.push_str("; ");
                    outcome.reason.push_str(&reason);
                    outcome.reason.push_str("; source retained unchanged");
                }
                if options.dry_run && detection.accepted && outcome.status == OutcomeKind::Clipped {
                    outcome.status = OutcomeKind::Proposed;
                    outcome.reason_code = "dry_run_proposal";
                }
                if let Some(rect) = crop {
                    let total = image.width * image.height;
                    let defined = total - outcome.original_undefined_pixels;
                    let bounds = rect.bounds();
                    let excluded: Vec<bool> = (0..total)
                        .map(|i| !rect.retains(i % image.width, i / image.width))
                        .collect();
                    outcome.crop_shape = Some(rect.shape_name().to_owned());
                    outcome.crop_origin_x = Some(bounds.x);
                    outcome.crop_origin_y = Some(bounds.y);
                    outcome.crop_width = Some(bounds.width);
                    outcome.crop_height = Some(bounds.height);
                    outcome.image_area_retained = rect.retained_pixels();
                    if let CropRegion::Circle {
                        center_x,
                        center_y,
                        radius,
                        ..
                    } = rect
                    {
                        outcome.crop_center_x = Some(center_x as f64);
                        outcome.crop_center_y = Some(center_y as f64);
                        outcome.crop_radius = Some(radius);
                    }
                    outcome.artifact_pixels_removed = detection
                        .mask
                        .iter()
                        .zip(&excluded)
                        .zip(&image.values)
                        .filter(|((candidate, excluded), value)| {
                            **candidate && **excluded && value.is_finite()
                        })
                        .count();
                    outcome.unmasked_pixels_sacrificed = detection
                        .mask
                        .iter()
                        .zip(&excluded)
                        .zip(&image.values)
                        .filter(|((candidate, excluded), value)| {
                            !**candidate && **excluded && value.is_finite()
                        })
                        .count();
                    outcome.crop_excluded_undefined_pixels = excluded
                        .iter()
                        .zip(&image.values)
                        .filter(|(excluded, value)| **excluded && !value.is_finite())
                        .count();
                    let artifact_fraction =
                        outcome.artifact_pixels_removed as f64 / defined.max(1) as f64;
                    let valid_loss_fraction =
                        outcome.unmasked_pixels_sacrificed as f64 / defined.max(1) as f64;
                    outcome.artifact_benefit = artifact_fraction * detection.evidence_ratio;
                    outcome.valid_pixel_loss = heuristics.heuristics.valid_pixel_loss_cost
                        * valid_loss_fraction
                        / (1.0 - valid_loss_fraction).max(f64::EPSILON);
                    outcome.net_utility = outcome.artifact_benefit - outcome.valid_pixel_loss;
                    crop_accepted =
                        outcome.net_utility >= heuristics.heuristics.minimum_net_utility;
                    if !crop_accepted {
                        outcome.status = OutcomeKind::Kept;
                        outcome.reason_code = "crop_utility_rejected";
                    }
                    outcome.reason.push_str(&format!(
                        "; proposed {} crop origin ({}, {}), bounds {}x{}, retained {} pixels; removes {} artifact pixels and sacrifices {} unmasked pixels; crop utility {:.4} (benefit {:.4} - loss {:.4}) {}",
                        rect.shape_name(), bounds.x, bounds.y, bounds.width, bounds.height, rect.retained_pixels(),
                        outcome.artifact_pixels_removed, outcome.unmasked_pixels_sacrificed,
                        outcome.net_utility, outcome.artifact_benefit, outcome.valid_pixel_loss,
                        if crop_accepted { "accepted" } else { "rejected; source retained" }
                    ));
                }
                if !options.dry_run {
                    let image_path = match layout.product_path(ProductCategory::Image, rel) {
                        Ok(path) => path,
                        Err(error) => {
                            outcome.status = OutcomeKind::Error;
                            outcome.reason = error.to_string();
                            return finish(outcome, timer);
                        }
                    };
                    let mask_path = match layout.product_path(ProductCategory::Mask, rel) {
                        Ok(path) => path,
                        Err(error) => {
                            outcome.status = OutcomeKind::Error;
                            outcome.reason = error.to_string();
                            return finish(outcome, timer);
                        }
                    };
                    let png_path = if options.png_preview && crop_accepted {
                        match layout.preview_path(rel) {
                            Ok(path) => Some(path),
                            Err(error) => {
                                outcome.status = OutcomeKind::Error;
                                outcome.reason = error.to_string();
                                return finish(outcome, timer);
                            }
                        }
                    } else {
                        None
                    };
                    let manifest_path = match layout.manifest_path(rel) {
                        Ok(path) => path,
                        Err(error) => {
                            outcome.status = OutcomeKind::Error;
                            outcome.reason = error.to_string();
                            return finish(outcome, timer);
                        }
                    };
                    if !options.overwrite
                        && (path_exists(&image_path)
                            || path_exists(&mask_path)
                            || path_exists(&manifest_path)
                            || png_path.as_ref().is_some_and(|path| path_exists(path)))
                    {
                        outcome.status = OutcomeKind::Error;
                        outcome.reason = "output exists; pass --overwrite".into();
                        return finish(outcome, timer);
                    }
                    if options.overwrite
                        && let Err(error) = remove_existing_product(&manifest_path)
                    {
                        outcome.status = OutcomeKind::Error;
                        outcome.reason = format!(
                            "could not invalidate previous completion manifest before replacing products: {error}"
                        );
                        return finish(outcome, timer);
                    }
                    let (write_result, has_mask) = if crop_accepted {
                        let region = crop.expect("accepted crop has a region");
                        let excluded: Vec<bool> = (0..image.width * image.height)
                            .map(|i| !region.retains(i % image.width, i / image.width))
                            .collect();
                        (
                            write_cropped_products(
                                &image_path,
                                &mask_path,
                                &excluded,
                                &image,
                                region,
                            ),
                            true,
                        )
                    } else if matches!(config.processing.mode, ProcessingMode::Mask) {
                        (
                            write_products(&image_path, &mask_path, &image, &detection.mask),
                            true,
                        )
                    } else {
                        (create_kept_symlink(path, &image_path), false)
                    };
                    if let Err(e) = write_result {
                        if e.downcast_ref::<NoUnusedIntegerCode>().is_some() {
                            if options.overwrite {
                                if let Err(cleanup_error) = remove_existing_product(&image_path) {
                                    outcome.status = OutcomeKind::Error;
                                    outcome.reason = format!(
                                        "{e}; could not remove stale image product: {cleanup_error}"
                                    );
                                    return finish(outcome, timer);
                                }
                                if let Err(cleanup_error) = remove_existing_product(&mask_path) {
                                    outcome.status = OutcomeKind::Error;
                                    outcome.reason = format!(
                                        "{e}; could not remove stale mask product: {cleanup_error}"
                                    );
                                    return finish(outcome, timer);
                                }
                                if let Some(preview) = &png_path
                                    && let Err(cleanup_error) = remove_existing_product(preview)
                                {
                                    outcome.status = OutcomeKind::Error;
                                    outcome.reason = format!(
                                        "{e}; could not remove stale preview: {cleanup_error}"
                                    );
                                    return finish(outcome, timer);
                                }
                            }
                            if let Err(fallback_error) = create_kept_symlink(path, &image_path) {
                                outcome.status = OutcomeKind::Error;
                                outcome.reason = format!(
                                    "{e}; keeping the source was required but symlink publication failed: {fallback_error}"
                                );
                                return finish(outcome, timer);
                            }
                            outcome.status = OutcomeKind::Kept;
                            outcome.reason_code = "integer_blank_unavailable_kept";
                            outcome.reason = format!(
                                "{e}; source retained unchanged because this integer action cannot represent added undefined pixels"
                            );
                            outcome.output_image = Some(image_path);
                            if let Err(error) = publish_completion_manifest(
                                layout,
                                rel,
                                run_id,
                                &mut outcome,
                                false,
                            ) {
                                outcome.status = OutcomeKind::Error;
                                outcome.reason = format!(
                                    "source symlink was created but its completion manifest failed: {error:#}"
                                );
                            }
                            return finish(outcome, timer);
                        }
                        outcome.status = OutcomeKind::Error;
                        outcome.reason = e.to_string();
                        return finish(outcome, timer);
                    }
                    outcome.output_image = Some(image_path);
                    outcome.output_mask = has_mask.then_some(mask_path);
                    if has_mask {
                        outcome.masked_pixels = if crop_accepted {
                            outcome.artifact_pixels_removed + outcome.unmasked_pixels_sacrificed
                        } else {
                            n
                        };
                        outcome.mask_fraction = outcome.masked_pixels as f64
                            / (image.width * image.height - outcome.original_undefined_pixels)
                                .max(1) as f64;
                        outcome.applied_artifact_pixels = if crop_accepted {
                            outcome.artifact_pixels_removed
                        } else {
                            n
                        };
                        if crop_accepted {
                            outcome.reason_code = "crop_applied";
                        } else if matches!(config.processing.mode, ProcessingMode::Mask) {
                            outcome.reason_code = "mask_applied";
                        }
                    }
                    if crop_accepted && let Some(preview) = png_path {
                        let region = crop.expect("accepted crop has a region");
                        if let Err(error) = write_crop_preview(
                            &preview,
                            &image.values,
                            image.width,
                            image.height,
                            region,
                        ) {
                            outcome.status = OutcomeKind::Error;
                            outcome.reason = format!(
                                "FITS crop products were written, but PNG preview failed: {error:#}"
                            );
                            return finish(outcome, timer);
                        }
                        outcome.output_png = Some(preview);
                    }
                    if let Err(error) =
                        publish_completion_manifest(layout, rel, run_id, &mut outcome, has_mask)
                    {
                        outcome.status = OutcomeKind::Error;
                        outcome.reason = format!(
                            "products were written but the completion manifest failed: {error:#}"
                        );
                    }
                }
            } else if !options.dry_run {
                let image_path = match layout.product_path(ProductCategory::Image, rel) {
                    Ok(path) => path,
                    Err(error) => {
                        outcome.status = OutcomeKind::Error;
                        outcome.reason = error.to_string();
                        return finish(outcome, timer);
                    }
                };
                let manifest_path = match layout.manifest_path(rel) {
                    Ok(path) => path,
                    Err(error) => {
                        outcome.status = OutcomeKind::Error;
                        outcome.reason = error.to_string();
                        return finish(outcome, timer);
                    }
                };
                if !options.overwrite && (path_exists(&image_path) || path_exists(&manifest_path)) {
                    outcome.status = OutcomeKind::Error;
                    outcome.reason = "output exists; pass --overwrite".into();
                    return finish(outcome, timer);
                }
                if options.overwrite
                    && let Err(error) = remove_existing_product(&manifest_path)
                {
                    outcome.status = OutcomeKind::Error;
                    outcome.reason = format!(
                        "could not invalidate previous completion manifest before replacing symlink: {error}"
                    );
                    return finish(outcome, timer);
                }
                if let Err(e) = create_kept_symlink(path, &image_path) {
                    outcome.status = OutcomeKind::Error;
                    outcome.reason = e.to_string();
                    return finish(outcome, timer);
                }
                outcome.output_image = Some(image_path);
                if let Err(error) =
                    publish_completion_manifest(layout, rel, run_id, &mut outcome, false)
                {
                    outcome.status = OutcomeKind::Error;
                    outcome.reason = format!(
                        "source symlink was created but its completion manifest failed: {error:#}"
                    );
                }
            }
            outcome
        }
    };
    finish(result, timer)
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
    }
    Ok(())
}

fn publish_completion_manifest(
    layout: &OutputLayout,
    input_relative: &Path,
    run_id: &str,
    outcome: &mut FileOutcome,
    has_mask: bool,
) -> anyhow::Result<()> {
    let manifest_path = layout.manifest_path(input_relative)?;
    let mut products = Vec::new();
    for (kind, path) in [
        ("science_image", outcome.output_image.as_deref()),
        (
            "mask",
            has_mask.then_some(outcome.output_mask.as_deref()).flatten(),
        ),
        ("preview", outcome.output_png.as_deref()),
    ] {
        let Some(path) = path else { continue };
        let run_relative = path
            .strip_prefix(layout.run())
            .context("product path is outside its run directory")?;
        let metadata = fs::metadata(path)
            .with_context(|| format!("reading completed product metadata {}", path.display()))?;
        let symlink = fs::symlink_metadata(path)?.file_type().is_symlink();
        let content_sha256 = hash_file(path)?;
        if symlink
            && kind == "science_image"
            && Some(content_sha256.as_str()) != outcome.source_sha256.as_deref()
        {
            bail!("kept-image symlink no longer resolves to the parsed source bytes");
        }
        products.push(serde_json::json!({
            "kind": kind,
            "path": run_relative,
            "bytes": metadata.len(),
            "sha256": content_sha256,
            "symlink": symlink,
        }));
    }
    let status = match &outcome.status {
        OutcomeKind::Clipped => "clipped",
        OutcomeKind::Proposed => "proposed",
        OutcomeKind::Kept => "kept",
        OutcomeKind::Ignored => "ignored",
        OutcomeKind::Error => "error",
    };
    let contents = serde_json::to_vec_pretty(&serde_json::json!({
        "manifest_version": 3,
        "complete": true,
        "run_id": run_id,
        "input": input_relative,
        "input_sha256": outcome.source_sha256.as_deref(),
        "status": status,
        "completed_utc": Utc::now().to_rfc3339(),
        "products": products,
    }))?;
    let parent = manifest_path
        .parent()
        .context("completion manifest has no parent directory")?;
    let mut staged = NamedTempFile::new_in(parent)?;
    staged.write_all(&contents)?;
    staged.as_file().sync_all()?;
    staged
        .persist(&manifest_path)
        .map_err(|error| error.error)
        .with_context(|| format!("publishing completion manifest {}", manifest_path.display()))?;
    outcome.output_manifest = Some(manifest_path);
    Ok(())
}

fn hash_file(path: &Path) -> anyhow::Result<String> {
    let mut file = fs::File::open(path)
        .with_context(|| format!("opening product for SHA-256 {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
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
