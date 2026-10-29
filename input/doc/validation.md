# Validation record

## Source-snapshot and completion-manifest follow-up (2026-10-27)

The parsed source FITS object now remains available through detection, writing and in-process verification. A writer regression replaces the input path after decoding and proves generated pixel samples and headers still come from the same snapshot that was analyzed. Successful kept/clipped inputs receive an atomically published `manifests/<relative-path>.complete.json`; a marker is absent for corrupt input and dry runs. Tests check generated image/mask/preview sets and kept-image symlinks. The report schema is 3 and per-image CSV/JSON includes the manifest path.

`cargo test --locked --all-targets` passed 61 tests (35 unit, 22 end-to-end and 4 version tests). `cargo fmt --all -- --check` and `cargo clippy --locked --all-targets -- -D warnings` passed. Release `0.0.63` ran three crop-mode/no-PNG trials at one and four workers, interleaved over the same 25-image development corpus. Serial medians were 12.97 s and 311,644 KiB peak RSS; four-worker medians were 3.81 s and 976,432 KiB. Parallel RSS varied from 932,140 to 1,109,752 KiB. All six runs had 16 clipped, 4 kept and 5 malformed-header errors, and Astropy confirmed identical pixels across all 36 science/mask arrays. Astropy plus fitsverify accepted 20 science images and 16 masks for one current-code trial; 20 completion markers were present.

The measurements show no clear runtime benefit from retaining the parsed snapshot (earlier release medians were 13.16 s serial and 3.80 s with four workers), and four-worker median RSS was higher than the previous 923,276 KiB observation. The worker trials used one host and dataset with a warm, interleaved run sequence; they are not an algorithm benchmark or memory guarantee. Completion markers detect interrupted product sets but do not make separate file renames transactional. The dataset is development material without independent artifact labels, so science gates remain open.

## Second scientific-plan engineering slice (2026-10-27)

The current implementation passed `cargo test --locked --all-targets`: 60 tests total (34 unit, 22 end-to-end and 4 build-version tests). `cargo fmt --all -- --check` and `cargo clippy --locked --all-targets -- -D warnings` passed. Added tests cover integer precision abstention, overflow-safe float centering and correlation, no-available-BLANK keep fallback, unsupported lookup-distortion crop refusal, independent disconnected-component gates, and exhaustive small-grid circle/rectangle geometry references.

Release `0.0.57` processed the repeatedly inspected 25-image development set in `/fast_disk/tmp/clippling_test/input/`, writing a new run to `/fast_disk/tmp/clippling_test/output/followup-0.0.57/runs/20261027T135512.136105834Z/`. Results: 16 clipped, 4 kept, 5 errors; the errors are the known malformed `INSAPNAM` quoted strings and remain explicit. The release run took 4.37 s wall time and reached 976,992 KiB maximum RSS on four workers with previews enabled. This is one observation, not a repeated paired benchmark; it supports no speed or memory improvement claim. The report uses schema 2 and records algorithm/policy IDs, Git revision/dirty state, executable hash, and source/effective configuration hashes.

Current-revision outputs were independently opened by Astropy and checked by `fitsverify`: all 20 generated science images and 16 masks passed; the five unparsable inputs have no products. Astropy emitted compatibility/deprecation warnings for legacy input WCS/time cards. This establishes interoperability for the readable files and checker scope, not WCS support for every convention, artifact truth, source completeness, photometric bias, or detector validity. The input corpus is development data, not an independent hold-out. The run exits with status 1 because the five malformed headers are reported as batch errors.

No current-code repeated paired benchmark, independent labeled science corpus, complete WCS matrix, transaction-interruption test, or image-level memory cap has been established. The relevant G1/G2/G4/G5 release gates remain open.

### Repeated worker comparison

The same input corpus and crop-mode configuration were run three times at one worker and three times at four workers, with PNG previews disabled, using release `0.0.57` and a 2048 MiB configured budget. Each run wrote to a distinct isolated output root under `/fast_disk/tmp/clippling_test/output/benchmarks/followup-0.0.57/`. Trials ran in serial-then-parallel order, so warm-cache and fixed-order effects remain possible.

| Workers | Wall-time trials | Median wall time | Peak RSS trials (KiB) | Median peak RSS |
| ---: | --- | ---: | --- | ---: |
| 1 | 13.15, 13.16, 13.17 s | 13.16 s | 307,964; 308,020; 307,884 | 307,964 KiB |
| 4 | 3.80, 3.85, 3.80 s | 3.80 s | 922,944; 997,276; 923,276 | 923,276 KiB |

This is a 3.46× ratio of observed median throughputs for this specific workload, while four-worker median peak RSS is 3.00× the serial median; one of the parallel RSS trials was higher at 997,276 KiB. All six runs had the same summary counts (16 clipped, 4 kept, 5 malformed-header errors). Astropy compared all 36 science/mask pixel arrays against the first serial trial, and all arrays were identical across all repetitions and both worker counts. FITS headers include run-time provenance, so byte-level product hashes are expected to differ. These measurements establish repeatability and parallel/serial output-array equivalence on this corpus, not a before/after improvement from the new correlation/component algorithms, a hard memory bound, or external scientific validity. Raw time records and logs are in the benchmark output directory above.

## Scientific-plan implementation slice (2026-10-27)

Implementation build `0.0.34` on the unchanged 25-image development set in `/fast_disk/tmp/clippling_test/input/` wrote a new run to `/fast_disk/tmp/clippling_test/output/runs/20261027T131800.144164298Z/`. It reported 16 clipped, 4 kept and 5 errors. The same five frames have malformed quoted-string headers (`unterminated string value`) in the historical runs; they were not reclassified as successful. The run used the default 2048 MiB budget and four effective workers. `/usr/bin/time -v` measured 3.35 s wall time and 952,624 KiB peak RSS. This is one observational run, not the repeated paired G5 benchmark and not evidence of better science accuracy.

`tools/cross_validate_fits.py` independently checked all 20 output science images and 16 masks with Astropy and `fitsverify`; all passed. This confirms FITS structure, checksums, samples, mask geometry, WCS behavior for the present frames, and provenance under the checker’s current scope. It does not validate artifact labels, boundary truth, photometric bias, all WCS conventions, or the five inputs that could not be parsed.

After the implementation changes, `cargo test --locked --all-targets` passed 51 tests (27 unit, 20 end-to-end, 4 build-version checks); `cargo fmt --all --check` and `cargo clippy --locked --all-targets -- -D warnings` passed. New regressions cover gzip bytes named `.fits`, scale/descriptive-header invariance, full-range uint8 images retained without eager sentinel allocation, reuse of an existing BLANK without searching for another code, stale extrema and mask units, undefined-pixel exclusion and dry-run proposal versus applied counts. The release executable used for the real-data smoke run was built before the final regression tests; source behavior under those tests was verified by the debug test binary.

The latest release build is `0.0.44`. Its follow-up dry run on the same 25 development images reports 16 proposed, 4 kept and 5 errors, with `dry_run: true`, `clipped: 0`, and summed applied `masked_pixels: 0`. It creates no science products. This confirms release-build report behavior; the five malformed FITS headers still yield a nonzero batch exit status.

## Deep review follow-up (2026-10-27)

The [scientific audit](scientific-audit-2026-10-27.md) reran `cargo test --locked --all-targets`: all 47 tests passed with test build `0.0.30`. The existing release executable remained `0.0.29`. New independent fixtures nevertheless reproduced tile-boundary under/over-removal, a large-BZERO detection change, full-code uint8 BLANK failures, gzip-content misclassification, stale crop extrema and incorrect mask units. These cases are unresolved implementation work in the [active plan](scientific-improvement-plan.md). Prior interoperability results below apply to their actual fixtures and do not certify arbitrary headers, scientific validity or a complete memory bound. No new performance claim is made by this review.

## Earlier evaluation evidence

The supplied-dataset runs documented below were generated by the previous `fits-auto-clipping` name and flat output layout. Their files are historical evidence and were left unchanged during migration. New invocations use `output/runs/<run-id>/` and identify as `rust_fits_auto_cropping`.

## Automated Rust checks

Run from the repository root with the pinned pure-Rust FITS dependency:

- `cargo test --all-targets` — 47 tests (25 unit, 18 end-to-end and 4 build-version checks) pass after the layout/name migration.
- `cargo clippy --all-targets -- -D warnings` — pass.
- `cargo fmt --all --check` — pass.
- `cargo build --release` — pass.
- `cargo tree` — FITS I/O uses `fitskit`; there is no `fitsio`/CFITSIO dependency.

The integration matrix covers synthetic one-corner, one-side, rectangular, pixelated circular and asymmetric masks; smooth-gradient and interior-anomaly controls; mask/crop utility decisions; rectangle-vs-circle area selection; every supported BITPIX family; scaling and retained raw samples; BLANK retention and out-of-range BLANK handling; malformed/truncated inputs; gzip and tiled-compressed inputs; multi-HDU files; non-ASCII/nested paths; `--recursive`; dry run; reports/logs; checksums; serial/parallel equivalence; output path sanitation; and run isolation/preservation. The fixtures validate software invariants, not observatory-level detector recall.

## Supplied real-data run

Inputs were read from `/fast_disk/tmp/clippling_test/input/`. All products, reports, logs and benchmark evidence were written under `/fast_disk/tmp/clippling_test/output/`. The directory contained 25 ordinary single-HDU 2D FITS images, approximately 274 MB. It contained no compressed or multi-HDU examples. SHA-256 manifests before and after processing each contained 25 entries and matched; the input set was not modified.

