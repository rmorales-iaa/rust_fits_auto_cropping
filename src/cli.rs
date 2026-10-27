//! CLI arguments. Directory paths are required to keep input and output provenance explicit.
use crate::config::ProcessingMode;
use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "fits-auto-clipping",
    version = env!("FITS_AUTO_CLIPPING_VERSION"),
    about = "Conservatively detect and mask or crop border artifacts in single-HDU 2D integer and floating-point FITS images."
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
    /// Product mode: preserve dimensions with masks, or crop to a clean rectangle.
    #[arg(long, value_enum)]
    pub mode: Option<ProcessingMode>,
    /// Include nested directories in input discovery.
    #[arg(long)]
    pub recursive: bool,
    /// Number of workers (0 uses available logical CPUs).
    #[arg(long)]
    pub threads: Option<usize>,
    /// Inspect and report candidates without writing image or mask products.
    #[arg(long)]
    pub dry_run: bool,
    /// Total memory budget shared by image workers (MiB).
    #[arg(long)]
    pub memory_budget_mib: Option<usize>,
    /// Allow existing output products to be replaced.
    #[arg(long)]
    pub overwrite: bool,
}
