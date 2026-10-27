//! Command-line entry point for FITS border artifact masking.
mod cli;
mod config;
mod detection;
mod fits_image;
mod logger;
mod model;
mod pipeline;
mod report;
mod writer;

use clap::Parser;

fn main() {
    let args = cli::Args::parse();
    if let Err(error) = pipeline::run(args) {
        eprintln!("Error: {error}");
        std::process::exit(error.exit_code());
    }
}
