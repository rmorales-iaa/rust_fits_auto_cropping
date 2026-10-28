# Implementation plan: rust_fits_auto_cropping

Status: historical baseline implementation plan. Engineering evaluation was recorded on 2026-10-27, including circle/rectangle cropping. A subsequent [deep audit](scientific-audit-2026-10-27.md) found correctness and scientific-validation gaps despite 47 passing tests. The [scientific improvement plan](scientific-improvement-plan.md) is now authoritative for future work. The proposals below, including source protection, multiscale inference, refined boundaries and interruption handling, are not all implemented; historical completion labels apply only to the recorded examples.

## Objective and scope

Build a Rust CLI that examines an input directory, identifies large border patterns in eligible FITS images using pixel statistics only, writes processed products into a chosen output directory, and summarizes every candidate. Preserve input files, valid science pixels and observational metadata. Process only files containing exactly one HDU, which must contain a non-empty two-dimensional primary image. Ignore multi-HDU files, tables, empty primary arrays and unsupported dimensions with explicit reasons. Malformed/unreadable FITS files are errors rather than ordinary ignores.

Rust is the implementation language. Use a pure-Rust FITS crate where it covers the file-format contract. The implementation selected `fitskit` after checking image types, stored integer access, scaling metadata, ordered header cards, multi-HDU parsing and checksum writing. Pin chosen versions and the Rust toolchain requirement. See [primary references](references.md).

## Processing semantics and assumptions

“Clipping” has two distinct meanings. **Masking** marks invalid pixels without changing dimensions and supports arbitrary patterns. An opt-in **crop** mode compares the largest clean axis-aligned rectangle and largest artifact-free circle, choosing whichever retains more pixel centers. A circle is stored in its rectangular bounding box with undefined samples outside the circle. Cropping can discard useful pixels and must report that loss; a configured net-utility gate retains the source if the loss is too high.

Default decisions are conservative: a high score alone is insufficient unless configured gates are met. Ambiguous cases are retained. The detector may use pixel statistics and geometry but must not use descriptive FITS header values as classification evidence. Structural shape/storage, scaling and null-value metadata remain necessary to read and interpret the pixel samples. No inpainting, resampling, flat-field correction or intensity normalization is in scope. Smooth vignetting with useful signal is not automatically an invalid border. Gzip-compressed and multi-HDU FITS files are ignored under the current contract.

Masking uses IEEE NaN for floating data. For integer data, preserve original storage and scaling and select a raw integer BLANK sentinel absent from all retained valid samples, respecting existing undefined samples. If no collision-free representation is available, report an error without writing a processed product; do not silently convert 64-bit integers to inexact floats. A future explicitly requested conversion may be added with a precision contract. Zero is never a default invalid marker.

A separate one-HDU binary mask FITS records rejected pixels and a sidecar records reasons; do not append a mask HDU to the science product. Distinguish pre-existing invalid pixels from newly rejected pixels in reporting. Kept eligible images are symlinked to their canonical input file by default; never edit the source through that link. Generated science and mask products receive `CREATOR`, UTC `DATE`, and timestamped `HISTORY` provenance cards. Ignored and failed files produce report entries but no science image. A dry run writes reports and logs without science outputs or symlinks.

## Proposed command and configuration

```text
rust_fits_auto_cropping --input-dir input/data --output-dir output \
  --config input/config.toml --mode mask --threads 4
```

Required directory arguments: `--input-dir` and `--output-dir`. Planned optional arguments: `--config`, `--mode mask|crop`, `--threads`, `--memory-budget-mib`, `--recursive`, `--dry-run`, `--overwrite`, `--help`, `--version`. Default configuration lives in `input/config.toml`; explicit CLI values override it. Fail on unknown or invalid keys. Record the effective configuration and hash in run provenance under the output root; this snapshot is an output record, not a second editable configuration.

Configuration groups: discovery, robust background estimation, source protection, multiscale tile sizes, region-size/contrast/coherence gates, boundary refinement, maximum mask fraction, crop policy, resources and diagnostics. Select numeric detection defaults from validation rather than inventing universal thresholds. Give each key units, allowed range, default, rationale and an example in the configuration reference.

Canonicalize paths and reject identical or overlapping input/output directory trees, including symlink aliases. Do not follow directory symlinks by default. Preserve relative input paths in output, detect collisions, and never overwrite without the explicit flag. Stage files on the destination filesystem and atomically rename after verification. A single processing worker owns each file handle and output path.

## Algorithm design to evaluate

