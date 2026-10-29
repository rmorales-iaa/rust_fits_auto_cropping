# Scientific implementation audit — 2026-10-27

Scope: review the current code, build identity, README, plans, skills, memory and validation evidence to design the next implementation. This audit does not implement the proposed algorithms or certify scientific accuracy.

## Baseline and evidence

- Reviewed source revision: `626f53c52e4fdd59e7e90ed86cd050c09cd8016c`, branch `master`; clean at review start, no remote configured.
- Cargo package version: `0.1.0`; tracked build template: `0.0.0`; existing release executable: `0.0.29`. These are distinct identities. The review's `cargo test --locked --all-targets` compiled test artifacts as `0.0.30` and advanced ignored local state; it did not replace the release executable.
- All 47 existing tests passed again: 25 unit, 18 integration and 4 version tests. Passing these tests does not cover the failure cases below.
- Local adversarial probes used the existing release executable, unchanged default TOML files, Astropy 7.2.0 and NumPy 2.4.6. Evidence, command arguments, executable hash, fixtures and a repeatable `probe.py` are in `output/reviews/science-plan-20261027-4hmuy3p9/`. These generated files are ignored by Git; portable fixture recipes are below.
- Prior supplied-data evidence remains in [validation](validation.md): 16 clipped, 4 kept, 5 malformed-header errors; independent FITS/PNG checks passed. No artifact ground truth exists for those frames. Their repeated use for design feedback makes them development examples, not a future untouched hold-out set.

## Findings mapped to planned work

Severity describes consequences for scientific reliability, not a claim that every observation triggers the issue. Source locations refer to the reviewed revision. `Observed` means exercised with an actual executable; `inspection` means established from code; `hypothesis` requires measurement.

