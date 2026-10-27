# User guide

## Current scope

The Rust CLI processes FITS files with exactly one two-dimensional primary image. It accepts signed integer images (BITPIX 8/16/32/64) and floating images (BITPIX -32/-64). Multi-HDU files, cubes, zero-sized arrays, non-image primary HDUs and gzip-compressed FITS inputs are reported as ignored. Corrupt files and resource violations are errors.

The detector is an initial conservative tile-based method. It compares robust tile medians and noise with an interior reference, requires strong contrast plus spatial coherence or reduced noise, and floods only candidate tiles connected to the image border. It can describe asymmetric corners, one-sided strips, rectangular and partly/circular patterns through the resulting mask. Detection evidence comes from pixels only: observation labels, WCS, instrument names and other descriptive header cards do not affect pattern classification. FITS structural, storage-scaling and null-value cards are still needed to decode and interpret the sample array. Detection scores are diagnostic Z-like scores, capped at 1,000,000, not probabilities. Constant regions have no defined Pearson correlation; a clear contrast plus zero measured dispersion can still qualify. Real instrument examples and held-out evaluation are required before scientific use.

Mask mode marks clipped samples as undefined in a copy of the original image, preserves dimensions and WCS, and writes a companion uint8 FITS mask. Crop mode compares the maximum-area clean rectangle with the largest artifact-free circle using pixel-mask geometry alone, then chooses the shape retaining more pixel centers (rectangle wins ties). The circle is stored in its rectangular FITS bounding box, with pixels outside the circle marked undefined. Crop mode updates dimensions and primary/alternate `CRPIX1/2` values while retaining other headers. Reports give geometry, origin, artifact pixels removed, unmasked pixels sacrificed, and net utility. If the configured loss penalty outweighs artifact-removal evidence, the original is kept. Both modes regenerate and verify FITS checksums and preserve retained raw samples.

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

Use `--recursive` to search subdirectories, `--heuristics PATH` to select a separate science-rule file, `--mode crop` to choose the larger pixel-derived circle/rectangle (`mask` is the default), `--threads N` to cap workers, `--memory-budget-mib N` to set the total image workspace budget, and `--overwrite` to replace existing products. `--help` lists the current options. Input and output trees may not overlap. Originals are never edited.

## Products

Each run receives a UTC run ID. Processed images and masks retain the input-relative path under `output/images/` and `output/masks/`; an unchanged eligible image is a symlink to its canonical source in `output/images/`. Generated FITS products include `CREATOR` (program and build version), `DATE` (UTC product time), and a nanosecond UTC `HISTORY` entry. Symlinked kept images retain the original source bytes and header unchanged, while reports and logs identify the current run version. `output/reports/<run-id>/summary.json` contains totals, program/version and per-file details; `per-image.csv` is convenient for spreadsheets. Activity events live in `output/logger/` as JSON Lines with an ISO-8601 UTC timestamp on each entry. A candidate finishes as clipped, kept, ignored or error. Exit status is 0 for no per-file errors, 1 when one or more files fail, and 2 for fatal startup/configuration errors.

The application build version starts at the three-part value in `input/program-version.txt`. Whenever Cargo recompiles the package, the patch component advances; after 99 it rolls over and advances minor, and after `0.99.99` it advances major. The current local counter is stored in ignored `output/program-version.txt`. A Cargo invocation that has no compilation work does not advance the counter.

## Configuration

Edit `input/config.toml` for worker/discovery/memory and output mode settings and `input/heuristics.toml` for clipping decisions; see [configuration reference](configuration.md). Invalid values stop the run before input processing. Effective settings and hashes are recorded in the run-start log. Detector rules are reviewable thresholds, not validated instrument-independent settings or calibrated probabilities. Ambiguous images are kept unchanged.

## Safe interpretation

Inspect the mask FITS and report before scientific analysis. A mask describes rejected pixels; downstream photometry should exclude these values and account for masked aperture area. Smooth sky structure, nebulosity, extended sources, PSF wings and moving targets can resemble correlated patterns. Retain images when detector evidence is ambiguous; compare representative data with instrument-specific knowledge. This release is an engineering prototype, not a validated observatory pipeline.
