# Build version and FITS provenance

## Application build counter

The tracked template in `input/program-version.txt` is `0.0.0`. On a Cargo package compilation, `build.rs` reads the current counter from ignored `output/.state/program-version.txt` (or starts from the template), increments it, and embeds the result in the executable. If the legacy `output/program-version.txt` exists and the new state is absent, the build migrates the file before incrementing it. A patch value advances through `99`; the next build resets patch and advances minor. `0.99.99` advances to `1.0.0`. Components must be numeric, and minor/patch values are restricted to 0–99. A Cargo invocation with no compilation work does not increment the counter.

The ignored state is machine-local: a fresh checkout starts again from the tracked template. This counter identifies local builds and is not a substitute for a published release tag. Cargo package metadata in `Cargo.toml` remains separate from the application build counter.

## Product provenance

Every newly serialized science image, crop, or mask receives:

- `CREATOR`: package name and compiled application version.
- `DATE`: UTC product creation time in FITS ISO date/time form.
- `HISTORY`: UTC timestamp with fractional seconds and the program/version.

The run-start log and JSON summary also store program/version, and the summary has its own UTC creation time. A kept image is a symlink to its canonical source and is never edited to add these cards; its run provenance is in the log and summary. Applications consuming output trees must retain access to the source files backing kept symlinks.

## Validation

Use `cargo test --all-targets` for rollover/parser checks and product-card tests. Run `target/release/rust_fits_auto_cropping --version` after building. For generated products, `tools/cross_validate_fits.py` verifies provenance cards, FITS checksums and crop/mask invariants; it also verifies that kept output symlinks resolve to their matching inputs.