1. **Read and validate.** Count HDUs before loading arrays; inspect image shape, BITPIX, scaling, null representation and headers. Check dimensions, buffer sizes and configured memory limits. Preserve raw data for writing; analysis may use a floating workspace but must not round retained integer samples.
2. **Build a valid-sample and source-protection map.** Exclude pre-existing undefined samples, saturation and compact sources from background statistics. Use robust local background/scale estimates (median and MAD or a validated alternative); handle zero scale, sparse tiles, all-invalid frames and source-dominated fields explicitly. Source protection affects estimation; it is not permission to erase a bright object inside a proposed border.
3. **Generate multiscale evidence.** Use tiles and row/column profiles to estimate level shifts, dispersion, gradients, boundary contrast and spatial coherence. Compare plausible border candidates with an estimated usable interior without assuming the center is valid. Evaluate residual lag correlation only where variance and sample support are sufficient. Constant regions need a separate low-variance/level-shift criterion because normalized correlation is undefined there.
4. **Segment border-connected candidates.** Seed from each edge and corner independently; grow connected components only through supported pixels/tiles. Permit multiple disconnected border components, unequal sides, curved boundaries and combinations. A coarse tile classification must be refined at pixel resolution so small valid features are not erased wholesale.
5. **Evaluate geometry and evidence jointly.** Test side strips, rectangles, circular/elliptical illumination boundaries and unconstrained connected masks as candidate descriptions. Geometry is an optional prior, never forced symmetry. Require minimum area, border connection, reproducible contrast or noise change, and coherent evidence over scales. A circular illuminated field may imply four rejected corners; an isolated internal disk is outside the baseline border scope.
6. **Control false positives.** Large correlated regions can be sky gradients, extended galaxies, nebulosity, scattered light or PSF halos. Compare against these confounders and a no-artifact hypothesis. Use spatially blocked resampling/held-out tiles if estimating uncertainty, accounting for spatial dependence and multiple candidate regions; do not interpret pixel count as independent sample count. Scores are diagnostic scores, not probabilities unless calibrated. Keep uncertain images unchanged.
7. **Refine and validate the mask.** Apply only justified, bounded morphology; preserve source-protected boundaries, minimum retained area and maximum rejected fraction. Report excessive masks, poor interior support and conflicting evidence for review. Avoid destructive all-image masks. Record component areas, shapes, boundary support and reasons.
8. **Apply the selected mode.** Mask only accepted invalid pixels, or compare the maximum-area all-valid rectangle with the largest artifact-free circle for crop mode. Select the geometry retaining more pixel centers, with deterministic tie-breaking. Report retained area and additional science loss; if utility is below its configured threshold, retain the source.

This is an algorithm research proposal, not a claim of scientific readiness. Synthetic evidence can validate invariants, but real annotated instrument examples are required before claiming general applicability. A useful later extension is an instrument-specific profile or manually supplied mask; it is not required for the first automatic detector.

## Header and pixel preservation contract

- Preserve observation keywords, user-defined cards, comments, HISTORY and long-string/HIERARCH conventions supported by the backend. Test duplicate COMMENT/HISTORY cards and nontrivial header ordering. Do not reconstruct headers from a short allowlist.
- In mask mode, preserve dimensions and all WCS metadata. Preserve retained raw samples exactly. Update/add BLANK as needed; update or remove stale derived extrema and recompute CHECKSUM/DATASUM after all changes. Add processing HISTORY/provenance without altering original observation time.
- In crop mode, update NAXIS1/NAXIS2 and supported reference-pixel coordinates: `CRPIX1_new = CRPIX1_old - x0`, `CRPIX2_new = CRPIX2_old - y0`, where `(x0,y0)` is the zero-based crop origin. Address alternate WCS and distortion conventions explicitly. Test standard celestial WCS first; when a distortion cannot be shifted safely, annotate it and apply the accepted pixel mask without changing geometry rather than skipping pixel processing or silently producing false cropped coordinates.
- Preserve science meaning rather than promising byte-identical headers after modifications. Enumerate allowed card changes and verify every other card. Verify exactly one HDU in every science output, including cases where an HDU-copy API might create an extra primary unit.
- Reopen completed products and verify structure, invalid-value semantics and checksums before publishing them. Preserve failed temporary files only under a documented diagnostics option; otherwise clean them up.

## Architecture and output layout

Keep a thin `src/main.rs`; pure algorithms in a library enable tests without the CLI. Suggested modules: `cli`, `config`, `pipeline`, `fits/reader`, `fits/writer`, `fits/header_policy`, `detection/background`, `detection/features`, `detection/components`, `detection/decision`, `mask`, `crop`, `logging`, `report`, `error`. Each substantial struct and its implementation gets a focused file; exact boundaries follow actual responsibilities. Do not build unused abstractions.