Run command:

```bash
target/release/rust_fits_auto_cropping \
  --input-dir /fast_disk/tmp/clippling_test/input \
  --output-dir /fast_disk/tmp/clippling_test/output \
  --config input/config.toml \
  --heuristics input/heuristics.toml \
  --recursive --threads 4 --memory-budget-mib 2048
```

The initial mask-mode baseline classified 13 clipped, 4 kept, 0 ignored and 8 errors. It predated the user-directed change to ignore invalid BLANK metadata for pixel analysis; its earlier report remains available for comparison. The final crop-mode run below is the current result. Five source headers still have an unterminated quoted `INSAPNAM` value and remain errors. The input set has no independent artifact labels, so accepted clips/crops are proposals requiring instrument-aware review and cannot calibrate scientific thresholds.

Main outputs:

- Legacy summary: `/fast_disk/tmp/clippling_test/output/reports/20261027T103352.466257843Z/summary.json`
- Legacy per-image decisions: `/fast_disk/tmp/clippling_test/output/reports/20261027T103352.466257843Z/per-image.csv`
- Legacy science products: `/fast_disk/tmp/clippling_test/output/images/`
- Legacy companion masks: `/fast_disk/tmp/clippling_test/output/masks/`
- Legacy activity logs: `/fast_disk/tmp/clippling_test/output/logger/`
- Benchmark measurements: `/fast_disk/tmp/clippling_test/output/benchmarks/benchmark-summary.json`

### Final crop-mode run

```bash
target/release/rust_fits_auto_cropping \
  --input-dir /fast_disk/tmp/clippling_test/input \
  --output-dir /fast_disk/tmp/clippling_test/output/crop-mode \
  --config input/config.toml \
  --heuristics input/heuristics.toml \
  --recursive --mode crop --threads 4 --memory-budget-mib 2048
```

This run classified **16 clipped, 4 kept, 0 ignored and 5 errors**. It completed all readable inputs; the non-zero exit code reflects the five malformed quoted headers. The target frame had `BLANK=32768` with `BITPIX=16`; that out-of-range metadata was ignored for sample statistics, and the cropped output has a valid `BLANK` sentinel. The current run used program version `0.0.12`. Kept images are symlinks to the corresponding input files; generated crop/mask products include FITS program/version and UTC time cards. Source hashes still match the pre-run manifest.

For the named target, crop mode selected a circle centered at `(840,840)` with radius `712.1133` pixels. Its FITS bounding box is `[128,1553) × [128,1553)`, or `1425×1425`. The circle retains 1,593,141 pixel centers, 31.5% more than the best clean rectangle (1,211,136); it removes 530,752 candidate pixels and sacrifices 503,723 unmasked pixels. The configured circle crop utility is `0.6542`, so the crop was accepted. The crop's corners inside the bounding box are marked undefined. These links refer to the preserved legacy flat-layout run: [crop preview](/fast_disk/tmp/clippling_test/output/diagnostics/target-circular-crop.png), [science product](/fast_disk/tmp/clippling_test/output/crop-mode/images/science%232007_01_11T20_23_31_000%23493-CAHA%23CAHA_2_2M%23OPEN%232002TC302%23500s%231626x1616%2311jan_0011.fits), and [run summary](/fast_disk/tmp/clippling_test/output/crop-mode/reports/20261027T110212.459560680Z/summary.json).

## FITS interoperability

### Source-digest follow-up

In the source-digest follow-up, SHA-256 is computed during FITS parsing through the same open file descriptor, including padding skipped by forward seeks. Version-4 per-image JSON/CSV rows and version-2 completion manifests first recorded the input digest; schema 5 retains it and adds reason codes and set counts below. Version-3 manifests now also hash every product. Source size and modification time are checked across parsing; detectable in-place changes fail that file. Kept-source symlinks are rehashed before publishing their completion marker and fail if they no longer resolve to the parsed bytes. A regression checks the digest against an independent SHA-256 of the exact FITS bytes, and the source-replacement writer test confirms the digest remains attached to the parsed source snapshot. The current release validation and per-product digest checks are recorded in the follow-up below. Multi-file publication still has no filesystem transaction guarantee.

### Decision and crop-set accounting follow-up

Report schema 5 added stable top-level `reason_code` categories and initial candidate, proposal, applied and crop-geometry counts. Its `artifact_pixels_removed` and `unmasked_pixels_sacrificed` counts partition defined crop exclusions into detector proposal overlap and geometry-only loss. Release `0.0.79` processed the supplied 25-image set in 3.86 s with 973,060 KiB peak RSS at four workers (single observation); Astropy and fitsverify verified 20 science images and 16 masks, 20 source digests independently matched, and all 25 CSV records had 39 columns. Five malformed-header errors remain. This validates engineering interoperability and accounting, not artifact classification.

### Per-product digest follow-up

Completion-manifest version 3 now stores a SHA-256 for every science image, mask and preview, including kept symlinks, as well as the parsed input digest. The end-to-end crop test recomputes each digest; an independent check of release `0.0.84` matched 20 source and 36 product digests. Astropy and `fitsverify` accepted 20 science images and 16 masks, and CSV rows were consistently 39 columns. On the same supplied corpus, this one four-worker run took 3.88 s and peaked at 950,508 KiB RSS. Release `0.0.79` without per-product hashing took 3.86 s / 973,060 KiB in its one run; that comparison is not paired or controlled, so it does not establish a runtime or memory improvement. Product hashing adds reads; repeat randomized paired trials before making a performance claim. The run again classified 16 clipped, 4 kept, and 5 malformed headers.

### Stage-specific output error codes (2026-10-27)

Output failures now report stable codes for path construction, existing products, cleanup, science-product writes, kept-source symlink publication, PNG previews, and completion-manifest publication. The generic `processing_error` remains a fallback for an uncategorized failure. The malformed-FITS integration regression asserts `input_read_error`; this slice does not yet fault-inject each filesystem/output failure stage. All 62 tests pass, with formatting and strict Clippy clean.

### C/A/E/U0 intersection follow-up

Report schema 6 records candidate/proposed/applied artifact intersections with original undefined samples, candidate/proposal overlap, artifact pixels excluded by crop geometry, proposed versus applied crop overlap with original undefined pixels, and accepted artifact pixels retained inside proposed crop geometry. This closes the report-count portion of the P1 set-accounting item; detector components select only finite samples, and tests assert those U0 intersections remain zero. A synthetic crop with a NaN outside the selected geometry verifies proposed undefined sacrifice is 1 while applied undefined sacrifice remains 0 when crop utility rejects the proposal. All 62 tests pass (35 unit, 23 end-to-end, 4 version); formatting and strict Clippy pass.

Release `0.0.87` processed the supplied 25-image development set in crop mode with four workers: 16 clipped, 4 kept and 5 malformed-header errors. The run took 4.00 s and peaked at 953,572 KiB RSS. Astropy and fitsverify accepted 20 science images and 16 masks. Independent checks found schema 6, 25 CSV rows with 45 columns each, 20 version-3 completion manifests, matching hashes for 20 source files and 36 products, and valid report set-count invariants. This is one uncontrolled observation and the repeatedly inspected corpus has no independent artifact labels; it establishes engineering interoperability/accounting only, not speed, memory bounds or scientific accuracy.

`tools/cross_validate_fits.py` opens science and mask products independently with Astropy and invokes `fitsverify` when installed. It checks readable FITS structure, HDU count, dimensions, BITPIX, checksums, raw crop samples, circle/rectangle exclusion masks, CRPIX updates and world coordinates at corresponding retained pixels. The final crop-mode run passed: Astropy verified 20 science images and 16 mask products, four unchanged images resolved through symlinks to their inputs, and fitsverify checked all generated products. The mask-mode baseline and both serial/parallel benchmark outputs also passed their earlier checks. Independent checksum verification caught a non-canonical CHECKSUM card comment in an earlier writer version; the writer was corrected to checksum the exact serialized bytes.

Kept-image output paths are symlinks to their matching input paths. Cross-validation confirms each target resolves to the corresponding source. Generated FITS products are independently checked for program/version (`CREATOR`), UTC product date (`DATE`) and generation timestamp (`HISTORY`).

## Performance and memory

Measurements used the release binary and `/usr/bin/time -v`, processing the same 25 input files with a 2048 MiB configured application budget. A 32-logical-CPU host reported 125 GiB RAM and NVMe storage. This is a single run per worker count; serial ran first, so filesystem cache state may favor the parallel timing. Measurements are observational, not a robust benchmark distribution.

| Workers | Wall time | Successful outputs/s (17 files) | Valid pixels/s (approx. 121.8M) | Peak RSS |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 12.19 s | 1.39 | 10.0 Mpx/s | 280,116 KiB (273.6 MiB) |
| 4 | 7.13 s | 2.38 | 17.1 Mpx/s | 873,780 KiB (853.3 MiB) |

Four workers gave 1.71x observed throughput and 3.20x peak RSS. The process stayed below the configured 2 GiB budget in this run; RSS includes runtime and parser overhead. Serial and parallel runs produced identical decisions and output-product hashes. The timing and memory logs, console outputs and input hash manifests are preserved in `output/benchmarks/`.

