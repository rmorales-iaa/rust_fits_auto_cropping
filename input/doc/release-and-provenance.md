# Build version and FITS provenance

## Application build counter

The tracked template in `input/program-version.txt` is `0.0.0`. On a Cargo package compilation, `build.rs` reads the current counter from ignored `output/.state/program-version.txt` (or starts from the template), increments it, and embeds the result in the executable. If the legacy `output/program-version.txt` exists and the new state is absent, the build migrates the file before incrementing it. A patch value advances through `99`; the next build resets patch and advances minor. `0.99.99` advances to `1.0.0`. Components must be numeric, and minor/patch values are restricted to 0–99. A Cargo invocation with no compilation work does not increment the counter.

The ignored state is machine-local: a fresh checkout starts again from the tracked template. This counter identifies local builds and is not a substitute for a published release tag. Cargo package metadata in `Cargo.toml` remains separate from the application build counter.

At the 2026-10-27 deep review, the package metadata was `0.1.0`, release executable `0.0.29`, and test compilation advanced local state to `0.0.30`. Inspect the actual executable with `--version`; the state file need not match every existing binary. Current summaries record Git revision/dirty state, executable/config hashes and algorithm/schema identities. Eligible source digests are recorded per input as described below. These identities support reproducibility but do not certify scientific accuracy.

## Product provenance

Every newly serialized science image, crop, or mask receives:

- `CREATOR`: package name and compiled application version.
- `DATE`: UTC product creation time in FITS ISO date/time form.
- `HISTORY`: UTC timestamp with fractional seconds and the program/version.

The run-start log and version-6 JSON summary also store program/version, algorithm/policy IDs, Git revision/dirty state, executable SHA-256 and source/effective configuration hashes. Eligible input rows and version-3 per-file completion manifests store SHA-256 for the exact source bytes read through the parsed file handle and for every published product. Reports include a stable `reason_code` and C/A/E/U0 set intersections. Each completed input has an atomically published manifest after all expected products are verified; missing manifests indicate incomplete or failed sets. The marker is a completion signal, not a multi-file transaction. A kept image is a symlink to its canonical source and is never edited to add these cards; its run provenance is in the log, summary and manifest. Applications consuming output trees must retain access to the source files backing kept symlinks.

## Validation

Use `cargo test --all-targets` for rollover/parser checks and product-card tests. Run `target/release/rust_fits_auto_cropping --version` after building. For generated products, `tools/cross_validate_fits.py` verifies provenance cards, FITS checksums and crop/mask invariants; it also verifies that kept output symlinks resolve to their matching inputs.
