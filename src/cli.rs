//! CLI arguments. Directory paths are required to keep input and output provenance explicit.
use crate::config::ProcessingMode;
use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "rust_fits_auto_cropping",
    version = env!("RUST_FITS_AUTO_CROPPING_VERSION"),
    about = "Detect and mask or crop border artifacts in single-HDU two-dimensional FITS images.",
    long_about = "Detect statistically supported border patterns using image pixel values. Non-structural FITS header metadata is not detector evidence; generated FITS products keep the source header cards byte-for-byte (including malformed non-structural cards) and change only NAXIS1/2, CRPIX1/2 (all WCS letters), existing LTV1/2, a needed BLANK and CHECKSUM/DATASUM; all program information is appended as MO_ keywords. Structural cards needed to locate and decode pixels must remain readable. Mask mode preserves image dimensions. Crop mode starts from the maximum-area clean axis-aligned rectangle, may enlarge it within the configured residual flagged fraction, updates NAXIS1/2 and shifts CRPIX by the crop origin, then applies the configured valid-pixel sacrifice cap (10% shipped), utility rule and exchange rate (0.5 shipped). Shipped crop mode retains no flagged samples; crop_masked mode tolerates up to 5% retained flagged samples and writes them undefined (BLANK/NaN). A rectangle covering the whole frame is written as a mask product instead of a crop. When no rectangle passes, heuristics.crop_fallback selects keep, mask, or bbox_mask (crop to the bounding box of retained unflagged samples and mark remaining flags undefined; shipped default).",
    after_help = "Examples:\n  rust_fits_auto_cropping --input-dir input/data --output-dir output\n  rust_fits_auto_cropping --input-dir input/data --output-dir output --recursive --mode crop --threads 4\n  rust_fits_auto_cropping --input-dir input/data --output-dir output --recursive --mode crop --png\n  rust_fits_auto_cropping --input-dir input/data --output-dir output --recursive --mode crop --flat-output --sanitize-output --png\n  rust_fits_auto_cropping --input-dir input/data --output-dir output --dry-run\n\nBy default, each run is stored under output/runs/<run-id>/ with images/, summary.json, per-image.csv, and an ISO-dated activity log. --flat-output writes science images directly under the given output directory while keeping previews/ and reports in their own locations; it can reuse the output root on later runs. --sanitize-output explicitly deletes every existing entry in that output root before processing. PNG previews are disabled by default; --png requires crop or crop_masked mode and writes a before/after ZScale preview for each geometry-changing product, creating previews/ only when needed. Crop mode reads heuristics.maximum_residual_artifact_fraction (0 in the shipped rules: a strict clean rectangle) and crop_masked mode heuristics.crop_masked_residual_fraction (5%); both always exclude original undefined pixels and reports its bounded search diagnostics. No manifests/ directory or per-input completion-marker files are generated. The final summary and activity log report per-input statuses, decisions, and output paths. Action masks are verified in memory; temporary reason_masks/ sidecars (only with --reason-mask) are removed before reports and process exit. Every discovered FITS input gets one image entry: under images/ by default or directly under the output root with --flat-output. Decodable images are processed even with malformed non-structural header cards; original card bytes are preserved. Unsupported structural layouts, compressed files, and multi-HDU files are annotated and preserved as symlinks. The output root is checked for symlinks and input/output overlap; unsafe relative paths and symlinked managed directories are rejected. Existing output files are preserved unless --overwrite is passed. --reason-mask creates a temporary FITS reason-code plane for validation; it is removed before exit. Generated FITS products keep the source header verbatim apart from the geometry, BLANK and checksum edits and carry MO_ provenance keywords (MO_ACT, MO_ALGO, MO_PROG, MO_DATE, MO_SHA, MO_FILE). Cropped products have MO_CROP = T with MO_SEC, MO_ONAX1/2 and MO_X0/Y0 on the original frame; tools/apply_crop.py applies the same crop to calibration frames. Memory: each image reserves its estimated peak working set from --memory-budget-mib before pixels are allocated, so large and small images share the budget by weight. Exit codes: 0 complete; 1 one or more file errors; 2 startup/configuration error."
)]
pub struct Args {
    /// Input directory containing FITS images.
    #[arg(long)]
    pub input_dir: PathBuf,
    /// Output root; runs are isolated by default unless --flat-output is used.
    #[arg(long)]
    pub output_dir: PathBuf,
    /// Write science images at the output root; previews stay in previews/.
    #[arg(long)]
    pub flat_output: bool,
    /// Delete all existing entries in the output root before processing.
    #[arg(long)]
    pub sanitize_output: bool,
    /// TOML configuration. Defaults to input/config.toml when present.
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// Science heuristic rules. Defaults to input/heuristics.toml.
    #[arg(long)]
    pub heuristics: Option<PathBuf>,
    /// Product mode: mask (preserve dimensions), crop (rectangle), or crop_masked (rectangle with retained flags undefined).
    #[arg(long, value_enum)]
    pub mode: Option<ProcessingMode>,
    /// Opt in to ZScale before/after PNG previews with green retained and red cropped outlines (crop mode only; disabled by default).
    #[arg(long)]
    pub png: bool,
    /// Build temporary FITS pixel-action reason planes for accepted actions (removed before exit).
    #[arg(long)]
    pub reason_mask: bool,
    /// Include nested directories in input discovery.
    #[arg(long)]
    pub recursive: bool,
    /// Worker threads (default 4 in config); 0 requests all logical CPUs, limited by the CPU count; memory is governed separately by weighted per-image admission.
    #[arg(long)]
    pub threads: Option<usize>,
    /// Inspect and report candidates without writing images, symlinks or masks.
    #[arg(long)]
    pub dry_run: bool,
    /// Total memory budget (MiB); each image reserves its estimated peak working set before decoding and waits while the budget is full.
    #[arg(long)]
    pub memory_budget_mib: Option<usize>,
    /// Allow existing output products to be replaced.
    #[arg(long)]
    pub overwrite: bool,
}