| ID | Evidence and consequence | Required response / plan phase |
| --- | --- | --- |
| A01 — high | **Observed:** `tile_detector.rs:260–270` expands whole tiles. A 20×32 zero border on a 96×96 constant-100 image leaves 128/640 artifact pixels; a 28×32 border removes 128 valid pixels in addition to all 896 artifacts. Default 16-pixel tiling therefore gives 80% recall in one case and 1.54% valid-pixel loss in the other. | Multiscale proposals followed by pixel-level boundary inference; quantitative truth metrics. P3–P4. |
| A02 — high | **Inspection:** `tile_detector.rs:81–119` assumes central reference tiles and uses the **maximum** interior tile noise as typical noise. No source exclusion, per-tile valid-count gate or uncertainty on the estimated reference exists. Noise-scale is MAD **of tile sigmas**, not typical pixel noise or a standard error. | Robust local reference selection and source-aware residual/noise model, with explicit support and scale floors. P2–P3. |
| A03 — high | **Inspection:** `correlation.rs` measures raw adjacent-value Pearson correlation with as few as eight pairs; one usable direction is enough. Trends can create high correlation, the axes are averaged, and no spatial-dependence calibration exists. | Detrend locally; keep directional evidence and pair counts; calibrate the complete region-selection procedure. P3. |
| A04 — high | **Inspection:** all edge seeds are flooded together; `tile_detector.rs:175–201` aggregates disconnected components into one tile count, median evidence and area utility. One component can subsidize another; unweighted tiles differ in pixel area at partial edges. Rejected masks are discarded. | Stable component labels, independent support/decision diagnostics, area-aware aggregation and retained proposal diagnostics. P3–P5. |
| A05 — high | **Inspection:** mask utility penalizes the entire candidate fraction; crop utility penalizes only unmasked geometry loss. `pipeline.rs:296–355` allows cropping only after the mask utility accepts, so a candidate rejected under one loss function never reaches the other. Candidate membership is not truth. | Separate segmentation from action selection; compare feasible actions using the same declared objective and uncertainty. P5. |
| A06 — high | **Inspection:** `crop.rs:100–198` searches integer-centered circles using nearest masked-center distance minus half a pixel diagonal and image-edge constraints. The rectangle is exact for its binary mask and axis-aligned family. Neither maximizes true valid science area over arbitrary shapes or accounts for pre-existing undefined pixels. | State the geometric domain, preserve the rectangle oracle, quantify circle discretization/clearance loss, and compare freeform masking with constrained crops. P5. |
| A07 — high | **Observed:** `fits_image.rs:173–195` computes an unused code before detection, even for kept/dry-run images; `unwrap_or(unused_integer_value(...)?)` eagerly evaluates its fallback even with an existing BLANK. A uint8 image containing all 256 codes errors both with no BLANK and with valid `BLANK=255`. | Defer sentinel allocation until a chosen action needs new undefined samples; search retained defined codes, honor existing BLANK lazily. P1. |
| A08 — high | **Observed:** integer analysis re-adds `BZERO` after subtracting its raw anchor (`fits_image.rs:191`). With identical stored int16 samples, `BZERO=0` clips a 16×32 border and `BZERO=1e20` keeps it because small differences vanish. Floating midpoints and correlation sums also lack extreme-value guards. | Centered analysis coordinates with explicit precision/overflow checks and affine-invariance tests; preserve raw storage separately. P1–P2. |
| A09 — medium | **Observed:** gzip bytes named `.fits` are reported as an error. `pipeline.rs:248–258` recognizes compression by suffix; content is not checked before parsing. | Recognize supported compression signatures/structural conventions for exclusion without decompression; retain malformed-input error distinctions. P1. |
| A10 — high | **Observed:** a side-strip crop retains only value 100 but still has `DATAMIN=0`; its mask inherits `BUNIT=adu`. **Inspection:** writer filtering omits BUNIT, science extrema are not refreshed, and circle products receive rectangle-specific HISTORY text (`writer.rs:313–317`). The observed long generation HISTORY text is shortened to fit a card. | Separate science/mask card policies, dimensionless mask units, accurate geometry/history, explicit long-card handling, allowed-change audit. P1. |
| A11 — high | **Inspection:** the writer shifts CRPIX but has no unsupported-distortion guard. Independent WCS checking exercises five points in the primary WCS, only for nonzero crop offsets, with `equal_nan=True`. Shared NaNs can pass; alternative WCS and distortion domains lack a release matrix. | Supported-convention allowlist, finite checks and independent dense retained-pixel checks for primary/alternate WCS; keep unsupported crops. P1/P7. |
| A12 — high | **Inspection/hypothesis:** 32 bytes/pixel preflight plus `budget / maximum_image_mib` worker count is not a peak-live-allocation bound. Raw arrays, conversion/sort buffers, serialized clones, verification rereads, two full EDT arrays and PNG buffers overlap by stage. Large-file RSS and small-file concurrency are uncharacterized. | Account for stage lifetimes, weighted memory admission, bounded buffers and measured RSS overhead. P0/P6. |
| A13 — medium | **Inspection/hypothesis:** repeated full-array sorts in integer conversion, two sorts per robust summary, allocated correlation pairs, repeated FITS parsing, serialization cloning, per-line EDT scratch allocation, and an O(N) finite copy for 1,000-sample ZScale. They are optimization candidates, not measured speedup claims. | Profile first; exact selection/histograms, buffer reuse, stable streaming moments, fewer reads and reusable geometry scratch behind equivalence gates. P6. |
| A14 — high | **Inspection:** `FileOutcome` does not distinguish original-invalid, suspected-artifact, applied-mask and geometry-sacrifice sets. Rejected crops can be kept while retaining nonzero proposed `masked_pixels`; dry-run proposals share clipped status without a summary-level dry-run field. | Versioned report semantics, disjoint counts, proposal/action distinction, reason codes and explicit dry-run/review state. P1/P4/P5. |
| A15 — medium | **Inspection:** raw-pixel verification occurs after publication, with rollback for ordinary errors; reports use direct writes; interruption recovery is absent. Repeated source opens only partly detect changes. PNG failure leaves valid FITS products with error status. | Verified staging plus completion manifest, stable source identity, fault injection and documented partial-product state; distinguish preview failure. P1/P6. |
| A16 — medium | **Inspection:** local build counters can differ across binaries and repeat across machines; provenance lacks source revision/dirty flag, executable identity and versioned algorithm/schema identity. | Retain required base-100 counter; add immutable build/science identities and counter concurrency/failure semantics. P1. |
| A17 — medium | **Inspection:** original plan still says 40 tests; architecture incorrectly says physical-valued integer analysis, mask-unit filtering and all validation before publication. Large modules (`writer.rs` 752 lines, `pipeline.rs` 518, `crop.rs` 377) contradict the small-file target. Skills describe intended practices, not proof of implementation. | Correct current documentation now; focus refactoring on measured ownership boundaries, not cosmetic fragmentation. P0/P6. |

## Portable probe recipes and observed outcomes

Write FITS fixtures with Astropy `PrimaryHDU(data)` (all dimensions below are `(height,width)`), use the unchanged `input/config.toml` and `input/heuristics.toml`, and run the release CLI with one worker and a distinct output root. Use mask mode except the header probe. Inspect JSON reports and read mask bytes independently with Astropy.

