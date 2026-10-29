//! Directory orchestration, bounded parallel processing, and report/log finalization.
use crate::{
    cli::Args,
    config::{Config, HeuristicsConfig, ProcessingMode},
    crop::{CropPolicy, largest_clean_region, residual_tolerant_rectangle},
    detection::detect_border,
    durability::sync_parent,
    fits_image::{NoUnusedIntegerCode, is_gzip_compressed, read_image},
    logger::Logger,
    model::{FileOutcome, OutcomeKind},
    output_layout::{OutputLayout, ProductCategory},
    preview::write_crop_preview,
    reason_mask, report,
    workers::effective_worker_count,
    writer::{write_cropped_product, write_masked_product, write_reason_mask},
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
        "version":env!("RUST_FITS_AUTO_CROPPING_VERSION"),"algorithm_version":heuristics.heuristics.reference_model.algorithm_version(heuristics.heuristics.evidence_model),"git_revision":env!("RUST_FITS_GIT_REVISION"),"git_dirty":env!("RUST_FITS_GIT_DIRTY")=="true","executable_sha256":executable_sha256,"threads":config.processing.threads,"effective_config":config,"effective_heuristics":heuristics,"mode":config.processing.mode,"recursive":config.processing.recursive,"dry_run":args.dry_run,"reason_mask":args.reason_mask}))?;
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
            algorithm_version: heuristics
                .heuristics
                .reference_model
                .algorithm_version(heuristics.heuristics.evidence_model),
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
            let detect_timer = Instant::now();
            let detection = detect_border(
                &image.values,
                image.width,
                image.height,
                &heuristics.heuristics,
            );
            let detect_ms = elapsed_ms(detect_timer);
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
            outcome.stage_ms.detect = detect_ms;
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
                // Candidate and accepted masks only include originally defined pixels.
                outcome.candidate_undefined_pixels = 0;
                outcome.candidate_proposed_overlap_pixels = n;
                outcome.proposed_undefined_pixels = detection
                    .mask
                    .iter()
                    .zip(&image.values)
                    .filter(|(candidate, value)| **candidate && !value.is_finite())
                    .count();
                let geometry_timer = Instant::now();
                let crop_search = if matches!(config.processing.mode, ProcessingMode::Crop)
                    && image.crop_wcs_unsupported.is_none()
                {
                    let crop_exclusions: Vec<bool> = detection
                        .mask
                        .iter()
                        .zip(&image.values)
                        .map(|(&candidate, value)| candidate || !value.is_finite())
                        .collect();
                    let undefined: Vec<bool> =
                        image.values.iter().map(|v| !v.is_finite()).collect();
                    largest_clean_region(&crop_exclusions, image.width, image.height).and_then(
                        |anchor| {
                            residual_tolerant_rectangle(
                                &detection.mask,
                                &undefined,
                                image.width,
                                image.height,
                                anchor,
                                CropPolicy {
                                    maximum_residual_artifact_fraction: heuristics
                                        .heuristics
                                        .maximum_residual_artifact_fraction,
                                    maximum_unmasked_sacrifice_fraction: heuristics
                                        .heuristics
                                        .crop_sacrifice_limit(detection.accepted_components),
                                    evidence_ratio: detection.evidence_ratio,
                                    valid_pixel_loss_cost: heuristics
                                        .heuristics
                                        .valid_pixel_loss_cost,
                                    minimum_net_utility: heuristics.heuristics.minimum_net_utility,
                                },
                            )
                        },
                    )
                } else {
                    None
                };
                outcome.stage_ms.geometry = elapsed_ms(geometry_timer);
                let selected_crop =
                    crop_search.map(|search| search.accepted.unwrap_or(search.proposed));
                let crop = selected_crop.map(|candidate| candidate.rect);
                let crop_wcs_mask_fallback = matches!(config.processing.mode, ProcessingMode::Crop)
                    && image.crop_wcs_unsupported.is_some();
                let crop_accepted = crop_search.is_some_and(|search| search.accepted.is_some());
                if let (Some(search), Some(candidate)) = (crop_search, selected_crop) {
                    outcome.crop_search_candidates = search.candidates_examined;
                    outcome.crop_search_step_px = search.pixel_step;
                    outcome.crop_residual_artifact_fraction = candidate.residual_artifact_fraction;
                }
                if matches!(config.processing.mode, ProcessingMode::Crop) && crop.is_none() {
                    if crop_wcs_mask_fallback {
                        outcome.reason_code = "crop_wcs_mask_fallback";
                        outcome.reason.push_str(
                            "; crop was omitted and the detected border pixels were applied as a dimension-preserving full-frame mask",
                        );
                    } else {
                        outcome.status = OutcomeKind::Kept;
                        outcome.reason_code = "crop_geometry_unavailable";
                        outcome.reason.push_str(
                            "; no positive-area rectangle excludes every detected artifact pixel; source retained unchanged",
                        );
                    }
                }
                if options.dry_run && detection.accepted && outcome.status == OutcomeKind::Clipped {
                    outcome.status = OutcomeKind::Proposed;
                    outcome.reason_code = "dry_run_proposal";
                }
                if let Some(rect) = crop {
                    let total = image.width * image.height;
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
                    outcome.crop_artifact_pixels_retained = detection
                        .mask
                        .iter()
                        .zip(&excluded)
                        .zip(&image.values)
                        .filter(|((artifact, excluded), value)| {
                            **artifact && !**excluded && value.is_finite()
                        })
                        .count();
                    let chosen = selected_crop.expect("crop rectangle came from a search result");
                    outcome.artifact_benefit = chosen.artifact_benefit;
                    outcome.valid_pixel_loss = chosen.valid_pixel_loss;
                    outcome.net_utility = chosen.net_utility;
                    let crop_sacrifice_fraction = chosen.sacrifice_fraction;
                    let crop_sacrifice_limit = heuristics
                        .heuristics
                        .crop_sacrifice_limit(detection.accepted_components);
                    if !crop_accepted {
                        outcome.status = OutcomeKind::Kept;
                        outcome.reason_code = if crop_sacrifice_fraction > crop_sacrifice_limit {
                            "crop_sacrifice_limit"
                        } else {
                            "crop_utility_rejected"
                        };
                    }
                    outcome.reason.push_str(&format!(
                        "; proposed {} crop origin ({}, {}), bounds {}x{}, retained {} pixels; removes {} artifact pixels, retains {} flagged pixels ({:.2}% of crop; limit {:.2}%), and sacrifices {} unmasked pixels ({:.1}% of remaining defined area; limit {:.1}% for {} accepted border-connected area(s)); utility {:.4} (benefit {:.4} - loss {:.4}); residual search evaluated {} rectangles at {} px step {}",
                        rect.shape_name(), bounds.x, bounds.y, bounds.width, bounds.height, rect.retained_pixels(),
                        outcome.artifact_pixels_removed, outcome.crop_artifact_pixels_retained,
                        100.0 * outcome.crop_residual_artifact_fraction,
                        100.0 * heuristics.heuristics.maximum_residual_artifact_fraction,
                        outcome.unmasked_pixels_sacrificed,
                        100.0 * crop_sacrifice_fraction,
                        100.0 * crop_sacrifice_limit,
                        detection.accepted_components,
                        outcome.net_utility, outcome.artifact_benefit, outcome.valid_pixel_loss,
                        outcome.crop_search_candidates, outcome.crop_search_step_px,
                        if crop_accepted { "accepted" } else { "rejected; source retained" }
                    ));
                }
                let write_timer = Instant::now();
                if !options.dry_run {
                    let image_path_result = if crop_accepted {
                        layout.cropped_image_path(rel)
                    } else {
                        layout.product_path(ProductCategory::Image, rel)
                    };
                    let image_path = match image_path_result {
                        Ok(path) => path,
                        Err(error) => {
                            outcome.status = OutcomeKind::Error;
                            outcome.reason_code = "output_path_error";
                            outcome.reason = error.to_string();
                            return finish(outcome, timer);
                        }
                    };
                    let png_path = if options.png_preview && crop_accepted {
                        match layout.preview_path(rel) {
                            Ok(path) => Some(path),
                            Err(error) => {
                                outcome.status = OutcomeKind::Error;
                                outcome.reason_code = "output_path_error";
                                outcome.reason = error.to_string();
                                return finish(outcome, timer);
                            }
                        }
                    } else {
                        None
                    };
                    let wants_reason_mask = options.reason_mask
                        && (crop_accepted
                            || matches!(config.processing.mode, ProcessingMode::Mask));
                    let reason_mask_path = if wants_reason_mask {
                        match layout.reason_mask_path(rel) {
                            Ok(path) => Some(path),
                            Err(error) => {
                                outcome.status = OutcomeKind::Error;
                                outcome.reason_code = "output_path_error";
                                outcome.reason = error.to_string();
                                return finish(outcome, timer);
                            }
                        }
                    } else {
                        None
                    };
                    if !options.overwrite
                        && (path_exists(&image_path)
                            || reason_mask_path
                                .as_ref()
                                .is_some_and(|path| path_exists(path))
                            || png_path.as_ref().is_some_and(|path| path_exists(path)))
                    {
                        outcome.status = OutcomeKind::Error;
                        outcome.reason_code = "output_exists_error";
                        outcome.reason = "output exists; pass --overwrite".into();
                        return finish(outcome, timer);
                    }
                    let (write_result, has_mask) = if crop_accepted {
                        let region = crop.expect("accepted crop has a region");
                        (write_cropped_product(&image_path, &image, region), true)
                    } else if matches!(config.processing.mode, ProcessingMode::Mask)
                        || crop_wcs_mask_fallback
                    {
                        (
                            write_masked_product(&image_path, &image, &detection.mask),
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
                                    outcome.reason_code = "output_cleanup_error";
                                    outcome.reason = format!(
                                        "{e}; could not remove stale image product: {cleanup_error}"
                                    );
                                    return finish(outcome, timer);
                                }
                                if let Some(reason_mask) = &reason_mask_path
                                    && let Err(cleanup_error) = remove_existing_product(reason_mask)
                                {
                                    outcome.status = OutcomeKind::Error;
                                    outcome.reason_code = "output_cleanup_error";
                                    outcome.reason = format!(
                                        "{e}; could not remove stale reason-plane product: {cleanup_error}"
                                    );
                                    return finish(outcome, timer);
                                }
                                if let Some(preview) = &png_path
                                    && let Err(cleanup_error) = remove_existing_product(preview)
                                {
                                    outcome.status = OutcomeKind::Error;
                                    outcome.reason_code = "output_cleanup_error";
                                    outcome.reason = format!(
                                        "{e}; could not remove stale preview: {cleanup_error}"
                                    );
                                    return finish(outcome, timer);
                                }
                            }
                            if let Err(fallback_error) = create_kept_symlink(path, &image_path) {
                                outcome.status = OutcomeKind::Error;
                                outcome.reason_code = "kept_symlink_error";
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
                            return finish(outcome, timer);
                        }
                        outcome.status = OutcomeKind::Error;
                        outcome.reason_code = if has_mask {
                            "product_write_error"
                        } else {
                            "kept_symlink_error"
                        };
                        outcome.reason = e.to_string();
                        return finish(outcome, timer);
                    }
                    outcome.output_image = Some(image_path);
                    if let Some(reason_path) = reason_mask_path {
                        let crop_exclusion = crop_accepted.then(|| {
                            let region = crop.expect("accepted crop has a region");
                            (0..image.width * image.height)
                                .map(|i| !region.retains(i % image.width, i / image.width))
                                .collect::<Vec<_>>()
                        });
                        let action_exclusion = crop_exclusion.as_deref().unwrap_or(&detection.mask);
                        let codes = match reason_mask::build(
                            &image.values,
                            &detection.mask,
                            action_exclusion,
                        ) {
                            Ok(codes) => codes,
                            Err(error) => {
                                outcome.status = OutcomeKind::Error;
                                outcome.reason_code = "reason_plane_error";
                                outcome.reason = error.to_string();
                                return finish(outcome, timer);
                            }
                        };
                        if let Err(error) = write_reason_mask(
                            &image.source_fits.primary().header,
                            &reason_path,
                            image.width,
                            image.height,
                            &codes,
                        ) {
                            outcome.status = OutcomeKind::Error;
                            outcome.reason_code = "reason_plane_write_error";
                            outcome.reason = format!(
                                "science/mask products were written but the optional reason plane failed: {error:#}"
                            );
                            return finish(outcome, timer);
                        }
                        outcome.output_reason_mask = Some(reason_path);
                    }
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
                        outcome.applied_artifact_undefined_pixels = detection
                            .mask
                            .iter()
                            .zip(&image.values)
                            .filter(|(artifact, value)| **artifact && !value.is_finite())
                            .count();
                        if crop_accepted {
                            outcome.crop_applied_undefined_pixels =
                                outcome.crop_excluded_undefined_pixels;
                            outcome.reason_code = "crop_applied";
                        } else if matches!(config.processing.mode, ProcessingMode::Mask) {
                            outcome.reason_code = "mask_applied";
                        } else if crop_wcs_mask_fallback {
                            outcome.reason_code = "crop_wcs_mask_fallback";
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
                            outcome.reason_code = "preview_write_error";
                            outcome.reason = format!(
                                "FITS crop products were written, but PNG preview failed: {error:#}"
                            );
                            return finish(outcome, timer);
                        }
                        outcome.output_png = Some(preview);
                    }
                }
                outcome.stage_ms.write = elapsed_ms(write_timer);
            } else if !options.dry_run {
                let image_path = match layout.product_path(ProductCategory::Image, rel) {
                    Ok(path) => path,
                    Err(error) => {
                        outcome.status = OutcomeKind::Error;
                        outcome.reason_code = "output_path_error";
                        outcome.reason = error.to_string();
                        return finish(outcome, timer);
                    }
                };
                if !options.overwrite && path_exists(&image_path) {
                    outcome.status = OutcomeKind::Error;
                    outcome.reason_code = "output_exists_error";
                    outcome.reason = "output exists; pass --overwrite".into();
                    return finish(outcome, timer);
                }
                if let Err(e) = create_kept_symlink(path, &image_path) {
                    outcome.status = OutcomeKind::Error;
                    outcome.reason_code = "kept_symlink_error";
                    outcome.reason = e.to_string();
                    return finish(outcome, timer);
                }
                outcome.output_image = Some(image_path);
            }
            outcome
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
