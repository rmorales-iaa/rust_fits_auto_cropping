# Validation record

## Automated Rust checks

Run from the repository root with the pinned pure-Rust FITS dependency:

- `cargo test --all-targets` — 29 tests (15 unit, 14 end-to-end) pass.
- `cargo clippy --all-targets -- -D warnings` — pass.
- `cargo fmt --all --check` — pass.
- `cargo build --release` — pass.
- `cargo tree` — FITS I/O uses `fitskit`; there is no `fitsio`/CFITSIO dependency.

The integration matrix covers synthetic one-corner, one-side, rectangular, pixelated circular and asymmetric masks; smooth-gradient and interior-anomaly controls; decision utility; every supported BITPIX family; scaling and retained raw samples; BLANK retention and invalid BLANK rejection; malformed/truncated inputs; gzip and tiled-compressed inputs; multi-HDU files; non-ASCII/nested paths; `--recursive`; dry run; reports/logs; checksums and serial/parallel equivalence. The fixtures validate software invariants, not observatory-level detector recall.

## Supplied real-data run

Inputs were read from `/fast_disk/tmp/clippling_test/input/`. All products, reports, logs and benchmark evidence were written under `/fast_disk/tmp/clippling_test/output/`. The directory contained 25 ordinary single-HDU 2D FITS images, approximately 274 MB. It contained no compressed or multi-HDU examples. SHA-256 manifests before and after processing each contained 25 entries and matched; the input set was not modified.

Run command:

```bash
target/release/fits-auto-clipping \
  --input-dir /fast_disk/tmp/clippling_test/input \
  --output-dir /fast_disk/tmp/clippling_test/output \
  --config input/config.toml \
  --heuristics input/heuristics.toml \
  --recursive --threads 4 --memory-budget-mib 2048
```

The recorded run classified **13 clipped, 4 kept, 0 ignored and 8 errors**. The exit status was 1 because the run correctly continued while reporting file-level errors. Five input headers have an unterminated quoted `INSAPNAM` value, and three `BITPIX=16` inputs specify out-of-range `BLANK=32768`. These malformed inputs were not repaired or rewritten. Since the supplied set has no independent artifact labels, the 13 masks are candidate proposals requiring instrument-aware review; this run does not calibrate scientific thresholds.

Main outputs:

- Summary: `/fast_disk/tmp/clippling_test/output/reports/20261027T103352.466257843Z/summary.json`
- Per-image decisions and diagnostics: `/fast_disk/tmp/clippling_test/output/reports/20261027T103352.466257843Z/per-image.csv`
- Science products: `/fast_disk/tmp/clippling_test/output/images/`
- Companion masks: `/fast_disk/tmp/clippling_test/output/masks/`
- Activity logs: `/fast_disk/tmp/clippling_test/output/logger/`
- Benchmark measurements: `/fast_disk/tmp/clippling_test/output/benchmarks/benchmark-summary.json`

## FITS interoperability

`tools/cross_validate_fits.py` opens science and mask products independently with Astropy and invokes `fitsverify` when installed. It checks readable FITS structure, HDU count, dimensions, BITPIX, checksum validity, WCS cards and consistency of the mask/product pixel data. The main run and both serial and parallel benchmark outputs passed: Astropy verified 17 science images and 13 mask products in each output tree. The synthetic test output passed the same checks. Independent checksum verification caught a non-canonical CHECKSUM card comment in an earlier writer version; the writer was corrected to checksum the exact serialized bytes and now passes both independent checkers.

## Performance and memory

Measurements used the release binary and `/usr/bin/time -v`, processing the same 25 input files with a 2048 MiB configured application budget. A 32-logical-CPU host reported 125 GiB RAM and NVMe storage. This is a single run per worker count; serial ran first, so filesystem cache state may favor the parallel timing. Measurements are observational, not a robust benchmark distribution.

| Workers | Wall time | Successful outputs/s (17 files) | Valid pixels/s (approx. 121.8M) | Peak RSS |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 12.19 s | 1.39 | 10.0 Mpx/s | 280,116 KiB (273.6 MiB) |
| 4 | 7.13 s | 2.38 | 17.1 Mpx/s | 873,780 KiB (853.3 MiB) |

Four workers gave 1.71x observed throughput and 3.20x peak RSS. The process stayed below the configured 2 GiB budget in this run; RSS includes runtime and parser overhead. Serial and parallel runs produced identical decisions and output-product hashes. The timing and memory logs, console outputs and input hash manifests are preserved in `output/benchmarks/`.

## Crop decision and heuristic caveats

Crop mode is not included. The mask mode handles irregular asymmetric boundaries while retaining dimensions and WCS. A crop would delete valid pixels beyond the mask and requires a separately validated WCS/distortion contract.

Scientific rules live in `input/heuristics.toml`. The detector reports candidate evidence, a configurable artifact-benefit diagnostic, valid-pixel-loss penalty and net utility for candidates, including those rejected by support/area gates. These are transparent heuristic units, not probabilities, p-values or measured photometric loss. The default thresholds are provisional and not empirically calibrated. A real science release requires labeled, instrument-specific development data and held-out evaluation.

## Required scientific acceptance work

Before production use, assemble licensed and documented annotated frames across instruments and test on held-out data. Measure pixel precision/recall, boundary distance, abstention rate, source/aperture flux bias, background changes, faint edge-object and moving-target recovery, trail preservation, and WCS agreement. Include crowding, galaxies, nebulosity, gradients, vignetting, saturation, unusual scaling/null conventions and all supported storage types. Freeze thresholds on development data before the hold-out evaluation; keep all candidate masks reviewable.
