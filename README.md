# Rust FITS Auto Clipping

A conservative Rust CLI prototype that detects large, anomalous regions connected to the borders of one-HDU, two-dimensional FITS images. It writes processed images with accepted samples marked undefined, a separate mask, symlinks unchanged images into the output tree, and creates a per-run report. Original files remain untouched.

The first detector uses robust tile statistics, spatial lag-one correlation and border-connected components. It supports asymmetric and partly symmetric border patterns; circle/rectangle boundaries are approximated at tile scale. Its thresholds are starting values and have **not** been scientifically validated against held-out observatory images. Inspect every mask before scientific use. See [the algorithm and its limits](input/doc/algorithm.md).

## Build and run

Requires Rust 1.88+. FITS reading and writing use the pure-Rust [`fitskit`](https://docs.rs/fitskit/) crate and do not require CFITSIO or a system C library.

```bash
cargo build --release
cargo run --release -- --input-dir input/data --output-dir output --config input/config.toml
cargo run --release -- --input-dir input/data --output-dir output --recursive --config input/config.toml --heuristics input/heuristics.toml
cargo run --release -- --input-dir input/data --output-dir output-crop --mode crop --recursive
cargo run --release -- --input-dir input/data --output-dir output --dry-run
```

Pass `--help` for all options. The input and output directory trees must not overlap. By default only the immediate input directory is searched; use `--recursive` for nested folders. Multi-HDU files and unsupported shapes/compressed inputs are recorded as ignored. Corrupt files and inputs exceeding the configured memory limit are errors.

## Products and configuration

`output/images/` contains processed images and symlinks for unchanged images; `output/masks/` contains companion uint8 exclusion masks. Generated FITS products record the program/version in `CREATOR`, the UTC product date in `DATE`, and a nanosecond UTC timestamp in `HISTORY`. Kept symlinks leave their source headers untouched; run version/time are in the report and activity log. The tracked `input/program-version.txt` template begins at `0.0.0`; each Cargo compilation increments the build version in base 100 (patch, then minor, then major), with local state under ignored `output/`. Mask mode preserves dimensions. Crop mode (`--mode crop`) compares a maximum-area clean rectangle with a largest artifact-free circle and selects the geometry retaining more pixel centers. Circular crops use a rectangular FITS bounding box, with pixels outside the circle undefined. Reports give removed artifact pixels, unmasked pixels sacrificed, crop geometry and net utility. `output/reports/<run-id>/` contains JSON and CSV summaries. `output/logger/` contains UTC ISO-timestamped JSON Lines activity logs. Integer image pixels use a safe FITS `BLANK` sentinel; float pixels use NaN. Retained raw samples and checksums are verified after writing.

Operational settings live in [input/config.toml](input/config.toml), and science decision rules live in [input/heuristics.toml](input/heuristics.toml). Read the [user guide](input/doc/user-guide.md), [configuration reference](input/doc/configuration.md), [architecture](input/doc/architecture.md), [algorithm limits](input/doc/algorithm.md), and [validation record](input/doc/validation.md). Crop mode is opt-in and can discard unmasked science pixels; check each report before downstream use. Gzip-compressed and multi-HDU inputs are ignored.

## Project development

Source code is organized in small modules under `src/`; tests are under `tests/`. Run `cargo test`, `cargo fmt --check`, and `cargo clippy --all-targets -- -D warnings`. Each verified coherent change is registered in local Git. Build products, logs and raw FITS inputs are ignored.
