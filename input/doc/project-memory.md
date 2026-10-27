# Project memory

Updated: 2026-10-27

## Identity and workflow

- Repository and Rust CLI: `rust_fits_auto_cropping`.
- FITS I/O uses the pure-Rust `fitskit` crate. Do not introduce CFITSIO unless the user explicitly changes this constraint.
- Respond and write project documentation in English.
- Store runtime configuration in `input/`, documentation and versioned project skills in `input/doc/`, and generated products in the selected output root.
- Before complex work, update the relevant plan in `input/doc/`; after implementation, reconcile every acceptance item against code and evidence.

## Scientific and operational invariants

- The detector uses pixel-value statistics and mask geometry. Descriptive FITS headers must not influence candidate classification. Structural, scaling and null cards may be read to decode samples.
- Only eligible single-HDU, two-dimensional images are processed. Compressed images, multi-HDU files and unsupported shapes are ignored and reported.
- Mask mode preserves dimensions. Opt-in crop mode compares the largest clean rectangle and artifact-free circle, then applies the configurable utility rule for artifact removal versus good-pixel loss. Ambiguous or rejected cases are kept.
- Preserve source data. Unchanged eligible outputs are symlinks to canonical inputs. Generated products include program/version/time provenance and valid checksums.
- Use bounded parallel workers and explicit memory budgets; verify serial/parallel decisions and products before claiming equivalence.
- Every run is isolated under `<output>/runs/<run-id>/` and includes science images, masks, optional previews, reports and the ISO-dated activity log. Reject output symlinks, unsafe destinations and input/output overlap. Preserve prior runs and unrelated files; do not perform implicit cleanup.

## Current implementation and validation snapshot

- Root instructions: `AGENTS.md`; primary design: `input/doc/implementation-plan.md`.
- CLI supports `--recursive`, `--mode mask|crop`, `--png`, worker and memory controls, dry run and isolated run reporting. Review `--help` after changes to CLI behavior.
- The synthetic/end-to-end test matrix covers border shapes, null/scaled samples, supported BITPIX values, compressed and multi-HDU exclusions, recursion, safety checks, reporting, checksums and parallel determinism.
- The supplied `/fast_disk/tmp/clippling_test/input/` dataset contains 25 ordinary single-HDU FITS files. The latest documented crop/PNG run generated 20 science products, 16 masks, 16 previews, and reported 16 clipped, 4 kept and 5 malformed-header errors. Astropy and `fitsverify` accepted generated FITS products; the independent preview checker passed. A warm 4-worker measurement took 4.56 s and peaked at 925,972 KiB RSS.
- These checks demonstrate software behavior and interoperability, not calibrated science quality. The five malformed quoted `INSAPNAM` headers are input errors. Detector thresholds and crop-loss utility remain provisional pending labeled, instrument-specific development and held-out observatory data.

## Git and release state

- At this update the local branch is `master`, with no other local or remote-tracking branches and no configured Git remote. Do not guess a remote URL. Commit verified work locally; push only after the repository remote is configured.
- Current base revision when this note was created: `c1a8fef` (`refactor: isolate run outputs and rename cropper`). Check Git status and refs again before each merge/push operation.
- `input/program-version.txt` is the tracked template. Local build counter state is ignored at `output/.state/program-version.txt`; do not commit run products, local observations, logs, previews or build outputs.

## Recommended next work

1. Obtain labeled artifact masks and instrument-aware review from a separate held-out sample; calibrate thresholds and the utility rule without leaking hold-out labels.
2. Measure false removal of valid sources, artifact recall, boundary error, aperture-flux bias, faint-edge and moving-target recovery, and WCS behavior across instruments.
3. Repeat performance measurements across image sizes and worker counts; report distributions, hardware/storage context and memory-budget overhead.
4. Configure the intended Git remote before attempting the previously requested push; merge only existing relevant branches after inspecting their commits.
