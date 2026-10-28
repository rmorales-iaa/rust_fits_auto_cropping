# Direct output directory plan

## Goal

Allow the yearly workflow to write `images/`, optional `previews/`, summary, CSV, and activity log directly under the requested `science_calibrated/` output path, without adding `runs/<run-id>/`.

## Acceptance

1. Add an explicit CLI switch for direct output; keep run-isolated output as the default for existing callers.
2. Update the yearly script to use direct output and overwrite matching generated products on reruns; retain its 2002-only selector.
3. Update CLI help, README, user guide, architecture and project memory.
4. Test the direct layout on year 2002 only; verify output products are directly under the requested path and no timestamped `runs/` layer is created by the new mode.

## Status

- Phase 1: complete. `--flat-output` selects the requested root directly; run-isolated layout remains the default.
- Phase 2: complete. The yearly script uses `--flat-output --overwrite` and still targets 2002–2026 by default, with a one-year selector.
- Phase 3: complete. CLI help, README, user guide and architecture document both layouts.
- Phase 4: complete. The 2002 run wrote all 3,906 image entries, reports, previews and the activity log directly beneath the requested `science_calibrated/` directory. Counts: 68 clipped, 3,838 kept, 0 ignored and 0 errors. All 68 previews passed 200-sample Astropy ZScale, geometry, orientation and outline checks.

## Verification

- `cargo test --locked --all-targets`: passed (57 unit, 29 end-to-end, 4 version tests).
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- `cargo fmt --check` and `git diff --check`: passed.
- The PNG checker uses Rust-compatible halfway rounding (`f64::round`, half away from zero); NumPy's default `rint` uses ties-to-even and produced false one-level differences.
- The prior timestamped `runs/20261028T121035.186415637Z/` directory was preserved. This task did not delete historical products.
