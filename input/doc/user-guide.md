# User guide

## Current scope

The Rust CLI processes FITS files with exactly one two-dimensional primary image. It accepts signed integer images (BITPIX 8/16/32/64) and floating images (BITPIX -32/-64). Multi-HDU files, cubes, zero-sized arrays, non-image primary HDUs and gzip-compressed FITS inputs are reported as ignored. Corrupt files and resource violations are errors.

The detector is an initial conservative tile-based method. It compares robust tile medians and noise with an interior reference, requires strong contrast plus spatial coherence or reduced noise, and floods only candidate tiles connected to the image border. It can describe asymmetric corners, one-sided strips, rectangular and partly/circular patterns through the resulting mask. Detection scores are diagnostic Z-like scores, capped at 1,000,000, not probabilities. Constant regions have no defined Pearson correlation; a clear contrast plus zero measured dispersion can still qualify. Real instrument examples and held-out evaluation are required before scientific use.

The tool marks clipped samples as undefined in a copy of the original image. It preserves dimensions and WCS. It writes a companion uint8 FITS mask (1 means newly clipped) and updates FITS checksums. Integer outputs use an existing BLANK sentinel or a collision-free stored integer value; unavailable sentinel representations fail safely. All retained stored integer pixels are verified unchanged. Floating retained values are also checked after writing. Cropping is not implemented.

## Install and run

Install Rust 1.88 or newer with a stable toolchain. FITS I/O uses a pure-Rust crate and requires no CFITSIO installation. Operational settings and science rules are separate files. Build and run from the repository root:

```bash
cargo build --release
cargo run --release -- --input-dir input/data --output-dir output --config input/config.toml
cargo run --release -- --input-dir input/data --output-dir output --recursive --config input/config.toml --heuristics input/heuristics.toml
```

For a dry run that writes reports and logs without science image or mask products:

```bash
cargo run --release -- --input-dir input/data --output-dir output --dry-run
```

Use `--recursive` to search subdirectories, `--heuristics PATH` to select a separate science-rule file, `--threads N` to cap workers, `--memory-budget-mib N` to set the total image workspace budget, and `--overwrite` to replace existing products. `--help` lists the current options. Input and output trees may not overlap. Originals are never edited.

## Products

Each run receives a UTC run ID. Processed images and masks retain the input-relative path under `output/images/` and `output/masks/`. `output/reports/<run-id>/summary.json` contains totals and per-file details; `per-image.csv` is convenient for spreadsheets. Activity events live in `output/logger/` as JSON Lines with an ISO-8601 UTC timestamp on each entry. A candidate finishes as clipped, kept, ignored or error. Exit status is 0 for no per-file errors, 1 when one or more files fail, and 2 for fatal startup/configuration errors.

## Configuration

Edit `input/config.toml` for worker/discovery/memory settings and `input/heuristics.toml` for clipping decisions; see [configuration reference](configuration.md). Invalid values stop the run before input processing. Effective settings and hashes are recorded in the run-start log. Detector rules are reviewable thresholds, not validated instrument-independent settings or calibrated probabilities. Ambiguous images are kept unchanged.

## Safe interpretation

Inspect the mask FITS and report before scientific analysis. A mask describes rejected pixels; downstream photometry should exclude these values and account for masked aperture area. Smooth sky structure, nebulosity, extended sources, PSF wings and moving targets can resemble correlated patterns. Retain images when detector evidence is ambiguous; compare representative data with instrument-specific knowledge. This release is an engineering prototype, not a validated observatory pipeline.
