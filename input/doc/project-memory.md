# Project memory

Updated: 2026-10-27, after the scientific planning review.

## Identity and active work

- Repository/CLI: `rust_fits_auto_cropping`; pure-Rust FITS I/O with `fitskit`; English communication and documentation.
- Active work specification: [scientific improvement plan](scientific-improvement-plan.md). Evidence and prioritized failures: [scientific audit](scientific-audit-2026-10-27.md). The [original implementation plan](implementation-plan.md) and crop/preview/layout plans are historical baseline records, not proof of every proposed guarantee.
- The latest request was a deep review and iterative plan improvement. Runtime algorithms and default thresholds were not changed. Future implementation must follow the phases and evidence gates; do not treat planning completion as a scientific release.
- Documentation and versioned skills belong in `input/doc/`, configuration in `input/`, generated data under the chosen output root. Read `AGENTS.md` and the main project skill before work.

## Persistent user and science contracts

- Classification depends on pixel statistics and geometry only. Structural/scaling/null metadata is necessary for decoding; WCS metadata is necessary for output preservation. Descriptive header values do not steer the detector. Evaluation groups may use instrument/night metadata without making it a classifier feature.
- Process single-HDU 2D images; ignore compressed/multi-HDU inputs. No resampling or source intensity modification beyond explicit exclusion. Preserve raw retained samples and original observations.
- Maximize valid science retained and true artifact area removed under a declared valid-loss and source-preservation budget. A larger mask is not inherently better. Candidate artifacts, original undefined samples, uncertain boundaries and geometric sacrifice are different quantities.
- Mask mode remains the default. Current crop mode compares an exact axis-aligned clean rectangle with a conservative integer-centered circle and applies heuristic utility. Freeform mask/auto/extra crop families in the active plan are proposed extensions, not current CLI options.
- Kept eligible images are canonical source symlinks. Generated products carry version/time provenance and checksums. Preserve source headers through kept links.
- Each invocation uses `<output>/runs/<run-id>/` for images, masks, previews, reports and ISO activity logging. Validate output paths and overlap; preserve prior runs and unrelated files. Build state is ignored under `output/.state/`.
- Keep bounded concurrency, separate runtime/science TOML, small modules and coherent local commits. Do not claim actual memory bounds or scientific accuracy from estimates or passing unit tests alone.

## Review baseline and verified evidence

- Audited source: `626f53c52e4fdd59e7e90ed86cd050c09cd8016c`. Cargo package `0.1.0`; existing release binary `0.0.29`; `cargo test --locked --all-targets` compiled test build `0.0.30` and advanced local counter state. Existing release binary remains `0.0.29`. Query the actual binary; build counter, package version and Git identity are distinct.
- All 47 existing tests passed again (25 unit, 18 integration, 4 version tests). This does not cover all adversarial failures.
- New release-binary probes confirmed: tile-boundary under/over-removal of 128 pixels; different detection after adding large BZERO to identical stored samples; full-code uint8 failure with/without existing BLANK; gzip named `.fits` misclassified as error; stale cropped DATAMIN and mask BUNIT copied from science. See audit A01–A17 for precise locations, consequences and phase mapping. No fixes were implemented in this planning task.
- Probe evidence and fixture script: `output/reviews/science-plan-20261027-4hmuy3p9/` (ignored local output); portable recipes are in the tracked audit.
- Earlier supplied-data run: 25 files from `/fast_disk/tmp/clippling_test/input/`; 16 clipped, 4 kept, 5 malformed quoted INSAPNAM errors; 20 science outputs, 16 masks and 16 previews. Independent FITS/PNG checks passed on those cases. One warm 4-worker run took 4.56 s with 925,972 KiB RSS. This is historical evidence, not a new benchmark or a universal interoperability claim.
- The repeatedly inspected supplied set is development data. It has no independent artifact labels and must not be called an untouched hold-out.

## Implementation priorities and release conditions

1. Freeze reproducible baseline and regression fixtures; fix numerical/sentinel/FITS/header/WCS/reporting/publication failures before algorithm promotion.
2. Obtain independently labeled data with group splits, uncertainty labels and a registered observing domain. Develop source-aware local reference/noise estimation, multiscale residual evidence, component decisions and pixel-boundary refinement.
3. Separate segmentation from action utility; compare retention/removal and source loss under one policy, with precise geometry domains and conservative keep/review behavior for unsupported cases.
4. Profile actual stage buffers, sorts, copies and reads; optimize exact algorithms/ownership first and validate weighted memory scheduling. Repeat release benchmarks with paired workloads and unchanged science gates.
5. Freeze thresholds on development/calibration groups, then use untouched groups and independent science/FITS/WCS checks. Proposed numeric targets in the active plan are requirements to justify and register, not achieved accuracy.
6. Stop planning after two full reviews find no new material omission; stop algorithm complexity only after bounded development experiments establish negligible practical gain with adequate uncertainty. Insufficient statistical power is not evidence of equivalence.

## Git and durable records

- Review began on clean `master`, with no other branches or configured remote. Recheck refs/status before merges or publication; never guess a destination URL. A previous push request remains unable to complete until the intended remote is known.
- Preserve historical validation, audit counterexamples and plan review decisions. Update README/skills/memory when scientific contracts or supported behavior change; synchronize matching installed skill copies without touching unrelated skills.
- Version template `input/program-version.txt` is tracked; local counter/build products, observations, run logs and derived products stay ignored. Check Git history for the commit containing this review instead of assuming its notes are the latest revision.
