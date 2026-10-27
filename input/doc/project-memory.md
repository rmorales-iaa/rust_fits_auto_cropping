# Project memory

Updated: 2026-10-27, after implementing the second correctness/spatial-evidence slice.

## Identity and active work

- Repository/CLI: `rust_fits_auto_cropping`; pure-Rust FITS I/O with `fitskit`; English communication and documentation.
- Active work specification: [scientific improvement plan](scientific-improvement-plan.md). Evidence and prioritized failures: [scientific audit](scientific-audit-2026-10-27.md). The [original implementation plan](implementation-plan.md) and crop/preview/layout plans are historical baseline records, not proof of every proposed guarantee.
- The active request is to implement the [scientific improvement plan](scientific-improvement-plan.md). Current work partially addresses P1 with no-BLANK keep fallback, integer precision abstention, crop refusal for recognized unsupported WCS distortion, and report/build/config provenance identities. P3 has independent provisional component gates and allocation-free scale-conditioned correlation. Exhaustive small-grid rectangle/circle oracles strengthen geometry checks. P2 and P4–P5 remain open; the detector still expands tiles and has no source-aware model or calibrated action objective. P6 has only an exact algorithm allocation change and no repeated paired performance/RSS measurement. Do not treat passing regressions or prior FITS interoperability checks as science validation.
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

- Audited baseline: `626f53c52e4fdd59e7e90ed86cd050c09cd8016c`, package `0.1.0`, release `0.0.29`; planning tests built `0.0.30`. The implementation smoke build was `0.0.34`; later test/lint builds advanced the local counter. Query the executable because package version, build counter and Git identity are distinct.
- Latest validation in this work: 60 tests passed (34 unit, 22 end-to-end, 4 version tests); formatting and Clippy passed. Regression coverage adds no-unused-BLANK keep fallback, lookup-distortion crop refusal, integer precision abstention, large-value stable correlation, independently gated components and exhaustive small-grid circle/rectangle oracles.
- One release run on 25 repeatedly inspected development frames: 16 clipped, 4 kept, 5 malformed-header errors; 3.35 s and 952,624 KiB RSS, four workers. Astropy and fitsverify accepted 20 science images and 16 masks. This is one interoperability/performance observation, not a benchmark distribution or a science-accuracy result. No independent labels exist.
- P1 remains partial: no-sentinel fallback and recognized unsupported-WCS crop refusal are now implemented, and report/build/config identities were added. Stable source identity, crash-consistent publication, broad WCS validation, all reason-code matrices and detailed set intersections remain. The detector still expands whole tiles and is not science-grade.
- Probe evidence and fixture script: `output/reviews/science-plan-20261027-4hmuy3p9/` (ignored local output); portable recipes are in the tracked audit.
- Earlier supplied-data run: 25 files from `/fast_disk/tmp/clippling_test/input/`; 16 clipped, 4 kept, 5 malformed quoted INSAPNAM errors; 20 science outputs, 16 masks and 16 previews. Independent FITS/PNG checks passed on those cases. One warm 4-worker run took 4.56 s with 925,972 KiB RSS. This is historical evidence, not a new benchmark or a universal interoperability claim.
- The repeatedly inspected supplied set is development data. It has no independent artifact labels and must not be called an untouched hold-out.

## Second engineering slice

- Independently gated components prevent disconnected detections from sharing count, area, evidence or provisional utility support. Candidate/accepted component counts are in per-image reports. This is a P3 engineering partial, not multiscale evidence or calibration.
- Correlation no longer materializes pair vectors; max-magnitude scaling and Welford moments prevent overflow on large finite samples. It still uses raw adjacent pairs and can interpret trends as correlation; directional diagnostics and statistical calibration are unresolved.
- Integer actions with no unused representable BLANK code now fall back to an unchanged source symlink, including crop publication rollback. Crop mode conservatively keeps recognized `-TAB`, CPDIS/CPERR, D2IM and lookup polynomial conventions.
- Summary schema version 2 records detector/policy IDs, Git revision/dirty state, executable SHA-256 and source/effective config/heuristics SHA-256. This does not guarantee the source remained unchanged between discovery and read, or crash consistency.
- `cargo test --locked --all-targets` passed 60 tests; rustfmt and Clippy passed. Release `0.0.57` processed 16 clipped, 4 kept and 5 malformed-header errors from the same 25-image development corpus in 4.37 s / 976,992 KiB RSS, with four workers and PNG output. Astropy/fitsverify accepted all 20 science images and 16 masks. Three repeated no-PNG crop runs measured 13.16 s median / 307,964 KiB median RSS serial and 3.80 s / 923,276 KiB with four workers; all 36 science/mask pixel arrays were equal across all six. This is operational evidence on repeatedly inspected data, not a before/after algorithm gain or science test. Rebuild after commit for clean Git provenance. No independent science corpus or held-out labels are available.

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