| Fixture | Recipe | Observed result in release 0.0.29 |
| --- | --- | --- |
| Misaligned narrow border | int16 `(96,96)`, value 100, set `data[:20,:32]=0` | 512 artifact pixels removed, 128 artifacts retained, 0 valid removed |
| Misaligned wide border | same, `data[:28,:32]=0` | 896 artifact pixels removed, 128 valid removed |
| Offset equivalence pair | same, `data[:16,:32]=0`, `BSCALE=1`, `BZERO=0` versus `1e20` | clipped versus kept; same stored array |
| Full-code uint8 | `tile(arange(256,dtype=uint8),64).reshape(128,128)` | error before decision; no unused BLANK |
| Existing full-code BLANK | preceding array with `BLANK=255` | same erroneous fallback failure |
| Hidden compression | gzip the first ordinary fixture, name result `compressed_named_fits.fits` | error rather than ignored |
| Header policy | int16 `(96,96)`, 100 except `data[:16,:]=0`, `BUNIT=adu`, `DATAMIN=0`, `DATAMAX=100`; crop mode | actual minimum 100, science DATAMIN 0; mask BUNIT adu |

These deliberately small fixtures identify counterexamples; they are not performance measurements or estimates of real-world error rates. No supplied observation was edited. Exact command and output paths are retained in local `probe-results.json` and `header-probe-results.json`.

## Interpretation

The useful engineering foundation is real: pure-Rust FITS handling, deterministic rectangle search, provenance, symlink retention, isolated outputs and independent sample/checksum checking. The next work must repair correctness gaps and measure scientific loss before stronger statistical machinery or performance changes are promoted. A more elaborate detector alone would not resolve the observed FITS, reporting and numerical failures.

The [active scientific improvement plan](scientific-improvement-plan.md) maps all findings to deliverables, acceptance gates and stopping criteria. Historical validation remains valid for the cases it actually checked; it does not establish the wider guarantees now required.

Planning closure: five refinement passes incorporated source, scientific, statistical and implementation findings; two subsequent coverage/consistency passes found no new material omission. All nine skill files passed the skill-creator validator and relative documentation links resolved. The current code and runtime thresholds remain unchanged. Implementation and real-data release validation are still required.

## Implementation follow-up (2026-10-27)

The first code slice addresses specific reproduced P1 failures without changing the detector thresholds or promoting new science claims:

- **A07:** raw integer analysis values are median-centered; no synthetic BLANK is allocated during reading. Mask and circular-crop writers select a replacement from retained stored values only when needed. Full-range uint8 retained input now has an end-to-end regression and stays a source symlink. The no-available-code fallback for an action that requires new undefined samples remains open.
- **A08:** integer and floating analysis buffers use median-centered stored values; the scale/zero cards are validated but not included in detector residuals. A regression confirms identical masks with large BZERO/BSCALE changes and unrelated OBJECT text. Extreme-value precision behavior still needs dedicated oracle tests.
- **A09:** gzip content signatures are checked before FITS parsing, so a gzip stream named `.fits` is ignored. Suffix checks still cover `.gz` and `.fz` conventions.
- **A10:** dimensionless masks omit BUNIT; derived science products drop stale DATAMIN/DATAMAX; HISTORY entries are split to bounded card text and crop shape wording matches the selected circle or rectangle. Astropy and fitsverify independently accepted the output products from the recorded development run.
- **A14:** `original_undefined_pixels`, `proposed_mask_pixels` and applied `masked_pixels` are now distinct; dry runs set `dry_run`, count accepted decisions as `proposed`, and keep applied counts at zero. Crop utility counts exclude pre-existing undefined samples and include sacrificed defined samples. Full intersections, reason-code stability, schema versioning and candidate/action alternatives remain open.

The P1 fixes and reporting changes are local implementation work; do not infer that A01–A06 or A11–A17 have been resolved. In particular, the tile detector remains coarse and uncalibrated. Current suite: 51 tests (27 unit, 20 end-to-end, 4 version tests). Formatting and Clippy pass. The actual implementation and remaining gates are tracked in [the active plan](scientific-improvement-plan.md) and [validation](validation.md).

## Second engineering follow-up (2026-10-27)

This slice adds a typed no-unused-integer-code condition and keeps affected inputs as unchanged symlinks rather than failing or corrupting their undefined semantics. Integer value spans beyond the exact f64 range abstain from detection. Crop mode keeps files with recognized lookup-table/distortion WCS conventions. FITS summaries now use schema 2 and identify the algorithm/policy, Git revision/dirty state, executable hash and source/effective config and heuristic hashes. The correlation implementation removes allocated pair vectors and uses rescaled Welford moments; small exhaustive mask patterns independently check rectangle and circle geometry. Border-connected components now pass support/area/evidence/provisional utility gates independently, and report counts expose candidate and accepted components.