The final crop-mode run took 4.22 s and reached 894,476 KiB peak RSS on four workers (single run; cache was warm). This included circle/rectangle selection and FITS output for 25 inputs. Do not compare this single timing directly with the earlier mask-mode serial/parallel results as a benchmark distribution.

## Crop decision and heuristic caveats

Crop mode is opt-in. It selects the circle or rectangle retaining more pixel centers, then compares evidence-weighted artifact removal against unmasked pixels sacrificed. The example crop passes the current configurable utility rule but still requires human review; the utility is not a calibrated scientific loss model. Crop WCS checks passed with Astropy on the supplied outputs.

### Optional ZScale crop previews

The `--mode crop --png` smoke run used the same 25 supplied frames and four workers. It wrote 16 side-by-side PNGs for the 16 accepted crops; the four kept files and five files with malformed headers had no preview. The five errors match the pre-existing `INSAPNAM` issue above. Output PNG bytes totaled 183,634,118. Wall time was 4.54 seconds and peak RSS was 911,572 KiB (one warm run; not a benchmark distribution). The prior crop-only run was 4.22 seconds and 894,476 KiB, so the rough difference includes PNG rendering/encoding and run-to-run variation.

`tools/cross_validate_previews.py` independently checked all 16 PNGs with Astropy `ZScaleInterval` and Pillow: panel dimensions, 1,000 sampled source intensity mappings per image, matching source/crop samples at retained pixels, FITS lower-origin orientation and black circular exterior. All passed with program version `0.0.16`. Synthetic tests also cover preview labels/orientation, shared-panel scaling, circular exterior masking and flag/mode validation. This confirms display interoperability for this dataset; it does not validate the scientific crop decisions.

### Renamed executable and isolated output-layout verification

The final run used `rust_fits_auto_cropping` version `0.0.28` and the sanitized layout at `/fast_disk/tmp/clippling_test/output/final-verification/runs/20261027T112150.081806592Z/`. The command was:

```bash
target/release/rust_fits_auto_cropping \
  --input-dir /fast_disk/tmp/clippling_test/input \
  --output-dir /fast_disk/tmp/clippling_test/output/final-verification \
  --config input/config.toml \
  --heuristics input/heuristics.toml \
  --recursive --mode crop --png --threads 4 --memory-budget-mib 2048
```

It generated 20 science images, 16 masks and 16 previews. The summary reports 16 clipped, 4 kept and 5 malformed-header errors; the error count matches earlier runs. Astropy and `fitsverify` accepted all science/mask FITS products, and the preview checker validated all 16 PNGs. The run took 4.56 seconds and peaked at 925,972 KiB RSS in one warm run. Preview data totaled 183,634,118 bytes. A serial/release benchmark was not repeated as part of this naming/layout migration.

Output-layout tests verify output-root symlink/overlap rejection, traversal and symlink-component rejection, collision handling, and preservation of unrelated existing files and prior runs. Full Rust checks on the migrated code passed: 47 tests, formatting, Clippy, release build and CLI help/version.

Scientific rules live in `input/heuristics.toml`. The detector reports candidate evidence, a configurable artifact-benefit diagnostic, valid-pixel-loss penalty and net utility for candidates, including those rejected by support/area gates. These are transparent heuristic units, not probabilities, p-values or measured photometric loss. The default thresholds are provisional and not empirically calibrated. A real science release requires labeled, instrument-specific development data and held-out evaluation.

### Staged science/mask pair publication (2026-10-27)

Mask-mode and crop-mode writers now encode and verify the science image and companion mask under temporary sibling directories on their respective destination filesystems before publishing either final path. If the second rename fails, a rollback restores pre-existing products or removes the first newly published file. Two injected-failure tests verify both cases. Existing integration coverage exercises both writer modes, FITS samples, headers and checksums. The full suite passes 64 tests (37 unit, 23 end-to-end, 4 version), with formatting and strict Clippy clean. Release `0.0.93` processed the 25-frame development set in 3.89 s with 972,660 KiB peak RSS at four workers; 16 images were clipped, 4 kept, and 5 malformed FITS inputs failed. Astropy/fitsverify accepted 20 science images and 16 masks. Independent checks confirmed schema 6, 25 rows with 45 columns, 20 completion manifests, 20 source hashes and 36 product hashes. This single run establishes interoperability for the supplied readable files, not performance or science accuracy.

This protects ordinary errors and prevents readers from seeing a product while it is still being encoded. The image and mask live in separate directories, so the pair is not power-loss atomic: a crash between renames may leave an unmanifested partial set. The completion manifest remains authoritative. Directory fsync, disk-full and process-kill recovery tests remain open.

### Primary and alternate WCS cross-validation (2026-10-27)

