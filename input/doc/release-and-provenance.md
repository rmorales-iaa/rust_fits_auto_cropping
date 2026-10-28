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

The run-start log and version-9 JSON summary store program/version, algorithm/policy IDs, Git revision/dirty state, executable SHA-256 and source/effective configuration hashes. Per-image reports store SHA-256 for exact source bytes read through the parsed file handle. Reports include a stable `reason_code` and C/A/E/U0 set intersections. Science and mask products are encoded and verified in sibling staging directories, then moved into place with rollback for ordinary publication errors. There is no `manifests/` output directory or per-input completion marker; use `summary.json`, `per-image.csv`, and the activity log to review decisions and published paths. Separate filesystem renames are not a multi-file transaction, so a process crash between final renames can leave a partial product set. A kept image is a symlink to its canonical source and is never edited to add provenance cards; its run provenance is in the report and activity log. Applications consuming output trees must retain access to the source files backing kept symlinks.

## Validation

Use `cargo test --all-targets` for rollover/parser checks and product-card tests. Run `target/release/rust_fits_auto_cropping --version` after building. For generated products, `tools/cross_validate_fits.py` verifies provenance cards, FITS checksums and crop/mask invariants; it also verifies that kept output symlinks resolve to their matching inputs.