Validation: 60 Rust tests passed; rustfmt and Clippy passed. Release `0.0.57` processed the recurring 25-frame development corpus (16 clipped, 4 kept, 5 malformed-header errors) in 4.37 s with 976,992 KiB peak RSS at four workers and PNG enabled. Astropy and fitsverify accepted all 20 science products and 16 masks. Three no-PNG crop runs measured 13.16 s median / 307,964 KiB median RSS at one worker and 3.80 s / 923,276 KiB at four workers. Astropy confirmed identical pixel arrays for all 36 outputs across all six trials. Fixed trial order, one machine/dataset, and no pre-change algorithm baseline limit inference; this establishes current workload repeatability and worker equivalence, not a measured algorithm speed gain or hard resource bound. The data have no independent artifact labels. P1 remains partial; A04 is partially mitigated at component gating; A01–A03, A05–A06 and other open findings are not cleared. See [the active plan](scientific-improvement-plan.md) and [validation](validation.md).

## Revision-2 addendum (2026-10-29)

Scope: a deep review of the current head (`71ec091`, package `0.1.0`; this container's build counter was `0.0.1`, while the user's host recorded `0.2.74` for the latest crop validation). It covered the README, skills, plan and memory, plus six deterministic detector probes and one large-frame timing observation. The 102 Rust tests, strict Clippy and formatting passed. No detector code, threshold or default was changed. The findings, design responses and roadmap are in [plan Revision 2](scientific-improvement-plan.md) §0, §4A and §8.

### Portable probe recipes (shipped `input/heuristics.toml`, mask decision only)

Call the crate-internal `detect_border(data, 256, 256, rules)` from a temporary `#[cfg(test)]` module. Use a fixed-seed LCG (`s = s·6364136223846793005 + 1442695040888963407`, uniform `((s>>11)+0.5)/2⁵³`) with Box–Muller for Gaussian noise. Sky is 1000 + N(0, 10²). A "star" adds `A·exp(−r²/2σ²)`. Truth is the stated strip; FP counts mask pixels outside truth. Observed outcomes:

| Probe | Recipe | Outcome |
| --- | --- | --- |
| A19 quiet | columns x<40 replaced by 300 + N(0, 2²) | accepted; TP 8,192, FN 2,048 |
| A19 amplified | columns x<40 replaced by 300 + N(0, 60²) | `no_border_candidate`; FN 10,240 |
| A20 edge source | star A=3000, σ=14 px at (0,128); no artifact | accepted; FP 2,816 |
| A03 vignetting | subtract `D·(1−e/48)²` for edge distance e<48; D=400 / 800; no artifact | accepted; FP 15,616 / 28,672 (same with or without an interior star) |
| A18 step | columns x<48 replaced by 700 + N(0, 5²); with/without interior star A=20000, σ=8 px at (128,128) | without star: missed (FN 12,288); with star: accepted (TP 12,288) |
| A01 strips | columns x<20 / x<28 set to 0 | FN 1,024 / FP 1,024 |

Mechanisms, by code inspection: A18 follows from `typical_noise = max(interior tile σ)` in `tile_detector.rs` for `central_tiles`. A19 follows because the candidate rule accepts only `quiet` (σ below a fraction of reference), `correlated` or exact-uniform tiles. A20 and A03 follow because raw, undetrended lag-one correlation of a smooth profile exceeds 0.95 and is sufficient alongside contrast.

Timing observation (release build, single run, 4-core container): synthetic 7100×5325 frame with a 300-column zero strip. `single_scale` took 1.68 s and `multiscale_consensus` 5.44 s. An O(n) selection median over all 147,852 16×16 tiles took 0.118 s. The mask covered 304 columns. This is a hint for R0, not a G5 benchmark.

Additional inspection findings: A21 (dominated keep fallback after a failed crop cap), A22 (tolerant crop retains flagged samples as data), A23 (anchor-restricted residual search), A24 (dimensionless utility and component-count tiers), A25 (0.5 mask-fraction veto), A26 (sort/allocation cost), A27 (documentation drift, fixed in this revision), A28 (unused master-flat pixel truth), A29 (no null false-alarm control), A30 (undefined label rule for partial illumination) and A31 (transient mask FITS write + fsync + verify per processed image). The responses are mapped in the plan. User decisions D1–D4 are open.
