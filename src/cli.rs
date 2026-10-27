//! CLI arguments. Directory paths are required to keep input and output provenance explicit.
use crate::config::ProcessingMode;
use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "fits-auto-clipping",
    version = env!("FITS_AUTO_CLIPPING_VERSION"),
    about = "Detect and mask or crop border artifacts in single-HDU two-dimensional FITS images.",
    long_about = "Detect statistically supported border patterns using image pixel values. Non-structural FITS header metadata is not evidence for the detector. Mask mode preserves image dimensions; crop mode compares maximum-area clean rectangles and artifact-free circles and applies a configurable loss-versus-removal utility rule.",
    after_help = "Examples:\n  fits-auto-clipping --input-dir input/data --output-dir output\n  fits-auto-clipping --input-dir input/data --output-dir output --recursive --mode crop --threads 4\n  fits-auto-clipping --input-dir input/data --output-dir output --dry-run\n\nUnchanged eligible images are symlinks under output/images/ and keep their source headers unchanged. Generated FITS products receive CREATOR, DATE and HISTORY provenance cards. Multi-HDU and compressed FITS inputs are ignored. Exit codes: 0 complete; 1 one or more file errors; 2 startup/configuration error."
)]
pub struct Args {
    /// Input directory containing FITS images.
    #[arg(long)]
    pub input_dir: PathBuf,
    /// Output directory for images, masks, reports and logger/.
    #[arg(long)]
    pub output_dir: PathBuf,
    /// TOML configuration. Defaults to input/config.toml when present.
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// Science heuristic rules. Defaults to input/heuristics.toml.
    #[arg(long)]
    pub heuristics: Option<PathBuf>,
    /// Product mode: preserve dimensions with masks, or crop to the larger clean rectangle/circle.
    #[arg(long, value_enum)]
    pub mode: Option<ProcessingMode>,
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
    /// Allow existing output products to be replaced.
    #[arg(long)]
    pub overwrite: bool,
}
