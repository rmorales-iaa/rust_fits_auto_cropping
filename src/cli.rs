//! CLI arguments. Directory paths are required to keep input and output provenance explicit.
use crate::config::ProcessingMode;
use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "rust_fits_auto_cropping",
    version = env!("RUST_FITS_AUTO_CROPPING_VERSION"),
    about = "Detect and mask or crop border artifacts in single-HDU two-dimensional FITS images.",
    long_about = "Detect statistically supported border patterns using image pixel values. Non-structural FITS header metadata is not evidence for the detector. Mask mode preserves image dimensions; crop mode compares maximum-area clean rectangles and artifact-free circles and applies a configurable loss-versus-removal utility rule.",
    after_help = "Examples:\n  rust_fits_auto_cropping --input-dir input/data --output-dir output\n  rust_fits_auto_cropping --input-dir input/data --output-dir output --recursive --mode crop --threads 4\n  rust_fits_auto_cropping --input-dir input/data --output-dir output --recursive --mode crop --png\n  rust_fits_auto_cropping --input-dir input/data --output-dir output --dry-run\n\nEvery run is stored under output/runs/<run-id>/ with images/, masks/, optional previews/, summary.json, per-image.csv and an ISO-dated activity log. The output root is validated and prior run directories are preserved. --png requires crop mode and writes a labeled before/after ZScale PNG for each accepted crop under previews/. Unchanged eligible images are symlinks under images/ and keep their source headers unchanged. Generated FITS products receive CREATOR, DATE and HISTORY provenance cards. Multi-HDU and compressed FITS inputs are ignored. Exit codes: 0 complete; 1 one or more file errors; 2 startup/configuration error."
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
    /// Product mode: preserve dimensions with masks, or crop to the larger clean rectangle/circle.
    #[arg(long, value_enum)]
    pub mode: Option<ProcessingMode>,
    /// Write a ZScale before/after PNG preview for every accepted crop (requires crop mode).
    #[arg(long)]
    pub png: bool,
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
