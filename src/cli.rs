//! CLI arguments. Directory paths are required to keep input and output provenance explicit.
use crate::config::ProcessingMode;
use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "rust_fits_auto_cropping",
    version = env!("RUST_FITS_AUTO_CROPPING_VERSION"),
    about = "Detect and mask or crop border artifacts in single-HDU two-dimensional FITS images.",
    long_about = "Detect statistically supported border patterns using image pixel values. Non-structural FITS header metadata is not evidence for the detector. Mask mode preserves image dimensions; crop mode writes the maximum-area axis-aligned rectangle excluding proposed artifacts and originally undefined pixels, updates NAXIS1/2 and shifts CRPIX by the crop origin, then applies a configurable clean-area sacrifice limit and loss-versus-removal utility rule.",
    after_help = "Examples:\n  rust_fits_auto_cropping --input-dir input/data --output-dir output\n  rust_fits_auto_cropping --input-dir input/data --output-dir output --recursive --mode crop --threads 4\n  rust_fits_auto_cropping --input-dir input/data --output-dir output --recursive --mode crop --png --reason-mask\n  rust_fits_auto_cropping --input-dir input/data --output-dir output --dry-run\n\nEvery normal run is stored under output/runs/<run-id>/ with images/, previews/ and per-input manifests/, summary.json, per-image.csv and an ISO-dated activity log. Every discovered FITS input gets one images/ entry: processed products are generated normally, while malformed, compressed, multi-HDU, and other unsupported inputs are annotated and preserved as symlinks to their originals. summary.json reports total inputs and output_images separately. Dry-run deliberately writes no image products. Transient masks/ and reason_masks/ sidecars are removed before reports are written and before exit. A .complete.json manifest is written only after that input's retained products pass verification; missing markers mean incomplete or failed products. The output root is checked for symlinks and input/output overlap; unsafe relative paths and symlinked managed directories are rejected. Existing runs and unrelated output-root files are preserved. --png requires crop mode and writes an RGB before/after ZScale preview for every accepted crop; each panel uses an independent ZScale fitted to its own FITS pixels, and the left panel outlines retained crop pixels in green and adjacent excluded pixels in red. --reason-mask builds a temporary FITS reason-code plane for validation; it is removed before exit. Generated FITS products receive CREATOR, DATE and HISTORY provenance cards. Exit codes: 0 complete; 1 one or more file errors; 2 startup/configuration error."
)]
pub struct Args {
    /// Input directory containing FITS images.
    #[arg(long)]
    pub input_dir: PathBuf,
    /// Output root; each run is isolated under runs/<UTC-run-id>/.
    #[arg(long)]
    pub output_dir: PathBuf,
    /// TOML configuration. Defaults to input/config.toml when present.
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// Science heuristic rules. Defaults to input/heuristics.toml.
    #[arg(long)]
    pub heuristics: Option<PathBuf>,
    /// Product mode: preserve dimensions with masks, or crop to the maximum-area fully clean rectangle.
    #[arg(long, value_enum)]
    pub mode: Option<ProcessingMode>,
    /// Write a ZScale before/after PNG preview with green retained and red cropped outlines (crop mode only).
    #[arg(long)]
    pub png: bool,
    /// Build temporary FITS pixel-action reason planes for accepted actions (removed before exit).
    #[arg(long)]
    pub reason_mask: bool,
    /// Include nested directories in input discovery.
    #[arg(long)]
    pub recursive: bool,
    /// Number of workers (0 uses available logical CPUs).
    #[arg(long)]
    pub threads: Option<usize>,
    /// Inspect and report candidates without writing images, symlinks or masks.
    #[arg(long)]
    pub dry_run: bool,
    /// Total memory budget shared by image workers (MiB).
    #[arg(long)]
    pub memory_budget_mib: Option<usize>,
    /// Allow conflicting products within the current isolated run to be replaced.
    #[arg(long)]
    pub overwrite: bool,
}