```text
input/config.toml                   editable runtime configuration
input/doc/                          guides, methods, plan, references
output/.state/program-version.txt   local ignored build counter
output/runs/<run-id>/images/          processed products or symlinks to unchanged inputs
output/runs/<run-id>/masks/           separate mask FITS files
output/runs/<run-id>/previews/        optional labeled before/after crop PNGs
output/runs/<run-id>/summary.json     deterministic run summary
output/runs/<run-id>/per-image.csv    row-oriented run summary
output/runs/<run-id>/activity_*.jsonl structured activity log with ISO timestamp
```

Every log entry has a UTC ISO timestamp, level, run ID, event and relevant file ID. Record invocation, configuration identity, dependency/version information, discovery, eligibility, detector decisions, writes, exceptions, timing, worker/memory settings and final counts. Record major activities rather than logging every pixel. Flush on shutdown; logging/report initialization failures must fail before processing. Handle interrupts by marking the run incomplete and flushing the completed-file manifest when possible; abrupt process termination cannot guarantee a final summary.

Each candidate has exactly one terminal category: `clipped`, `kept`, `ignored` or `error`. A kept image may have `review_required=true`. Dry runs classify changes as proposals and clearly state that no science output was written. Summary fields include totals, categories/reasons, input/output paths, dimensions, original-invalid count, newly masked count/fraction, crop origin, additional crop loss, scores, warnings, elapsed time and output sizes. Sort file records deterministically. Enforce `total_candidates = clipped + kept + ignored + error`; report non-FITS directory entries separately. Exit 0 on a complete run without errors, 1 on per-file failures, and 2 on invalid configuration or fatal startup failure. Continue independent files after recoverable errors.

## Phases and acceptance gates

The table records the original acceptance gates; implementation state and evidence are summarized in the progress table below. Each coherent phase should update documentation and tests and be registered in a local Git commit. Record actual commands and evidence in `input/doc/validation.md`.

| Phase | Work | Completion evidence |
| --- | --- | --- |
| 1. Feasibility and contracts | Prototype pure-Rust FITS reads/writes, single-HDU eligibility, header preservation, raw integer/null handling and WCS metadata; inventory representative samples | Round-trip tests cover float and integer data, scaling, retained cards and exact HDU count; backend/toolchain selected; unresolved formats documented |
| 2. CLI and pipeline foundation | Cargo project, typed config, discovery, eligibility, atomic outputs, structured logger and summary schema | End-to-end no-op kept-symlink run, invalid-config/path tests, deterministic manifest and balanced counters; help/config guide available |
| 3. Detector research | Seeded synthetic generator, source-aware robust features, components, conservative decision rules and evidence diagnostics | Ground-truth masks and null/confounder cases evaluated; thresholds selected on development data; algorithm documented and frozen before held-out evaluation |
| 4. Safe mask writer | Apply masks, sentinel policy, header/checksum updates and provenance | Retained pixels exact, invalid semantics correct, one-HDU outputs; independent FITS-reader verification |
| 5. Optional crop mode | Valid rectangle/circle selection, loss limits, WCS/geometry updates | Geometry unit/integration tests and Astropy world-coordinate comparison for the supplied cropped frames |
| 6. Performance | Bounded file-level scheduling, allocation/I/O improvements and memory accounting | Serial/parallel scientific outputs agree; measured throughput and peak memory for multiple sizes/worker counts; no shared FITS handles |
| 7. Release validation and documentation | Full regression matrix, representative real data, extensive help/README, guides and acceptance audit | Every requirement maps to code/tests; failures documented; reproducible benchmark/science report; scoped commit and clean task-owned worktree |

For concurrency, use independent parser/writer instances per file and do not share mutable FITS objects between workers. Limit concurrent images by estimated resident bytes as well as CPU count. Oversized files receive a clear resource error until a validated tiled path exists. Avoid nested pools. Sorting results must make scheduling independent of scientific output; timestamps and timings may differ.

## Test and validation matrix

- **Eligibility/discovery:** zero-length/corrupt files, one versus many HDUs, table-only/empty/cube images, ordinary and compressed formats, unrelated directory entries, nested paths, symlinks, duplicate names, non-ASCII paths and empty input.
- **Scientific patterns:** single corner, one side, unequal strips, rotated/curved boundaries, circular/elliptical field, rectangle, partly symmetric and mixed components, weak/strong contrast and different border widths. Include gradients, flat/constant data, shot/read noise, correlated noise, saturation and existing invalid regions.
- **False-positive controls:** no-artifact sky, crowded fields, bright edge stars, extended galaxies, nebulosity, faint edge targets, moving/trailing sources and smooth vignetting. Validate conservative rejection/retention when evidence is inadequate.
- **Numerics:** BITPIX/storage combinations, unsigned scaling, 64-bit integers beyond exact float representation, NaN/infinity, BLANK collisions, zero MAD, tiny dimensions, extreme sizes and safe allocation arithmetic.
- **Scientific preservation:** exact retained raw samples; unchanged usable-source aperture fluxes; mask-aware background effects; TNO/trail recovery; WCS agreement through an independent implementation; round-trip header-card preservation and valid checksums.
- **Reliability:** permissions, output collisions, disk/write failures, interrupted work, unavailable logs, per-file errors with continued processing, temporary cleanup, no input modification and complete summary accounting.
- **Determinism/performance:** fixed seeds, repeated runs, worker-count equivalence, stable result order, bounded memory and throughput benchmarks with machine/storage context.

