# Output safety, layout and program rename plan

Status: implemented and verified locally.

## Assumptions

- Sanitizing an output destination means validate and isolate writes. It does not authorize deleting existing products, reports or activity logs.
- Existing outputs remain readable and unchanged. New runs write beneath one unique `runs/<run-id>/` directory, preventing stale products from being mixed with new ones.
- The output root must not be a symlink and must not overlap the input tree. Every generated path derives from a validated relative input path and a newly created run directory.
- Rename the checked-out repository folder `rust_fits_auto_clipping` to `rust_fits_auto_cropping`; rename the Cargo package and CLI to `rust_fits_auto_cropping` too.

## Phases

1. Completed: inventoried implementation, tests, build state, documentation, outputs, branch and remote state.
2. Completed: grouped products, report and activity log under one isolated run directory; migrated build counter into `output/.state/`.
3. Completed: validated roots and relative paths, rejected symlinked managed directories and overlap, and isolated runs without deleting prior outputs.
4. Completed: renamed Cargo package, CLI, FITS provenance, documentation and checkout directory to `rust_fits_auto_cropping`.
5. Completed: updated tests, cross-validation scripts, README, AGENTS and project documentation; historical output links are labeled.
6. Completed locally: 47 tests, formatting, Clippy, release/help, supplied-dataset FITS/PNG interoperability and output-safety checks passed. The verified change is registered in local Git.

## Acceptance criteria

- Each run's science products, masks, previews, summary files and activity events live below one `output/runs/<run-id>/` directory.
- A prior output tree remains intact and a subsequent run cannot accidentally append to or overwrite it.
- Output-root symlinks, input/output overlap, unsafe relative components, symlinked output subpaths and run-id collisions fail before any science products are written.
- Product symlinks still target original input files; generated FITS headers and summary `program` values use the new name.
- The Rust executable, version output, docs, tests and remote-readme examples use `rust_fits_auto_cropping`; historical output records identify their previous name.
- All existing scientific decisions and pixel values remain unchanged; output-layout migration tests and prior FITS/PNG checks pass.

## Progress evidence

- Completed the repository/output inventory and documented the non-destructive sanitation policy.
- Added a shared `OutputLayout` that validates/canonicalizes the output root, rejects input/output overlap and managed symlink components, creates a unique run directory, and validates input-relative paths before product writes.
- Consolidated run products, reports and logs beneath `output/runs/<run-id>/`; build state is now at `output/.state/program-version.txt` and migrates the existing root-level counter.
- Preserved legacy output trees in place and verified that unrelated files are left intact; no automatic purge or legacy-output relocation occurs.
- Renamed the Cargo package, CLI, FITS provenance and workspace directory to `rust_fits_auto_cropping`.
- Final checks: `cargo fmt --all --check`, 47 tests, Clippy, release build/version/help, 20 FITS and 16 mask checks with Astropy/`fitsverify`, and 16 preview checks against Astropy/Pillow.