`tools/cross_validate_fits.py` now discovers alternate WCS codes, checks every recognized WCS card including alternate CRPIX shifts, and samples a 9×9 grid over each crop. It compares finite forward world coordinates, uses wrapped spherical angular separation for celestial axes, rejects all-nonfinite reference grids instead of accepting shared NaNs, and checks world-to-pixel residuals to 1e-6 pixel when Astropy converges. Four Python unit tests cover primary/alternate crop shifts, zero-origin crops, a deliberately unshifted alternate solution, and all-nonfinite samples. FITS alternate suffix and Astropy `WCS(key=...)` behavior follow the [FITS Standard WCS section](https://fits.gsfc.nasa.gov/standard40/fits_standard40aa-le.pdf) and [Astropy WCS API](https://docs.astropy.org/en/stable/api/astropy.wcs.WCS.html).

Rechecking the release `0.0.93` development outputs passed Astropy and fitsverify for 20 science images and 16 masks. Several legacy SIP inputs have finite forward coordinates matching after crop, but Astropy's inverse transform does not converge for the sampled points; the checker reports that inverse residual as unavailable for those cases and relies on the independent forward-map comparison plus card checks. This corpus does not include an alternate-WCS science frame, so alternate WCS is only synthetically tested here. The checker establishes evidence for these cases, not full WCS support or a general accuracy claim.

## Required scientific acceptance work

Before production use, assemble licensed and documented annotated frames across instruments and test on held-out data. Measure pixel precision/recall, boundary distance, abstention rate, source/aperture flux bias, background changes, faint edge-object and moving-target recovery, trail preservation, and WCS agreement. Include crowding, galaxies, nebulosity, gradients, vignetting, saturation, unusual scaling/null conventions and all supported storage types. Freeze thresholds on development data before the hold-out evaluation; keep all candidate masks reviewable.

### Experimental robust-affine tile reference (2026-10-27)

Added an opt-in `robust_affine` reference model that fits a bounded Huber plane to finite central-interior tile medians. The default remains `central_tiles`; model identity is recorded in the run-start event and summary. Synthetic unit/integration cases verify exact affine-gradient null retention, separable border-seam detection, one-outlier resistance, and abstention on insufficient or rank-deficient support. The full Rust suite passes 70 tests (42 unit, 24 end-to-end, 4 version); the four WCS-checker tests pass, Python compilation succeeds, formatting is clean, and strict Clippy passes.

This experiment has not been run against labeled observatory frames or evaluated for source masking, uncertainty coverage, boundary error, photometric bias, and threshold calibration. Its existence does not close P2 and does not change the default algorithm.


### Interleaved release performance trials (2026-10-27)

A release `0.1.1` binary ran crop mode without PNG on `/fast_disk/tmp/clippling_test/input/` (25 files), with one warm-up per worker count followed by 10 measured runs each at 1 and 4 workers. The measured schedule was randomized with seed `20261027`; each trial had a separate run directory. A standard-library Python harness invoked GNU `/usr/bin/time`, retained logs and products, recorded binary/config hashes and checked a canonical per-image decision digest. Command:

```bash
python3 tools/benchmark_release.py \
  --binary target/release/rust_fits_auto_cropping \
  --input-dir /fast_disk/tmp/clippling_test/input \
  --output-dir /fast_disk/tmp/clippling_test/output/rust_fits_auto_cropping-benchmark-20261027-final \
  --config input/config.toml --heuristics input/heuristics.toml \
  --runs 10 --warmup-runs 1 --workers 1 4 \
  --memory-budget-mib 2048 --seed 20261027
```

| Workers | Measured trials | Median wall time (range) | Median peak RSS (range) |
| ---: | ---: | ---: | ---: |
| 1 | 10 | 16.22 s (14.26–44.17 s) | 312,116 KiB (311,616–312,276 KiB) |
| 4 | 10 | 4.495 s (3.89–10.68 s) | 945,900 KiB (768,488–1,028,208 KiB) |

All warm-up and measured runs had the same decision digest, `402f1ff5…`, and the same summary: 16 clipped, 4 kept, 5 errors from the known malformed quoted headers, 0 ignored. The benchmark evidence, per-trial logs, time records, reports and products are under `/fast_disk/tmp/clippling_test/output/rust_fits_auto_cropping-benchmark-20261027-final/`; `benchmark-summary.json` includes the complete values and digest. Independent Astropy plus `fitsverify` validation passed on the final measured run (`20261027T175236.101190405Z`): 20 science images and 16 masks. Legacy FITS cards generated expected Astropy deprecation/date warnings, and inverse WCS residuals were unavailable for several SIP cases even though forward coordinates matched. A preceding trial set without explicit warm-ups is separately preserved at `/fast_disk/tmp/clippling_test/output/rust_fits_auto_cropping-benchmark-20261027-run2/` and is not used for these medians.

The wall-time tails are large, especially in serial runs, and there is only one host and one repeatedly inspected development corpus. This measures worker-count tradeoffs, not a code revision speedup. It does not establish a hard memory bound, cold/warm performance separation or science quality; G5 and scientific release gates remain open.


### Unix publication durability barriers (2026-10-27)

Staged FITS bytes are synced before rename. After science/mask pair publication, the destination parent directories are synced; rollback syncs the restored directory entries. Kept-image symlink publication, PNG publication, activity-log creation and completion-manifest publication sync their parent directory. Explicit overwrite syncs removal of the old completion marker before replacing products. Output-directory creation syncs the relevant parent entries. This is best-effort Unix directory fsync; non-Unix directory syncing is a documented no-op in the standard-library helper.

Validation: two durability-helper unit tests passed, the full Rust suite passes 72 tests (44 unit, 24 end-to-end, 4 version), strict Clippy and formatting pass. Release `0.1.5` processed the supplied 25-frame development set with four workers (16 clipped, 4 kept, 5 malformed-header errors); Astropy and `fitsverify` accepted 20 science images and 16 masks. Evidence is under `/fast_disk/tmp/clippling_test/output/durability-sync-check-20261027/runs/20261027T182049.972720961Z/`. This verifies normal operation and interoperability, not simulated crash recovery. Directory barriers do not make the image/mask pair transactional, eliminate the crash window between their renames, or guarantee device write-cache durability.

### Reason-plane semantics and metadata policy audit (2026-10-27)

The independent checker now verifies every optional reason plane against its binary action mask and original image: code 3 must identify exactly the original undefined samples; codes 1 and 2 together must identify exactly the defined samples excluded by the action mask; retained defined samples must be code 0. It rejects unsupported codes, shape/type mismatches, reason planes without a corresponding mask, and dimensional units on masks/reason planes. For science, mask and reason products it compares source metadata card values/comments against a documented preservation policy and rejects unexpected non-structural output cards. Reason outputs also require `RSCHEME=PIXACT1` and compatible WCS cards.

All 12 tests in `python3 -m unittest tools.test_cross_validate_fits -v` passed. The checker revalidated release `0.1.33` run `20261027T202641.065615444Z` from the requested 25-file dataset: Astropy verified 20 science images, 16 masks and 16 reason planes; `fitsverify` checked the generated FITS products. Four unchanged symlinked images resolve to original inputs. Astropy emitted warnings for legacy PC/RADECSYS/PROJPn cards and an inconsistent DATE-OBS/MJD-OBS pair in source headers. Some SIP world-to-pixel residuals remained unavailable because Astropy did not converge; the independently sampled finite forward coordinates matched where available. The run passed the checker's tested scope, not full WCS-convention coverage, labeled artifact accuracy, photometric integrity or held-out scientific validation. The five malformed source FITS files remain batch errors.

### One output image per discovered FITS input (2026-10-27)

Normal runs now produce one corresponding `images/<relative-path>` entry for every discovered FITS input. Malformed/read-failure and unsupported/ignored files are annotated and preserved through canonical source symlinks; their per-input manifests hash the linked file and retain the original `error` or `ignored` status. Summary schema 8 records `total` and `output_images`; dry-run remains output-free. The output-parity integration tests cover malformed FITS, multi-HDU FITS, gzip streams and tiled-compressed FITS, with source-target, byte identity, manifest and count assertions. The locked Rust suite passes 83 tests, Python FITS-checker tests pass 14, formatting and strict Clippy pass.

Release `0.1.44` at clean Git revision `a1ef5a97967b` ran crop mode with previews and reason masks on `/fast_disk/tmp/clippling_test/input/`, using four workers and recursive discovery. Run `20261027T224357.086048273Z` reports `total=25`, `output_images=25`, 16 clipped, 4 kept, 0 ignored and 5 malformed-header errors. All five malformed inputs have annotated source symlinks; output symlinks preserve their original bytes. Astropy/fitsverify checked the 20 parseable image outputs, 16 masks and 16 reason planes; the independent preview checker verified all 16 PNGs. The run exits with status 1 because malformed input errors remain visible, even though every input has an output image entry. The science classification remains unvalidated on independent labels.

### RGB crop-preview boundaries (2026-10-27)

Release `0.1.50` ran with four workers, recursive discovery, crop mode, PNG previews and reason masks on `/fast_disk/tmp/clippling_test/input/`. Run `20261027T224741.776915429Z` reported 25 inputs and 25 published image entries: 16 clipped, 4 kept and 5 malformed-input errors. The malformed inputs were annotated and symlinked unchanged. All 16 RGB previews passed `tools/cross_validate_previews.py` checks for green retained-region contours, red adjacent excluded-region contours, unchanged grayscale crop-panel samples, source-derived ZScale and geometry. The preview for `science#2007_01_11T20_23_31_000#493-CAHA#CAHA_2_2M#OPEN#2002TC302#500s#1626x1616#11jan_0011.fits` was visually checked; its source panel shows the circle crop boundary around the retained field.

Astropy and `fitsverify` accepted the 20 parseable science outputs, 16 masks and 16 reason planes; the five pass-through symlinks were validated by target. Astropy emitted legacy-header warnings and could not compute inverse WCS residuals for several SIP cases, while sampled forward coordinates matched. The full locked Rust suite passes 84 tests, strict Clippy and formatting pass, and Python checker compilation succeeds. Overlay lines are display annotations and are not written into FITS arrays. This validation checks implementation/interoperability on the development corpus; it does not establish labeled scientific accuracy.

### Rectangular FITS crop dimensions (2026-10-28)

Optimized build `0.1.60` ran with four workers, recursive crop mode, PNG previews and reason masks on the same 25 inputs. Crop support compares the maximum clean rectangle with a conservative pixel-mask-derived circle; when the circle contains more clean pixel centers, output is the exact maximum-area axis-aligned rectangle wholly inside that circle. In every accepted crop, serialized `NAXIS=2`, `NAXIS1/2`, decoded image dimensions, and crop origin agree. No circle or newly undefined circular bounding-box corners are written. Run `20261027T230523.268308343Z` published 25/25 image entries: 15 clipped, 5 kept, 5 malformed-input errors. The malformed inputs have annotated symlinks. Astropy/fitsverify validated 20 parseable science entries, 15 masks and 15 reason planes; the independent preview checker verified all 15 RGB previews. It also verified rectangular crop cards and checked preview colors, geometry and sampled ZScale pixels.

The specifically discussed `science#2007_01_11T20_23_31_000#493-CAHA#CAHA_2_2M#OPEN#2002TC302#500s#1626x1616#11jan_0011.fits` was kept unchanged by the configured utility rule: its candidate rectangle was `1131×1131`, with 933,415 unmasked defined pixels sacrificed, so no crop product or preview was emitted for it. This follows the keep-on-excessive-loss policy. Another circular-field crop in the run was emitted as a `1085×1085` rectangle. Astropy produced legacy source-header warnings and could not compute inverse residuals for several SIP WCS cases; sampled forward coordinates matched. The full Rust suite passes 84 tests, strict Clippy/formatting and 14 Python checker tests pass. These are engineering and FITS-interoperability checks on the repeatedly inspected development corpus, not a labeled accuracy assessment.


### Experimental multiscale consensus evidence (2026-10-27)

Added `heuristics.evidence_model = "multiscale_consensus"` as an opt-in mode. It evaluates up to three deterministic tile widths, retains only finite pixels selected by at least two accepted scale masks, and reapplies image-level area/evidence/utility gates. Unsupported scale sets, one-scale-only candidates, and null images abstain. New algorithm IDs cover all reference/evidence combinations; the default single-scale ID and behavior remain unchanged.

Synthetic controls cover a two-scale-supported corner, separate corner components, an offset corner boundary, an exact affine-gradient null, and a candidate accepted at only one scale. An end-to-end test verifies configuration and report/log provenance. Full suite: 78 Rust tests (49 unit, 25 end-to-end, 4 version); strict Clippy and formatting pass. Four independent Python WCS checker tests also pass. A release `0.1.15` dry run on the supplied 25-frame development set recorded 16 proposals, 4 kept images, and 5 malformed-header errors; it wrote no science products. This is not a labeled evaluation. The mode adds up to three detector passes, has no shifted origins, source masks, uncertainty calibration or observatory-label evaluation, and remains experimental. P3 remains partial.

## Optional reason-plane implementation and interoperability — 2026-10-27

The opt-in `--reason-mask` feature now emits an original-frame uint8 FITS sidecar for accepted mask/crop actions. `PIXACT1` codes are 0 retained defined input, 1 detector-selected artifact excluded, 2 defined pixel sacrificed only by crop geometry, and 3 pre-existing undefined input (highest precedence). Reports use schema 7; completion manifests hash the sidecar like every other product. A unit matrix checks code precedence and dimensions, and end-to-end mask and circular-crop tests verify codes 1/2/3, preserved dimensions, and manifest hashes. An additional writer regression normalizes legacy value-style HISTORY text into standard commentary without generating CONTINUE cards.

Release build `0.1.33` ran on the requested `/fast_disk/tmp/clippling_test/input/` dataset with `--recursive --mode crop --reason-mask --threads 4`. The final-source run at `/fast_disk/tmp/clippling_test/output/runs/20261027T202641.065615444Z/` reported 25 total, 16 clipped, 4 kept, 0 ignored and 5 malformed-input errors. The five errors are the previously observed malformed FITS headers; generated outputs completed for the other 20 inputs. Independent Astropy verification and `fitsverify` checked 52 FITS paths (16 generated science images, 16 binary masks, 16 reason planes, and 4 unchanged-image symlinks), with zero FITS errors and no new warnings beyond the source-header warnings. All 16 reason outputs preserved tested WCS reference cards and had matching source shapes. An earlier run against the same data produced the same clipping decisions. That run’s aggregate code counts were 83,463,799 retained, 22,621,072 artifact exclusions, 19,406,377 crop-only sacrifices and 0 pre-existing undefined samples. These counts describe program decisions, not accuracy or science truth.

A five-pair 4-worker release microbenchmark on build `0.1.23` (before the later legacy HISTORY normalization fix) compared reason-plane output on/off, alternating order, on the same development dataset. Medians were 4.90 s / 1,002,808 KiB without the plane and 4.60 s / 982,864 KiB with it; ranges were 4.10–9.94 s / 924,900–1,125,924 KiB and 4.31–6.20 s / 833,704–1,024,580 KiB respectively. Paired decision digests were identical in all five pairs. The timing and RSS variation is larger than the apparent median difference, so no speed or memory improvement/regression is established. Raw samples and run IDs are in `/fast_disk/tmp/clippling_test/output/benchmarks/reason-mask-2026-10-27.json`; this is a feature-cost check, not the registered G5 benchmark.

Final engineering gates for this slice: `cargo fmt --all -- --check`, `cargo test --locked --all-targets` (83 tests), `cargo clippy --locked --all-targets -- -D warnings`, plus Astropy/fitsverify checks above. Reason-plane work is complete. P1 crash/WCS/structural gaps, P2 source-aware model, P3 calibrated evidence, P4 boundary inference, P5 common calibrated utility, P6 registered broad cold/warm performance and memory gates, and P7 independent labeled-data science validation remain open in the active plan. This release does not claim science-grade detector accuracy.

### Global maximum rectangle regression — 2026-10-28

The prior circle-constrained selector returned a `1085×1085` crop for `science#2007_01_11T20_57_04...`, retaining 1,177,225 pixels. Crop selection now searches the complete combined detector-exclusion and originally undefined-pixel mask using the exact histogram/monotonic-stack maximum-rectangle algorithm; a fitted circle does not constrain the crop. This image now selects origin `(336,144)`, size `1040×1296`, and retains 1,347,840 pixels (170,615 more than the prior crop). It removes 487,744 detector-proposed artifact pixels and sacrifices 792,032 unmasked pixels; the configured utility gate accepted the crop. The target FITS dimensions and `NAXIS1/2` agree.

Release `0.1.64` processed the supplied 25 files recursively in crop mode with four workers, PNGs and reason masks. Run `20261027T231651.090556482Z` published all 25 output entries: 16 clipped, 4 kept, 5 malformed FITS errors. The malformed inputs remain annotated source symlinks. Independent Astropy plus `fitsverify` validation passed for all 25 entries, 16 masks and 16 reason planes; preview validation passed all 16 PNGs. Rust validation passed 82 tests (52 unit, 26 end-to-end, 4 version), `cargo fmt --all -- --check`, and strict Clippy. The exhaustive small-grid oracle checks exact rectangle area for all 3×3 binary masks. This is geometric optimality under the current detector mask, not validation that the mask matches scientific truth; the dataset remains repeatedly inspected development data without independent labels.

### Crop sacrifice guard — 2026-10-28

Review of `science#2007_11_08T21_05_00_300...` showed that the exact maximum clean rectangle was `1520×2992` (4,547,840 pixels) from a `2154×4200` image. It removed 2,250,768 detector-proposed artifacts while sacrificing 2,248,192 other defined pixels: 33.1% of the remaining defined, non-artifact area. The previous utility score accepted the crop because detector evidence was used as a multiplicative benefit despite the comparable sacrificed area. Added `heuristics.maximum_crop_sacrifice_fraction` (default 0.25); a proposed rectangle above this cap is rejected and the source image is preserved unchanged with reason code `crop_sacrifice_limit`. The threshold is provisional and requires labeled-data calibration.

Release `0.1.68` on the same 25-file development dataset published all 25 image entries: 5 crops accepted, 15 images kept, and 5 malformed FITS errors preserved as annotated symlinks. The target frame is kept unchanged. All 5 generated FITS/mask/reason products passed Astropy plus `fitsverify`; all 5 previews passed the independent preview checker. `cargo test --locked --all-targets` passed 84 tests (53 unit, 27 end-to-end, 4 version), strict Clippy passed, and the configured cap has input-validation and keep-as-symlink regression coverage. The increased keep count reflects the explicit conservative crop-loss cap; it is a policy tradeoff, not science-label validation. Run: `20261027T232407.709789486Z`.

Release `0.1.71` includes the output naming update: generated crop FITS files append `_cropped` before their extension; kept and pass-through images retain their original names. The same 25-file corpus again published 25 entries (5 clipped, 15 kept, 5 malformed errors). For the reviewed frame, `images/science#2007_11_08T21_05_00_300...r594244-3.fits` is the unchanged source symlink because its crop exceeded the sacrifice cap. Astropy/`fitsverify` accepted all 25 image entries, 5 masks and 5 reason planes; the independent preview validator accepted all 5 generated crop previews.

### FITS crop dimensions and preview alignment — 2026-10-28

For `science#2007_11_08T20_49_21_500...`, independent inspection confirmed input `NAXIS1/2 = 2154/4200`; generated crop `crop_width/height = 2048/4064`, decoded array shape `(4064,2048)`, and output `NAXIS1/2 = 2048/4064`. The FITS header and array therefore already agreed. Review of the attached before/crop preview exposed a separate renderer defect: it top-aligned the shorter crop image in the full-height comparison canvas, leaving all 136 excluded rows as an unexplained black strip at the bottom. The renderer now maps crop pixels using their source-coordinate `y` offset, so the preview margins correspond to the 104 rows excluded at the top and 32 at the bottom. These margins are display canvas only; no padding is written to the FITS image. The independent preview checker now samples using this source-coordinate alignment.

Release `0.1.74` regenerated the 25-image development run `20261028T091609.089852769Z`: 25/25 output entries, 5 accepted crops, 15 kept and 5 malformed-input errors. Astropy/`fitsverify` validated all 25 image entries, 5 masks and 5 reason planes; preview geometry/scaling validation passed for all 5 generated crops. Rust suite (84 tests), Python FITS checker suite (14 tests), formatting and strict Clippy passed.

### Crop-fit preview without blank panel margins — 2026-10-28

The source-coordinate-aligned preview removed the misleading bottom-only band, but it showed the actual top/bottom crop offsets as black padding. Following visual review, the after-panel now uses a uniform aspect-preserving nearest-neighbor display fit to the source-panel height and is labeled `CROP FIT`; it contains no blank canvas margins. This is a visualization-only resampling: FITS data and `NAXIS1/2` are unchanged. The independent preview checker now validates fitted-panel dimensions and represented source samples.

Release `0.1.80` reran the supplied 25-file corpus as run `20261028T093318.351294889Z`: 25 output entries, 5 accepted crops, 15 kept images and 5 malformed-input errors. Astropy/`fitsverify` validated all 25 image entries, 5 masks and 5 reason planes; all 5 fitted previews passed the independent ZScale/sample/geometry checker. Rust tests passed (84), strict Clippy and formatting passed.


## Transient mask cleanup and independent preview stretches

The run pipeline now removes `masks/` and `reason_masks/` before writing its final summary, and `OutputLayout` performs best-effort cleanup during unwinding/early return. Completion manifests list only retained science images and previews; JSON/CSV output leaves the removed mask-path fields empty. The PNG after-panel now uses ZScale estimated from finite pixels in the exact cropped data, while the before-panel uses source-image ZScale. The independent Astropy checker computes and verifies separate limits. Focused Rust and integration tests cover cleanup, manifest/report consistency, and a crop whose scale differs from its source; dataset-level verification remains to be run for this change.


The first post-cleanup Astropy cross-check exposed an independent-validator assumption that a cropped image without a retained mask sidecar must equal its entire source. The checker now verifies crop samples directly against the reported source rectangle, with no mask dependency; a focused regression rejects a modified sample. Release `0.1.85` run `20261028T095259.389621545Z` processed 25 inputs: 5 clipped, 15 kept, 5 malformed-input errors preserved as symlinks. Both `masks/` and `reason_masks/` were absent on exit. Astropy independently validated 25 image entries and `fitsverify` accepted every parsed FITS product; the 5 preview PNGs passed 500 sampled pixel checks each using separate source/crop ZScale intervals. The 15 Python FITS-checker tests pass. Existing legacy WCS/time-card warnings remain.

The preview ZScale path was then changed to stream-count finite pixels and collect only the deterministic sample subset, avoiding an additional crop-sized `f64` allocation. Iterator-backed and slice-backed source arrays produce exactly matching sample intervals; Rust unit tests and all end-to-end tests pass. Release `0.1.88` reran the 25-input corpus as run `20261028T095608.478191222Z`: output parity remained 25/25, with 5 accepted crops, 15 kept and 5 malformed-input errors; both transient directories were absent. All 5 previews passed 500 sampled Astropy checks, and Astropy/fitsverify cross-validation passed all 25 entries with zero persistent masks or reason planes. This run validates output consistency after streaming; no controlled timing/RSS benchmark was performed.

Final cleanup hardening also unlinks a transient path if it is replaced by a symlink or regular file, without following its target; a drop-path regression covers recursive cleanup on early exit. Release `0.1.95` rerun `20261028T095757.692575858Z` confirms 25/25 output entries, both transient directories absent, all 5 previews passing 500-sample Astropy validation, and all 25 image entries passing FITS/Astropy/fitsverify validation. Current gates: 55 Rust unit + 27 end-to-end + 4 version tests, 15 Python FITS-checker tests, strict Clippy, `cargo fmt --check`, and `git diff --check`. Five malformed source files remain reported as input errors (exit status 1) while their unchanged symlinks are preserved.

## Decodable images with unsupported crop WCS metadata

Crop mode no longer passes a detected frame through unchanged solely because lookup-table/distortion WCS cards cannot be translated safely. It records the limitation and applies accepted border detections as a dimension-preserving mask; the image grid and WCS reference coordinates remain unchanged. The new `crop_wcs_mask_fallback` end-to-end case uses valid pixels plus `CPDIS1`, verifies that detected border pixels are actually undefined in the generated science FITS, and checks the summary warning and clipped status. Release `0.1.98` corpus run `20261028T100255.644897914Z` remained 25/25 (5 clipped, 15 kept, 5 malformed FITS errors preserved as symlinks); the corpus contains no lookup-WCS fallback case. Astropy/fitsverify passed all 25 entries and independent PNG validation passed all 5 previews. Full Rust tests (55 unit, 27 end-to-end, 4 version), 15 Python checker tests, strict Clippy and formatting pass. Structurally unsupported inputs remain excluded as required by the existing multi-HDU/compression contract; malformed files remain annotated passthroughs because their pixel arrays cannot be safely decoded.

## Crop sacrifice caps by border-connected area count (2026-10-28)

The single 25% cap is now tiered by the number of accepted, distinct detector components that touch at least one image border: 25% for one area, 30% for two, 35% for three and 41% for four or more. Connectedness is evaluated by the detector; one component touching multiple sides counts once. This keeps the stricter original limit for single-area patterns while allowing the previously requested 41% ceiling only when several independent edge areas support the crop. These are provisional policy thresholds on the repeatedly inspected development set, not scientifically calibrated cutoffs.

Release `0.2.6` ran with four workers, recursive crop mode, PNG previews and reason masks on the same 25 inputs. Run `20261028T102000.904896287Z` published all 25 image entries: 8 cropped, 12 kept and 5 malformed FITS errors preserved as source symlinks. The three accepted multi-corner crops each had four accepted border-connected areas and used the 41% tier (observed sacrifice 34.0%, 37.0%, and 40.2%); the other five crops had one accepted area and stayed below 4% sacrifice under the 25% tier. The per-image reasons record both the accepted-area count and the selected limit. All 8 previews passed 500-sample Astropy checks for ZScale, orientation and crop geometry. Independent FITS cross-validation checked all 25 output entries, including the five source symlinks; FITSVerify accepted generated/parseable FITS products, with existing legacy-header warnings. `masks/` and `reason_masks/` were absent. Rust tests passed (57 unit, 27 end-to-end, 4 version), strict Clippy and formatting passed. The five source parse errors still make the batch exit with status 1, as reported in the summary. This run tests implementation and interoperability, not scientific threshold accuracy; no independent artifact labels or controlled performance measurements exist.

The committed table-driven code was rebuilt as release `0.2.9` and rerun with unchanged settings as `20261028T102329.450953167Z`. Decisions were reproducible: 25/25 output entries, 8 crops, 12 kept, and the same 5 malformed-input errors; four-area crops used the 41% cap and one-area crops used 25%. All 8 previews again passed 500-sample Astropy validation. FITS cross-validation covered 25 entries (including the five original-file symlinks), FITSVerify accepted parseable products with legacy metadata warnings, and both transient mask directories were absent. The application returned status 1 solely for the five reported FITS read errors.

## Residual-tolerant crop evaluation — 2026-10-29

The crop selector now accepts a configurable fraction of detector-flagged defined pixels inside a larger rectangle. The shipped provisional ceiling is 5%; originally undefined samples remain excluded. A 100,000-evaluation bounded search expands each side of the exact largest-clean-rectangle anchor. Candidate choice maximizes area only within this search family and still requires the configured crop-sacrifice and minimum-utility gates. Older custom heuristic files that omit the setting remain strict at zero. The report schema is 11 and includes residual flag fraction, search count and pixel step. Optional temporary reason planes use PIXACT2 code 4 for retained detector flags; Python validation checks this code's geometry and action-mask partition.

After raising the input preflight estimate to 40 bytes/pixel for the two crop-search count planes, the four inspected frames were rerun in final release `0.2.74` (`20261029T080236.828463976Z`) with 4 workers and `--png --reason-mask`, isolated from annual outputs. All four were clipped. Strict versus tolerant rectangle sizes were 1024x960 → 1192x1134 (2004-02-16), 624x1184 → 786x1300 (2004-02-17 01:06), 560x1120 → 680x1270 (2004-02-17 02:05), and 608x736 → 863x786 (2005-01-07). Residual flagged fractions were 4.92–4.99%; valid-pixel sacrifice was 9.3–22.1%. Astropy verified source-slice identity, dimensions and supported forward WCS; fitsverify accepted all four products. All four PNGs passed 500-sample preview validation. Temporary `masks/` and `reason_masks/` were absent. Astropy emitted pre-existing deprecated-PC/RADECSYS warnings and reported some inverse WCS residuals unavailable while sampled forward coordinates matched. A separate single four-worker timing sample was 0.22s wall / 153,536 KiB peak RSS; no controlled speedup or regression claim is justified. The repeated examples provide no independent science-label validation, so the 5% cap remains provisional.

### Oversized yearly frames and disputed rectangles (2026-10-29)

Raising the operational workspace estimate to 1,536 MiB allowed the requested 2009 rerun to complete as release `0.2.55`, run `20261029T070704.156467735Z`: all 3,095 inputs have output entries, 1,025 clipped, 2,070 kept and zero errors in 758,857 ms (12m 38.857s). The user stopped the selected-year batch during 2011; 2,675 inputs had completed when interrupted, the sanitized output has no final summary, its temporary `masks/` directory was removed, and later years were not run. Do not treat the partial 2011 output as a complete year.

The four disputed 2004/2005 inputs were isolated in `/fast_disk/tmp/clippling_test/rectangle_investigation_20261029_fpj28i5i/` and reproduced with release `0.2.57`, run `20261029T073318.329119629Z`. Their rectangles and detector proposals exactly match the prior `0.2.49` summaries. An independent maximum-rectangle implementation on each temporary reason plane reproduces all `(x,y,width,height)` bounds. Astropy confirms that each output FITS array is byte-value-identical to the reported source slice and that `NAXIS1/2` match the output shape; `fitsverify` found zero errors. The green rectangle/red detector/orange sacrifice overlay is `reason_mask_overlay.png` in that diagnostic directory. This isolates the narrow geometry to broad detector exclusion masks, including a fragmented mask on the 2005 frame; it does not establish whether those pixels are scientifically unusable. The classification remains unvalidated without independent usable/invalid pixel labels. Temporary reason-mask copies were deleted after the overlay was made.

## Revision 2 implementation evidence (2026-10-29)

Environment: cloud review container, 4 logical CPUs, Rust release builds, Astropy 8.0.2 (pip). `fitsverify` was unavailable for this section (it was installed later; see the Revision 3 follow-up), so FITS checks used Astropy plus the project's checksum, card-policy and sample verification. Branch `claude/eager-galileo-khb8fs`, local commits only. User decisions: D1 `bbox_mask` fallback, D2 opt-in `crop_masked`, D3 flats as evaluation labels only, D4 eta_I = 0.20 and eta_V = 0.50 with Q in between. All data below are synthetic or seeded. Synthetic success is method evidence only; gate G4 (labelled real frames) remains open.

### Speed-only changes (bit-identical)
- **Exact selection (R0.2/R0.3).** A 2,000-trial property test shows bit-identity with the sort reference. The decision digest `6f25a8d0…` over 48 seeded fixtures × 3 detector models matches the previous revision. Detection on a 7100×5325 frame: 1.69 s before, 1.17 s single-threaded (−31%), 0.345 s with 4 threads (three runs each, quiet machine).
- **In-memory mask verification (R0.5).** Previous and new release binaries on 2×18 float32/int16 fixtures in mask and crop modes gave 0 differences in status, counts, geometry, science data or non-provenance cards. fsync calls fell 69→41 (mask) and 64→39 (crop); renames 27→18 and 26→18 (strace).

### Crop actions (R4)
- **`bbox_mask` (D1).** It removes every accepted flag with zero unflagged loss, verified end to end for float32 and int16/BLANK. A full-frame box degrades to `mask`. Legacy rule files keep `keep`.
- **`crop_masked` (D2).** Retained flags are written as BLANK/NaN. The independent checker verifies masked interiors against report counts; preview checks require undefined samples to render black.
- **Global tolerant-rectangle family (R4.2).** It matches an exhaustive 3-state oracle on 3,000 random grids up to 7×7. A separated-block fixture yields 36 px versus 18 for the anchor family. On a 7100×5325 circular field at rho = 0.05: 17,408,034 vs 17,305,599 retained pixels (+0.59%), 0.355 s vs 0.200 s.

### Opt-in v2 detector: null false-alarm gate (R1.2, G3a)
Frame-level false acceptances with exact one-sided 95% Clopper-Pearson upper bounds; seeded families with sigma^2 = 25 + level/2.

| Family | 256 px, n = 2,995 (final code): v1 | 256 px: v2 | 512 px, n = 1,000 (final code): v1 | 512 px: v2 |
| --- | --- | --- | --- | --- |
| flat | 0 (≤ 0.0010) | 0 (≤ 0.0010) | 0 (≤ 0.0030) | 0 (≤ 0.0030) |
| gradient (±40%) | 0 (≤ 0.0010) | 0 (≤ 0.0010) | 0 (≤ 0.0030) | 0 (≤ 0.0030) |
| illuminated vignetting (eta ≥ 0.6) | 12 (≤ 0.0065) | 0 (≤ 0.0010) | 0 (≤ 0.0030) | 0 (≤ 0.0030) |
| bright edge sources | 2,966 (≤ 0.9931) | 0 (≤ 0.0010) | 623 (≤ 0.6484) | 0 (≤ 0.0030) |
| nebulosity | 0 (≤ 0.0010) | 0 (≤ 0.0010) | 0 (≤ 0.0030) | 0 (≤ 0.0030) |
| correlated noise | 0 (≤ 0.0010) | 0 (≤ 0.0010) | 0 (≤ 0.0030) | 0 (≤ 0.0030) |
| quantized low counts | 0 (≤ 0.0010) | 0 (≤ 0.0010) | 0 (≤ 0.0030) | 0 (≤ 0.0030) |

The 256-px column was rerun on the final code (`9d60043`, including source-seed validation) with identical results: v2 had 0 false alarms in every family. An earlier run at `1c7b7ac` gave the same counts. The 512-px column was run on the final code with 1,000 frames per family, where zero failures bounds the rate at <= 0.0030; the full 2,995-frame G3a bound is established at 256 px.

### Opt-in v2 detector: injection recovery (R1.4), final code, 100 frames per cell
Truth: plateau = I; illumination eta < 0.2 = I, eta ≥ 0.5 = V, Q in between (excluded).

| Family | Size | v1 detected / recall / precision | v2 detected / recall / precision | v2 valid px removed (total) |
| --- | --- | --- | --- | --- |
| sharp strip 8–48 px | 256 | 81 / 0.574 / 0.997 | 100 / 1.000 / 0.997 | 2,016 |
| sharp strip | 512 | 79 / 0.614 / 0.996 | 100 / 1.000 / 0.997 | 4,063 |
| corners 10–40 px | 256 | 29 / 0.353 / 0.998 | 58 / 0.828 / 1.000 | 87 |
| corners | 512 | 0 / 0 / – | 11 / 0.203 / 1.000 | 17 |
| raw circular stop (ramp 4–16 px) | 256 | 94 / 0.230 / 0.599 | 100 / 1.000 / 0.9998 | 421 |
| raw circular stop | 512 | 78 / 0.124 / 0.693 | 100 / 1.000 / 0.9999 | 914 |
| flat-divided circular stop | 256 | 0 / 0 / – | 99 / 0.966 / 0.9999 | 317 |
| flat-divided circular stop | 512 | 0 / 0 / – | 98 / 0.963 / 0.9999 | 788 |

Small corners are limited by the configured `minimum_mask_fraction` (0.005 of defined pixels per component), a policy gate rather than an evidence failure. Flat-divided stops first reached only 0.93–0.94 recall. The false-negative locator traced the shortfall to noise clumps in the amplified exterior that were accepted as protected "sources"; local-background validation of seeds (commit `9d60043`) raised recall to 0.963–0.966, meeting the 0.95 target. Excluding the uncertain band was tested and ruled out as the cause (recall 0.931 vs 0.930, lower precision).

### End-to-end products scored independently (R1.5)
`tools/make_synthetic_corpus.py --seeds 8 --group-dirs` produced 72 fixtures (9 families, 8 seed groups, physical noise model). Products were evaluated by `tools/evaluate_masks.py`, which reconstructs removed pixels from products and sources only; intervals are 95% bootstraps over the 8 groups. All 144 product sets per mode passed `tools/cross_validate_fits.py`.

| Detector / mode | Recall | Precision | Valid loss (pooled) | Worst-image valid loss | Residual contamination |
| --- | --- | --- | --- | --- | --- |
| v1 mask | 0.329 [0.240, 0.430] | 0.817 | 0.0124 | 0.1648 | 0.1020 |
| v1 crop | 0.279 | 0.749 | 0.0156 | 0.2408 | 0.1092 |
| v2 mask | 0.9927 [0.9895, 0.9953] | 0.9987 [0.9984, 0.9991] | 0.0002 [0.0001, 0.0003] (Q worst case 0.0023) | 0.0021 | 0.0012 |
| v2 crop | 0.845 | 0.847 | 0.0254 | 0.2130 | 0.0260 |
| v2 crop_masked | 0.9957 | 0.867 | 0.0256 | 0.2132 | 0.0007 |

Crop-mode valid loss is the rectangle's geometric sacrifice around circular field stops under the provisional 35/47% caps; mask mode is the valid-retention optimum on the same detections.

### Probe regressions (seeded, 256×256; also unit tests)
v2 removes 0 valid pixels on the A20 edge source and A03 vignetting nulls (v1: 2,816 / 15,616 / 28,672). It detects A18 identically with or without the interior star, exactly (0 FP, 0 FN). It detects the A19 quiet, amplified and sky-level amplified strips with ≤ 10 FN and ≤ 173 FP, and the A01 20/28-px strips exactly; v1 misses 6 of these 7. On a 7100×5325 strip v2 has 0 FN and 13 FP in 2.6–3.6 s (contended CPU), against v1's 0.37–0.47 s with every noisy-strip pixel missed.

### Test totals
98 unit + 33 end-to-end + 4 version tests (plus 6 ignored manual harnesses), strict Clippy and rustfmt clean; 25 Python tests (19 checker, 6 evaluation tools).

### Yearly batch script review (2026-10-29)

`scripts/process_cropped_years.sh` was rewritten after review. Fixes:
- **Orphaned crop processes.** Background jobs of a non-interactive bash ignore SIGINT, so Ctrl-C used to leave the crop process writing into an already sanitized directory. A signal trap and a wrapper that forwards TERM to the crop process fix this.
- **Path guards** before `--sanitize-output`: absolute, non-root, non-nested roots and no symlinked output directories.
- **Exclusive lock** per output root.
- **Exit-status classification** separates input errors from failures.
- **Per-year report:** summary counts, parity check and elapsed time.
- **Final progress line** is now rendered even when a short run finishes before its log is polled.
- **New options:** multi-year selection, `--mode`, `--heuristics`, `--no-png` and the read-only `--plan`.

Evidence:
- ShellCheck 0.11 is clean.
- `scripts/test_process_cropped_years.sh` passes 42 mock-based checks, including TERM and job-control SIGINT runs; both verify the crop process is gone.
- A real release-binary run on two synthetic years (9 FITS inputs each) gave 9/9 outputs per year with bias/flat links and the missing dark reported.
- The independent FITS checker, extended to the `--flat-output` layout it previously could not read, verified all 18 entries.
- The preview checker verified the 3 previews in each year.

## Revision 3 evidence (2026-10-29)

Environment: cloud container, 4 logical CPUs, release build, Astropy 8.0.2; no `fitsverify`. All data are synthetic. These numbers support decision D5 and the crop policy (plan §0R); they are not a real-data (G4) validation.

### Crop-policy grid (development corpus)

`tools/make_synthetic_corpus.py --seeds 8 --group-dirs` (seeds 0–7, 72 frames in 8 groups) with the v2 detector, scored by `tools/evaluate_masks.py` from products only. The grid crossed residual {0, 0.05} × λ {none, 0.25, 0.5, 1.0} × cap {0.05, 0.10, 0.20, 0.35} × mode {crop, crop_masked}: 64 runs. Selected rows:

| Policy | Mode | Recall | Precision | Valid loss | Worst image |
| --- | --- | ---: | ---: | ---: | ---: |
| previous (r 0.05, tiers 35–47%, no λ) | crop | 0.8445 | 0.8473 | 0.0254 | 0.2130 |
| r 0, λ 0.5, cap 0.10 (shipped) | crop | 0.9927 | 0.9900 | 0.0017 | 0.0124 |
| r 0.05, λ 0.5, cap 0.10 (shipped) | crop_masked | 0.9927 | 0.9987 | 0.0002 | 0.0021 |
| r 0.05, λ 0.5, cap 0.10 | crop | 0.8975 | 0.9998 | 0.0000 | 0.0007 |
| r 0, no λ, cap 0.35 | crop | 0.9928 | 0.9236 | 0.0137 | 0.3003 |

Twelve of the sixteen r = 0 crop cells match the shipped crop row exactly; the chosen cell is inside that plateau. The full table is in the plan (§0R.2).

### Held-out confirmation

Fresh corpora with disjoint seeds and truth stored outside the input tree:

- `--seeds 8 --seed-offset 5000` (float32, 72 frames);
- `--seeds 4 --seed-offset 7000 --dtype int16` (36 frames).

Each was evaluated once with the shipped rules. 95% intervals are group bootstrap (1,000 resamples).

| Corpus / mode | Recall | Precision | Valid loss | Worst image |
| --- | --- | --- | --- | ---: |
| float32 mask | 0.9921 [0.9875, 0.9963] | 0.9984 [0.9977, 0.9989] | 0.0003 [0.0002, 0.0004] | 0.0025 |
| float32 crop | 0.9921 | 0.9849 [0.9792, 0.9898] | 0.0027 [0.0018, 0.0039] | 0.0048 |
| float32 crop_masked | 0.9921 | 0.9984 | 0.0003 | 0.0025 |
| float32 crop, previous policy | 0.8495 [0.8354, 0.8630] | 0.8116 [0.7829, 0.8472] | 0.0350 [0.0271, 0.0420] | 0.0438 |
| int16 mask | 0.9973 [0.9953, 0.9989] | 0.9980 | 0.0003 | 0.0026 |
| int16 crop | 0.9973 | 0.9856 [0.9837, 0.9875] | 0.0022 [0.0019, 0.0026] | 0.0045 |
| int16 crop_masked | 0.9973 | 0.9980 | 0.0003 | 0.0026 |
| int16 crop, previous policy | 0.8279 | 0.8456 | 0.0231 | 0.0314 |

The Astropy checker (`tools/cross_validate_fits.py`) verified all 72 and 36 products of every run. Crop actions on the float32 corpus were 24 crop, 16 bbox_mask, 8 mask and 24 keep (artifact-free frames).

### Out-of-model nulls (R1.6)

`tools/make_out_of_model_nulls.py OUT 30` writes five families absent from the corpus generator, 30 frames each (seeds from 9000). Output is byte-reproducible.

| Family | v2 frames with removal | v1 frames with removal |
| --- | ---: | ---: |
| cosmic rays (incl. outer-pixel hits) | 0/30 | 0/30 |
| satellite trail | 0/30 | 0/30 |
| saturated edge star with bleed | 0/30 | 0/30 |
| interior hot column | 0/30 | 0/30 |
| crowded edge stars | 0/30 | 1/30 (384 px) |

Zero in 30 bounds the per-family false-alarm rate at ≤ 9.5% (one-sided 95%); zero in 150 bounds the pooled rate at ≤ 2.0%. Parity with G3a needs 2,995 frames per family.

### Test totals after Revision 3

99 unit, 33 end-to-end and 4 version tests (7 ignored manual harnesses); strict Clippy and rustfmt clean. 26 Python tests (19 checker, 7 evaluation tools including one new test for `--seed-offset` and `--truth-dir`). The 42 yearly-script checks pass.

## Revision 3 follow-up: work without observatory data (2026-10-29)

Same environment as above. All evidence is synthetic or engineering evidence; G4 remains open.

### Out-of-model nulls at G3a scale (R1.6)

`tools/make_out_of_model_nulls.py OUT 2995 --family F --seed-offset 10000`, 2,995 frames per family, dry run:

| Family | v2 frames with removal | tile_v1 frames with removal |
| --- | ---: | ---: |
| cosmic rays (incl. outer-pixel hits) | 0 | 0 |
| satellite trail | 0 | 0 |
| saturated edge star with bleed | 0 | 0 |
| interior hot column | 0 | 0 |
| crowded edge stars | 0 | 25 (9,440 px) |

Zero in 2,995 bounds each v2 family's false-alarm rate at ≤ 1.0e-3 (one-sided 95%), the same standard as G3a.

### Positive extended artifacts (risk K2)

Labelled families from the same tool (`--family corner_glow --family light_leak --truth-dir`, 200 frames each). Truth: I where the additive excess is ≥ 3σ, Q from 0.5σ to 3σ. Mask mode:

| Family | v2 frames acted / recall | tile_v1 frames acted / recall / precision |
| --- | --- | --- |
| corner glow | 0/200 / 0.000 | 146/200 / 0.112 / 1.000 |
| light leak | 0/200 / 0.000 | 148/200 / 0.227 / 1.000 |

This confirms K2: v2 protects bright, smooth, edge-anchored excess as if it were a source and removes none of it. A single frame cannot separate such an excess from extended astronomical emission cut by the frame edge (the nebulosity null family). Any rule for it must be opt-in and evaluated against the nebulosity and edge-source nulls first. It is not implemented.

### Shadow comparison and drift monitoring tools (R5.0, R5.2)

- `tools/shadow_compare.py`. A 300-repetition simulation (1,200 frames, enriched disagreement strata) showed no detectable bias in the v2 − v1 valid-loss difference, and 95% intervals covered the truth in 90–99% of repetitions. A census reproduces the population ratio exactly, and an unsampled non-empty stratum is refused.
- Held-out corpus workflow (v2 vs v1, 28 labelled of 72): estimates agree with the full-corpus values within their intervals, and the rollback trigger is not met.
- `tools/drift_monitor.py`. A per-year baseline from the median of the other years (robust to one drifted year) flagged only the injected drift year. The family-wise false-flag rate on homogeneous years was 2.5–3% (target ≤ 5%).

### Exact performance work (R6, K4)

Speed-only changes, each proven decision-identical:
- the equivalence digest is unchanged (now covering three tile_v1 variants plus v2, including uncertain and protected counts);
- 12 run summaries over the development, held-out, int16 and out-of-model corpora in all three modes are identical to the previous build;
- 72/72 science arrays are byte-identical.

New bit-identity tests cover `PixelEval`, selection-based centring (f32/f64/i16 with NaN, ±0 and ties) and chunked banded distances.

Paired, interleaved release benchmark on 37.8-Mpx float32 frames (dry run, 4 workers, 10 measured runs per build after a warm-up; medians with min–max):

| Frame | Wall time old → new | Read | Detect | Peak RSS |
| --- | --- | --- | --- | --- |
| border strip | 5.48 s [5.14–6.67] → 2.23 s [2.19–2.40] | 2.51 → 0.51 s | 2.63 → 1.49 s | 872 → 715 MiB |
| artifact-free | 3.97 s [3.50–4.49] → 1.52 s [1.50–1.56] | 2.74 → 0.50 s | 1.05 → 0.82 s | 872 → 462 MiB |

The two builds' ranges do not overlap. In mask mode, the product write stage fell from 2.6–3.9 s to 1.6 s: the written file is now re-read and its DATASUM checked once, and samples are compared in their stored type. v2 detection remains slower than tile_v1 (about 0.35 s); K4 is reduced, not closed.

### Crop provenance and calibration cropping

- **Cards.** Cropped products carry `CROPPED`, `CROPSEC`, `ORIGNAX1/2`, `LTV1/2`, `CROPUNDF`, `ORIGSHA` and `ORIGFILE`. The writer verifies them on the on-disk snapshot, and the independent checker verifies them against the source file, including its SHA-256. Uncropped products carry none.
- **Full-frame rectangles.** A full-frame `crop_masked` rectangle is now written as a mask (`crop_full_frame_mask`).
- **Calibration tool.** `tools/apply_crop.py` was tested on unsigned int16 bias data (BZERO 32768 preserved; raw samples identical) and on its refusals (size mismatch, uncropped reference, already cropped frame, existing output). Unit tests caught and fixed two defects before release: Astropy dropping BZERO/BSCALE from new HDUs, and an eager underflow in section parsing.
- **Alignment check.** Applying each held-out crop product's section to its own source frame reproduced the product exactly in 59/59 cases: same section and LTV, and identical samples outside the `CROPUNDF` undefined pixels.

### fitsverify

`fitsverify` 4.22 (HEASARC, Ubuntu package) was installed in this container. The independent checker now runs it on every parsed product. On the held-out float32 and int16 corpora, in mask, crop and crop_masked modes, all products pass. Cropped products with the new cards report 0 warnings and 0 errors, and all 59 `tools/apply_crop.py` outputs pass.

### Test totals

103 unit, 33 end-to-end and 4 version tests (7 ignored manual harnesses); 37 Python tests; 42 yearly-script checks. Strict Clippy and rustfmt are clean.

## Verbatim product headers with MO_ provenance (2026-10-29)

At the user's request, products keep the source header as it was. The only exceptions are the NAXIS adaptation and, by the user's choice, the CRPIX shift, so the WCS stays correct. All program information is in `MO_` keywords. Evidence:

- **Unit tests.** Card formatting round trips (floats including subnormals). Assembly edits only the allowed cards and inserts BLANK after NAXISn. The verifier rejects a changed preserved card. Finished HDUs parse with valid DATASUM. MO_ composition handles re-crops and masks of crops. An end-to-end writer test with a value-style HISTORY, an odd-spaced CRPIX, a quoted apostrophe and source DATE/CREATOR/DATAMIN shows every source card copied byte for byte (mask) or with only NAXIS1 and CRPIX1 changed (crop).
- **Pipeline runs.** On the held-out float32 and int16 corpora in mask, crop and crop_masked modes, the strict checker passes: byte-for-byte source cards in order, allowed edits only, MO_ values including MO_SHA against the source file, samples, WCS and fitsverify. Astropy verifies every product's CHECKSUM and DATASUM with warnings treated as errors. A tampered preserved card is rejected by the checker.
- **CHECKSUM layout.** Astropy re-renders the CHECKSUM card with its comment slash in column 32 when verifying. Writing it in that conventional layout was required for Astropy-compatible checksums; fitsverify accepted both layouts.
- **Calibration frames.** `tools/apply_crop.py` (byte-level copy, canonical checksum encoding identical to Astropy's in 2,000/2,000 random values) reproduced 59/59 held-out crops exactly, and all 59 outputs pass fitsverify.
- **Write time.** Products no longer clone and re-serialize the image. Crop-mode write on the 37.8-Mpx frame fell from about 2.0 s to 0.9 s.

