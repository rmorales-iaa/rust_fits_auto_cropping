# rust_fits_auto_cropping

A conservative Rust CLI prototype that detects large, anomalous regions connected to the borders of one-HDU, two-dimensional FITS images. It writes processed images with accepted samples marked undefined, a separate mask, symlinks unchanged images into the output tree, and creates a per-run report. Original files remain untouched. Detection is driven by pixel statistics; non-structural header content is not a detector feature.

The first detector uses robust tile statistics, spatial lag-one correlation and border-connected components. It supports asymmetric and partly symmetric border patterns; circle/rectangle boundaries are approximated at tile scale. Its thresholds are starting values and have **not** been scientifically validated against held-out observatory images. Inspect every mask before scientific use. See [the algorithm and its limits](input/doc/algorithm.md).

The [scientific audit](input/doc/scientific-audit-2026-10-27.md) found boundary over/under-removal, numerical scaling and integer-sentinel failures, and metadata/reporting gaps despite 47 passing tests. The first P1 correctness slice is now implemented: detection uses centered stored-pixel values, BLANK selection is deferred until a product needs undefined pixels, gzip signatures are ignored even under a `.fits` suffix, derived-image extrema are removed, mask units are omitted, and dry-run proposals are reported separately from applied clipping. Regression coverage now includes scale/header invariance and full-range byte images retained without modification. These fixes do **not** resolve tile-scale boundary error or establish scientific accuracy; the source-aware model, per-component evidence, pixel refinement and calibrated action policy remain open in the [active improvement plan](input/doc/scientific-improvement-plan.md). Cargo package version `0.1.0` and the machine-local `--version` build counter identify different things; neither is a scientific certification.

## Build and run

Requires Rust 1.88+. FITS reading and writing use the pure-Rust [`fitskit`](https://docs.rs/fitskit/) crate and do not require CFITSIO or a system C library.

```bash
cargo build --release
cargo run --release -- --input-dir input/data --output-dir output --config input/config.toml
cargo run --release -- --input-dir input/data --output-dir output --recursive --config input/config.toml --heuristics input/heuristics.toml
cargo run --release -- --input-dir input/data --output-dir output-crop --mode crop --recursive
cargo run --release -- --input-dir input/data --output-dir output-crop --mode crop --png --recursive
cargo run --release -- --input-dir input/data --output-dir output --dry-run
```

Pass `--help` for options, examples, crop semantics and exit codes. `--version` reports the build counter. The output root is canonicalized and checked before use; a symlinked root, overlapping input/output trees, unsafe relative destinations or symlinked managed subdirectories are rejected. Previous run data are preserved. By default only the immediate input directory is searched; use `--recursive` for nested folders. Multi-HDU files and unsupported shapes/compressed inputs are recorded as ignored. Corrupt files and inputs exceeding the configured memory limit are errors.

## Products and configuration

Each run is stored beneath `output/runs/<run-id>/`, with `images/`, `masks/`, optional `previews/`, `summary.json`, `per-image.csv` and an ISO-dated `activity_*.jsonl` together. Existing run directories are preserved. Output-root symlinks, input/output overlap, unsafe relative paths and symlinked product subdirectories are rejected before product writes. A kept image symlink targets the canonical input file and preserves its bytes and header. Generated FITS products record program/version in `CREATOR`, UTC product date in `DATE`, and generation time in bounded `HISTORY` cards. The tracked `input/program-version.txt` template begins at `0.0.0`; each Cargo package recompilation increments the build version in base 100. A no-op Cargo invocation does not advance it. Mask mode preserves dimensions. Crop mode (`--mode crop`) compares the current clean axis-aligned rectangle and conservative integer-centered circle families. Circle pixels outside the selected region are undefined in the rectangular storage bounds. The configured utility remains provisional and is not a calibrated science loss model. Integer image pixels use FITS `BLANK` when a representable value is available; float pixels use NaN. Retained raw samples and checksums are verified after writing.

Reports distinguish originally undefined samples, `proposed_mask_pixels`, and `masked_pixels` actually applied. Dry runs set the top-level `dry_run` flag, count accepted decisions as `proposed`, and report zero applied masked pixels. `clipped` counts completed product actions. Crop utility counts now ignore pre-existing non-finite samples, but complete set intersections and uncertain/valid/invalid science labels are not yet reported.

With `--mode crop --png`, `output/runs/<run-id>/previews/<input-relative-path>.png` is written for each accepted crop. Each grayscale preview places the complete input at left and the crop at right using one source-derived ZScale interval, native pixel scale, labeled panels and FITS lower-origin orientation. Invalid samples and pixels outside a circular crop are black. Previews are diagnostic only; the FITS output remains the science product, and each preview path appears in the report.

Operational settings live in [input/config.toml](input/config.toml), and science decision rules live in [input/heuristics.toml](input/heuristics.toml). Read the [user guide](input/doc/user-guide.md), [configuration reference](input/doc/configuration.md), [architecture](input/doc/architecture.md), [algorithm limits](input/doc/algorithm.md), [build and provenance policy](input/doc/release-and-provenance.md), [validation record](input/doc/validation.md), and [project memory](input/doc/project-memory.md). Crop mode is opt-in and can discard unmasked science pixels; check each report before downstream use. Gzip-compressed and multi-HDU inputs are ignored.

## Project development

Source code is organized in small modules under `src/`; tests are under `tests/`. Run `cargo test --all-targets`, `cargo fmt --all --check`, and `cargo clippy --all-targets -- -D warnings`. The independent FITS checker is `tools/cross_validate_fits.py`; it requires Astropy and runs `fitsverify` when available. `tools/cross_validate_previews.py` requires Astropy and Pillow to compare PNG scaling and geometry against FITS samples. Each verified coherent change is registered in local Git. Build products, logs and raw FITS inputs are ignored. A local branch commit does not publish without a configured remote and explicit push.
