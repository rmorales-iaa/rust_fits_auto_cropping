//! CLI arguments. Directory paths are required to keep input and output provenance explicit.
use crate::config::ProcessingMode;
use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "rust_fits_auto_cropping",
    version = env!("RUST_FITS_AUTO_CROPPING_VERSION"),
    about = "Detect and mask or crop border artifacts in single-HDU two-dimensional FITS images.",
    long_about = "Detect statistically supported border patterns using image pixel values. Non-structural FITS header metadata is not detector evidence; malformed non-structural cards use temporary parser placeholders and are restored byte-for-byte in generated headers. Structural cards needed to locate and decode pixels must remain readable. Mask mode preserves image dimensions; crop mode writes the maximum-area clean axis-aligned rectangle, updates NAXIS1/2 and shifts CRPIX by the crop origin, then applies a configurable sacrifice limit and loss-versus-removal utility rule. The sacrifice caps are 35%, 35%, 35%, and 47% for one, two, three, and four-or-more accepted distinct border-connected areas. A connected area touching multiple edges counts once.",
    after_help = "Examples:\n  rust_fits_auto_cropping --input-dir input/data --output-dir output\n  rust_fits_auto_cropping --input-dir input/data --output-dir output --recursive --mode crop --threads 4\n  rust_fits_auto_cropping --input-dir input/data --output-dir output --recursive --mode crop --png\n  rust_fits_auto_cropping --input-dir input/data --output-dir output --recursive --mode crop --flat-output --sanitize-output --png\n  rust_fits_auto_cropping --input-dir input/data --output-dir output --dry-run\n\nBy default, each run is stored under output/runs/<run-id>/ with images/, summary.json, per-image.csv, and an ISO-dated activity log. --flat-output writes science images directly under the given output directory while keeping previews/ and reports in their own locations; it can reuse the output root on later runs. --sanitize-output explicitly deletes every existing entry in that output root before processing. PNG previews are disabled by default; --png requires crop mode and writes a before/after ZScale preview for each accepted crop, creating previews/ only when needed. Crop mode reads heuristics.maximum_residual_artifact_fraction (5% in the shipped rules; zero selects a strict clean rectangle), always excludes original undefined pixels, and reports its bounded search diagnostics. No manifests/ directory or per-input completion-marker files are generated. The final summary and activity log report per-input statuses, decisions, and output paths. Temporary masks/ and reason_masks/ sidecars are removed before reports and process exit. Every discovered FITS input gets one image entry: under images/ by default or directly under the output root with --flat-output. Decodable images are processed even with malformed non-structural header cards; original card bytes are preserved. Unsupported structural layouts, compressed files, and multi-HDU files are annotated and preserved as symlinks. The output root is checked for symlinks and input/output overlap; unsafe relative paths and symlinked managed directories are rejected. Existing output files are preserved unless --overwrite is passed. --reason-mask creates a temporary FITS reason-code plane for validation; it is removed before exit. Generated FITS products receive CREATOR, DATE and HISTORY provenance cards. Exit codes: 0 complete; 1 one or more file errors; 2 startup/configuration error."
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
    /// Product mode: preserve dimensions with masks, or crop to the maximum-area fully clean rectangle.
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
    /// Worker threads (default 4 in config); 0 requests all logical CPUs, subject to CPU and memory limits.
    #[arg(long)]
    pub threads: Option<usize>,
    /// Inspect and report candidates without writing images, symlinks or masks.
    #[arg(long)]
    pub dry_run: bool,
    /// Total memory budget shared by image workers (MiB).
    #[arg(long)]
    pub memory_budget_mib: Option<usize>,
    /// Allow existing output products to be replaced.
    #[arg(long)]
    pub overwrite: bool,
}
