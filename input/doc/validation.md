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

Report schema 5 adds stable top-level `reason_code` categories, the count of finite pixels in edge-connected candidate tiles, proposed artifact pixels, artifact pixels actually applied, and pre-existing undefined pixels excluded by the proposed crop geometry. The existing `artifact_pixels_removed` and `unmasked_pixels_sacrificed` counts continue to partition defined crop exclusions into detector proposal overlap and geometry-only loss. A synthetic FITS regression includes a NaN at an excluded corner and verifies it is reported separately. The detector algorithm and chosen masks are unchanged. All 62 tests (35 unit, 23 end-to-end, 4 version) pass. Release `0.0.79` processed the supplied 25-image set in 3.86 s with 973,060 KiB peak RSS at four workers (single observation); Astropy and fitsverify verified 20 science images and 16 masks, 20 source digests independently matched, and all 25 CSV records had 39 columns. Five malformed-header errors remain. This validates engineering interoperability and accounting, not artifact classification.

### Per-product digest follow-up

Completion-manifest version 3 now stores a SHA-256 for every science image, mask and preview, including kept symlinks, as well as the parsed input digest. The end-to-end crop test recomputes each digest; an independent check of release `0.0.84` matched 20 source and 36 product digests. Astropy and `fitsverify` accepted 20 science images and 16 masks, and CSV rows were consistently 39 columns. On the same supplied corpus, this one four-worker run took 3.88 s and peaked at 950,508 KiB RSS. Release `0.0.79` without per-product hashing took 3.86 s / 973,060 KiB in its one run; that comparison is not paired or controlled, so it does not establish a runtime or memory improvement. Product hashing adds reads; repeat randomized paired trials before making a performance claim. The run again classified 16 clipped, 4 kept, and 5 malformed headers.

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

## Required scientific acceptance work

Before production use, assemble licensed and documented annotated frames across instruments and test on held-out data. Measure pixel precision/recall, boundary distance, abstention rate, source/aperture flux bias, background changes, faint edge-object and moving-target recovery, trail preservation, and WCS agreement. Include crowding, galaxies, nebulosity, gradients, vignetting, saturation, unusual scaling/null conventions and all supported storage types. Freeze thresholds on development data before the hold-out evaluation; keep all candidate masks reviewable.
