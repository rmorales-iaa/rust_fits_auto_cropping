# Direct output directory plan

## Goal

Allow the yearly workflow to write FITS images directly under the requested `science_calibrated/` output root, retain optional PNGs under `previews/`, and write reports/logs at the root, without adding `images/` or `runs/<run-id>/` layers. Sanitize the dedicated output directory before each run.

## Acceptance

1. Add an explicit CLI switch for direct output; keep run-isolated output as the default for existing callers.
2. Update the yearly script to sanitize its dedicated output root and use direct output; retain its 2002-only selector.
3. Update CLI help, README, user guide, architecture and project memory.
4. Test the direct layout on year 2002 only; verify FITS products are directly under the requested path, previews stay under `previews/`, sanitation clears stale contents, and no new `images/` or timestamped `runs/` layer is created.

## Status

- Phase 1: complete. `--flat-output` selects the requested root directly; run-isolated layout remains the default.
- Phase 2: complete. The yearly script uses `--flat-output --sanitize-output` and still targets 2002–2026 by default, with a one-year selector.
- Phase 3: complete. CLI help, README, user guide and architecture document both layouts.
- Phase 4: complete. Run `20261028T123004.136144622Z` processed 2002 only and published 3,906/3,906 FITS entries at the output root (68 cropped, 3,838 kept, 0 ignored/errors). All 68 PNGs are under `previews/` and passed independent 200-sample Astropy ZScale/geometry/orientation/outline checks. No `images/`, `runs/`, `masks/` or `reason_masks/` directory remains.

## Verification

- `cargo test --locked --all-targets`: passed (57 unit, 29 end-to-end, 4 version tests).
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- `cargo fmt --check` and `git diff --check`: passed.
- The PNG checker uses Rust-compatible halfway rounding (`f64::round`, half away from zero); NumPy's default `rint` uses ties-to-even and produced false one-level differences.
- `--sanitize-output` removes every existing child of the explicitly selected output root without following symlinks; input/output overlap and output-root symlinks are rejected before sanitation.
- The yearly script uses sanitation only for its dedicated science destination; its calibration symlinks are sibling directories and remain untouched.
- `cargo test --locked --all-targets`: passed (57 unit, 29 end-to-end, 4 version tests).
- `cargo clippy --locked --all-targets -- -D warnings`, `cargo fmt --check` and `git diff --check`: passed.
