# Validation record

## Current implementation checks

Commands run from the repository root on 2026-10-27:

- `cargo check` — passed with the Rust-native backend.
- `cargo test --all-targets` — passed: 13 unit and 9 end-to-end cases.
- `cargo clippy --all-targets -- -D warnings` — passed.
- `cargo fmt --all` — applied; formatting verification is included in the final checks.
- `cargo tree` — dependency tree contains `fitskit` and no `fitsio`/CFITSIO wrapper.

Synthetic cases cover corner, one-sided, rectangular and pixelated circular masks; oversized candidates; a smooth gradient; interior anomalies; float64 and integer image writes; int64 small contrast beyond f64 integer precision; scaled int16; unsigned byte; preserved retained samples and header values; one-HDU output; mask, report and logger output; and multi-HDU rejection. These fixtures use the pure-Rust fitskit crate. They do not establish false-positive rates or recall on astronomical observations, and an independent FITS implementation should be used for release cross-validation. Header model round-tripping does not guarantee byte-identical card formatting.

## Scientific validation required

Before a scientific-release claim, assemble licensed and documented real frames from each target instrument and a held-out set. Annotate known non-science regions independently. Measure pixel-level precision/recall, boundary distance, abstention rate, false rejection in source/comparison apertures, flux bias, background changes and moving-target recovery. Include empty fields, gradients, crowded frames, nebulosity, vignetting, PSF wings, trails, all supported BITPIX types, integer scaling and existing BLANK values. Freeze thresholds using development data before reporting held-out results.

## Performance evidence

No published speedup is claimed yet. Record CPU, memory, disk, file count and dimensions; compare one worker with several workers for median throughput, p95 latency and peak memory. Verify numerical mask equality and retained-pixel identity for each worker count. Independent files can use separate fitskit parser/writer instances; tune concurrency only from these measurements.
