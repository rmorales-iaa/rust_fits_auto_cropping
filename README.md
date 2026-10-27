# Rust FITS Auto Clipping

A conservative Rust CLI prototype that detects large, anomalous regions connected to the borders of one-HDU, two-dimensional FITS images. It writes an image copy with accepted samples marked undefined, a separate mask, and a per-run report. Original files remain untouched.

The first detector uses robust tile statistics, spatial lag-one correlation and border-connected components. It supports asymmetric and partly symmetric border patterns; circle/rectangle boundaries are approximated at tile scale. Its thresholds are starting values and have **not** been scientifically validated against held-out observatory images. Inspect every mask before scientific use. See [the algorithm and its limits](input/doc/algorithm.md).

## Build and run

Requires Rust 1.88+. FITS reading and writing use the pure-Rust [`fitskit`](https://docs.rs/fitskit/) crate and do not require CFITSIO or a system C library.

```bash
cargo build --release
cargo run --release -- --input-dir input/data --output-dir output --config input/config.toml
cargo run --release -- --input-dir input/data --output-dir output --recursive --config input/config.toml --heuristics input/heuristics.toml
cargo run --release -- --input-dir input/data --output-dir output --dry-run
```

Pass `--help` for all options. The input and output directory trees must not overlap. By default only the immediate input directory is searched; use `--recursive` for nested folders. Multi-HDU files and unsupported shapes/compressed inputs are recorded as ignored. Corrupt files and inputs exceeding the configured memory limit are errors.

## Products and configuration

`output/images/` contains processed or unchanged copies; `output/masks/` contains companion uint8 masks where one means newly masked. `output/reports/<run-id>/` contains JSON and CSV summaries. `output/logger/` contains UTC ISO-timestamped JSON Lines activity logs. Integer image pixels use a safe FITS `BLANK` sentinel; float pixels use NaN. Retained raw samples, dimensions and the original header are checked after writing; checksum cards are regenerated.

Operational settings live in [input/config.toml](input/config.toml), and science decision rules live in [input/heuristics.toml](input/heuristics.toml). Read the [user guide](input/doc/user-guide.md), [configuration reference](input/doc/configuration.md), [architecture](input/doc/architecture.md), [algorithm limits](input/doc/algorithm.md), and [validation record](input/doc/validation.md). Crop mode is deferred; this release preserves the pixel grid and WCS. Gzip-compressed and multi-HDU inputs are ignored.

## Project development

Source code is organized in small modules under `src/`; tests are under `tests/`. Run `cargo test`, `cargo fmt --check`, and `cargo clippy --all-targets -- -D warnings`. Each verified coherent change is registered in local Git. Build products, logs and raw FITS inputs are ignored.