Proposed initial quantitative gates, to freeze in phase 3 before held-out testing: 100% preservation of retained raw pixels and required metadata invariants; no newly masked pixels on deterministic artifact-free fixtures; at least 95% artifact recall on clearly separable synthetic borders; aggregate valid-pixel false-removal rate at most 0.1%, also reported per image/instrument and against protected science apertures. Report boundary error and abstention/review rate so conservative abstention cannot hide poor detection. These are targets, not achieved results or guarantees for every instrument. Define a WCS numerical tolerance appropriate to the tested projection/library and record it before comparisons.

Use meaningful unit/property tests, CLI integration tests and small deterministic generated FITS fixtures. Cross-check outputs with an independent FITS/WCS reader in test tooling where practical. Store fixture-generation metadata and licenses/provenance for any real samples; keep large observations out of Git. Expected Rust checks once code exists: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --all-targets`, release-build CLI smoke tests and recorded benchmarks. These checks have now been run for the current prototype; see the dated validation record. Their passing results do not replace held-out scientific validation.

## Documentation and final reconciliation

Deliver user guide, complete CLI help with examples and exit codes, configuration reference, algorithm assumptions/limitations, architecture, FITS/header policy, validation procedures, benchmark procedure and README build/install instructions. All detailed documents live in input/doc/. Maintain .gitignore and local milestone commits.

Before declaring completion, create a requirement-to-evidence table covering: one-HDU eligibility; all requested geometries; statistically supported decisions; preserved headers/pixels; CLI directory arguments; final summary; configuration/output/logging layout; English documentation; tests; performance; modularity; and local Git history. Link each to implementation paths, actual tests and outcomes. Mark incomplete items explicitly. Real-image validation remains a prerequisite to a scientific-release claim if representative instrument samples are unavailable.


## Implementation progress (2026-10-27)

| Phase | Status | Evidence / remaining work |
| --- | --- | --- |
| 1. Feasibility and contracts | Implemented for 2D BITPIX 8/16/32/64 and -32/-64 | Pure-Rust fitskit round trips cover float64, int64, scaled int16, uint8, BLANK and headers. Compressed and multi-HDU inputs are explicitly ignored. Independent Astropy and fitsverify validation passed. |
| 2. CLI and pipeline foundation | Implemented | Typed operational and heuristic TOML, required directories, tested recursive discovery, dry run, overwrite, bounded workers, ISO JSONL log, JSON/CSV report and exit behavior. |
| 3. Detector research | Provisional heuristic implemented | Synthetic tests cover corner, side, rectangle, pixelated circle, asymmetry, oversized component, interior anomaly, gradient and clip-versus-keep utility. Threshold calibration on labeled and held-out instrument samples remains outstanding. |
| 4. Safe mask writer | Implemented for supported FITS cases | Separate mask, undefined-value handling, headers, retained samples and checksums are verified; Astropy and fitsverify pass on synthetic and real products. |
| 5. Optional crop mode | Implemented, opt-in | Maximum-area clean rectangle versus largest artifact-free circle; selects greater retained pixel count, reports artifact removal and unmasked loss, and applies the configured utility gate. FITS crop products retain raw stored samples, update NAXIS/CRPIX, and pass Astropy/fitsverify checks on the supplied frame set. |
| 6. Performance | Measured | On 25 supplied files, 4 workers produced 1.71x observed throughput versus 1 worker; peak RSS was 853.3 MiB versus 273.6 MiB. Decisions and products matched. See [validation record](validation.md). |
| 7. Release validation and documentation | Baseline checks passed; release gates open | 47 tests passed again during the deep audit; CLI help/version, independent FITS checker and supplied-data measurements are recorded in [validation.md](validation.md). Adversarial probes expose additional failures listed in the [audit](scientific-audit-2026-10-27.md). The supplied set has 5 malformed quoted headers and no independent artifact labels; scientific thresholds and instrument-level performance remain unvalidated. |

The current processor is useful for controlled prototype evaluation, not unattended scientific production. Every detected mask must be reviewed against representative images before downstream measurements.
