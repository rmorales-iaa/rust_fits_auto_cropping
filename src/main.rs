//! Command-line entry point for FITS border artifact masking.
mod cli;
mod config;
mod crop;
mod detection;
mod durability;
mod fits_image;
mod logger;
mod model;
mod output_layout;
mod pipeline;
mod preview;
mod reason_mask;
mod report;
mod workers;
mod writer;

use clap::Parser;

fn main() {
    let args = cli::Args::parse();
    if let Err(error) = pipeline::run(args) {
        eprintln!("Error: {error}");
        std::process::exit(error.exit_code());
    }
}
