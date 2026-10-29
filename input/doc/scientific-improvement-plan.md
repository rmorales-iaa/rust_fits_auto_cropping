# Scientific accuracy, area retention and performance plan

Status: active implementation. **Revision 3 (2026-10-29, §0R)** records decision D5 (v2 is the default before G4) and the evaluated crop policy. It adds a risk register with a pre-registered rollback trigger, an out-of-model corpus (R1.6), shadow comparison with weighted enriched labelling (R5.0), drift monitoring (R5.2) and action-specific G3 targets; review passes 15–19 close it. **Revision 2 (2026-10-29)** was implemented the same day (see §4A implementation status; G4 open). Revision 2 adds a code-and-probe review, new findings A18–A31, a reordered R0–R6 roadmap and closing review passes 8–14 (§0, §4A, §8). Initial correctness/reporting fixes were applied after the 2026-10-27 audit; engineering follow-ups add conservative integer fallback, crop WCS abstention, independent component gates, stable correlation arithmetic, build/run provenance and an opt-in experimental robust-affine reference. Source-aware science-model and release gates remain open. The original audit baseline was source `626f53c`, release executable `0.0.29`, and 47 tests. See [the audit and implementation follow-up](scientific-audit-2026-10-27.md) and [validation record](validation.md).

This is the active implementation plan for scientific improvements. The [original implementation plan](implementation-plan.md) and completed crop/preview/layout plans remain historical records. Completion of the planning review does not mean the software meets the release gates below. No universal science-grade claim is possible without a declared observing domain, independent labels and downstream measurement evidence.

## 0R. Revision 3 — v2 default, crop policy and plan review (2026-10-29)

Revision 3 records two user decisions taken on synthetic evidence, the evidence behind them, the risks they create and the plan changes needed to control those risks. Nothing in this revision is a real-data validation claim; gate G4 is still open.

### 0R.1 Decision D5: v2 is the default detector before G4

The user decided to make `background_v2` the default. Until now R5 allowed promotion only after G1–G5 passed. D5 replaces that rule **for the default only**. The release claim is unchanged: v2 is not validated for scientific release until G4 passes in a declared domain. `detector = "tile_v1"` reproduces earlier runs, the old name `background_v2_experimental` still parses, and every report carries the algorithm ID `border-detector-v2-background`.

Synthetic evidence at the time of promotion (details in [validation](validation.md#revision-3-evidence-2026-10-29)):

- G3a null false alarms: 0/2,995 frames in each of seven null families at 256 px (one-sided 95% bound ≤ 1.0e-3 per family) and 0/1,000 at 512 px.
- Injection recall: 1.000 on strips and raw circular stops; 0.963–0.966 on flat-divided stops.
- Development corpus, mask mode: recall 0.9927, precision 0.9987, valid loss 0.0002.
- Held-out corpus with disjoint seeds, mask mode: recall 0.9921/0.9973 (float32/int16) and precision 0.9984/0.9980.
- Out-of-model nulls (R1.6, new): 0/150 frames for v2 versus 1/150 for v1.

**Risk register.** Each risk has an owner item and an observable signal.

| ID | Risk | Mitigation and signal |
| --- | --- | --- |
| K1 | The reference is wrong when affected meshes form the largest coherent cluster. Support abstention catches only part of this case. | Report the `reference_support_abstained` rate per year (R5.2). G4 includes a stratum of frames with a large affected fraction. |
| K2 | Positive extended artifacts (amplifier glow, light leaks) are protected like sources and stay in the product. This retains valid pixels but leaves contamination. | R1.6 adds labelled positive-artifact families to measure this recall. Report border `protected_source_pixels` per year. |
| K3 | Corners smaller than `minimum_mask_fraction` are kept. | This is a policy gate, not an evidence failure. G4 reports the small-corner stratum separately before any threshold change. |
| K4 | v2 detection is slower than v1 on large frames (37.8 Mpx: about 2–2.6 s versus 0.35 s with 4 workers). | Accepted as an explicit scientific trade-off under the G5 clause. R6 v2 work (border-band meshes, strided mesh subsampling) moves ahead of the other R6 items. |
| K5 | The synthetic evidence is in-model: the same author wrote the generator and the detector. Held-out seeds test sampling variation, not model misspecification. | R1.6 out-of-model corpus; real-data G4. The out-of-model bound is still weak (30 frames per family). |
| K6 | Yearly products already published with v1 differ from v2 products. | R5.0 shadow comparison before any yearly re-run; the algorithm ID in each report separates the two generations. |

**Rollback trigger**, registered before G4 data are seen. The shipped default returns to `tile_v1` if any of the following holds:

- On the G4 probability sample (R5.0), the one-sided 95% upper bound of v2's pooled valid loss exceeds v1's point estimate.
- On flat-certified null frames, v2's group-bootstrap 95% upper FAR bound exceeds 1e-2 per frame.
- A G1 integrity failure is traced to v2.

A rollback is a configuration change; the code stays.

### 0R.2 Crop-policy decision (R4.1 for the synthetic domain)

The user asked to fix the crop limits. Policies were compared on a grid using the v2 detector on the development corpus (seeds 0–7, 72 frames in 8 groups, exact truth). The grid crossed residual ceiling {0, 0.05} × exchange rate λ {none, 0.25, 0.5, 1.0} × sacrifice cap {0.05, 0.10, 0.20, 0.35} × mode {crop, crop_masked}. The selected policy was then evaluated once on a held-out corpus whose seeds and truth were never used for selection.

Development-corpus results (v2; pooled over 72 frames; "worst" is the largest per-image valid loss):

| Policy | Mode | Recall | Precision | Valid loss | Worst image | Residual |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| previous: r 0.05, no λ, tiers 35–47% | crop | 0.8445 | 0.8473 | 0.0254 | 0.2130 | 0.0260 |
| r 0.05, no λ, cap 0.35 | crop_masked | 0.9957 | 0.8666 | 0.0256 | 0.2132 | 0.0007 |
| r 0.05, λ 0.5, cap 0.10 | crop | 0.8975 | 0.9998 | 0.0000 | 0.0007 | 0.0169 |
| **r 0, λ 0.5, cap 0.10 (shipped crop)** | crop | 0.9927 | 0.9900 | 0.0017 | 0.0124 | 0.0012 |
| **r 0.05, λ 0.5, cap 0.10 (shipped crop_masked)** | crop_masked | 0.9927 | 0.9987 | 0.0002 | 0.0021 | 0.0012 |
| r 0, no λ, cap 0.35 | crop | 0.9928 | 0.9236 | 0.0137 | 0.3003 | 0.0012 |

Robustness:

- **Crop.** With r = 0, 12 of the 16 cells give the shipped result. The four that degrade are cap 0.20 without λ, and cap 0.35 without λ or with λ ≥ 0.5. The chosen cell is inside this plateau, not at its edge.
- **crop_masked.** With r = 0.05, cap 0.05 with any λ, and cap 0.10 with λ ≤ 0.5, give the shipped result. Cap 0.10 with λ = 1.0 or without λ degrades, so λ = 0.5 is one grid step from the plateau edge.
- **Crop with r = 0.05** keeps 10% of artifact pixels as data (recall 0.8975), which is why crop uses r = 0.
- **Residual 0.0012 at r = 0** is detector misses, not policy. It equals the mask-mode residual.

Shipped policy:

- **Crop:** `maximum_residual_artifact_fraction = 0`.
- **crop_masked:** `crop_masked_residual_fraction = 0.05`.
- **Shared:** `maximum_crop_sacrifice_fraction = 0.10`, `crop_exchange_rate = 0.5`, `crop_fallback = "bbox_mask"`.
- **Removed:** the component-count tier table is no longer shipped.

Structural reasons, independent of the grid:

1. **Crop residual must be zero.** Crop mode keeps retained flagged samples as data, so any positive residual ceiling contradicts "remove invalid areas" (A22). Zero is the only value consistent with the contract.
2. **crop_masked can tolerate residual flags.** It writes retained flags undefined, so a positive ceiling enlarges the rectangle without keeping artifact data.
3. **The exchange rate bounds precision by construction.** λ guarantees sacrificed valid pixels ≤ λ × flags removed, so per-image action-level removal precision is ≥ 1/(1+λ) = 2/3. The cap bounds per-image valid loss at 10%.
4. **Large caps are no longer needed.** With the `bbox_mask` fallback, a rejected rectangle no longer means "keep everything". The large tiered caps (35–47%) existed to avoid that outcome, and they caused most of the old policy's valid loss.

Held-out confirmation (seeds 5000–5007 float32, 7000–7003 int16; truth outside the input tree):

| Corpus | Mode | Recall [95% group-bootstrap] | Precision | Valid loss | Worst image | Actions |
| --- | --- | --- | --- | --- | ---: | --- |
| float32, 72 frames | mask | 0.9921 [0.9875, 0.9963] | 0.9984 [0.9977, 0.9989] | 0.0003 [0.0002, 0.0004] | 0.0025 | 48 mask, 24 keep |
| float32 | crop (shipped) | 0.9921 | 0.9849 [0.9792, 0.9898] | 0.0027 [0.0018, 0.0039] | 0.0048 | 24 crop, 16 bbox_mask, 8 mask, 24 keep |
| float32 | crop_masked (shipped) | 0.9921 | 0.9984 | 0.0003 | 0.0025 | 26 crop_masked, 16 bbox_mask, 6 mask, 24 keep |
| float32 | crop (previous policy) | 0.8495 [0.8354, 0.8630] | 0.8116 [0.7829, 0.8472] | 0.0350 [0.0271, 0.0420] | 0.0438 | 46 crop, 2 mask, 24 keep |
| int16, 36 frames | mask | 0.9973 [0.9953, 0.9989] | 0.9980 | 0.0003 | 0.0026 | 23 mask, 13 keep |
| int16 | crop (shipped) | 0.9973 | 0.9856 [0.9837, 0.9875] | 0.0022 [0.0019, 0.0026] | 0.0045 | 12 crop, 8 bbox_mask, 3 mask, 13 keep |
| int16 | crop_masked (shipped) | 0.9973 | 0.9980 | 0.0003 | 0.0026 | 12 crop_masked, 8 bbox_mask, 3 mask, 13 keep |
| int16 | crop (previous policy) | 0.8279 | 0.8456 | 0.0231 | 0.0314 | 23 crop, 13 keep |

The held-out results agree with development within the bootstrap intervals, so there is no sign of selection overfit. The independent Astropy checker verified every product. In crop mode, `bbox_mask` handles 22% of frames; those products contain undefined samples by design (decision D1).

### 0R.3 Plan changes

- **R1.6 out-of-model null and stress corpus (new).** `tools/make_out_of_model_nulls.py` generates cosmic rays (including hits on the outer pixels), satellite trails, saturated edge stars with bleed columns, interior hot columns and crowded edge-star fields. Scale each family to 2,995 frames for parity with G3a. Then add labelled positive-artifact families (corner glow, light leak) as *invalid* truth to measure K2.
- **R5.0 shadow comparison and enriched labelling (new; before G4 and before any yearly re-run).**
  - Run both detectors on a simple random sample of frames from each year (reports only).
  - Stratify frames into agree-keep, agree-act and disagree.
  - Label a simple random sample in each stratum with recorded inclusion probabilities π_h, oversampling the disagree stratum.
  - Estimate population metrics with Horvitz–Thompson weights 1/π_h. Enrichment raises power for detector differences without biasing population estimates.
  - Never report metrics from disagreement frames alone (selection bias).
  - Split by night group as in §3.
- **R5.2 production drift monitoring (new).** For each processed year, report the action distribution (keep, mask, crop, crop_masked, bbox_mask), the abstention rate, border protected-source pixels, uncertain-band fraction and median noise. Flag a year whose rate departs from the pooled rate by more than three binomial standard errors (Holm-adjusted across monitored quantities) for domain review. A flag is a review trigger, not an automatic change.
- **G3 split by action type (amended).**
  - The removal-precision target ≥ 99% applies to detector masks (mask and crop_masked modes).
  - Rectangular crop products sacrifice valid pixels by geometry. For them, register: pooled valid loss ≤ 0.5%, per-image valid loss ≤ the cap (by construction), per-image precision ≥ 1/(1+λ) (by construction) and pooled precision ≥ 0.98.
  - The held-out crop result (0.985/0.986, valid loss 0.0027/0.0022) meets these targets. Without the split, crop products could never meet the old single target, so the gate was ill-defined.
- **G5 (amended).** D5's detection-time regression is recorded as an accepted scientific trade-off. Every other speed-only change still has to be bit-identical and must not regress.
- **R5 (amended).** Promotion rules now govern the *release claim*, not the default; the rollback trigger above governs the default.
- **§5 rules.** Implemented decision keys: `maximum_residual_artifact_fraction` (crop), `crop_masked_residual_fraction`, `maximum_crop_sacrifice_fraction`, optional component-count table, `crop_exchange_rate`, `crop_fallback`. Rule files without the new keys keep their earlier meaning.

### 0R.4 Remaining open items, in dependency order

1. **R5.0 shadow sample and G4.** The tools are ready (`tools/shadow_compare.py`, `tools/drift_monitor.py`). Running them needs observatory data access and calibration provenance for `tools/flat_labels.py`. This blocks every release claim and the rollback decision.
2. **K2 positive extended artifacts.** Measured (v2 recall 0). Any rule is an opt-in experiment, gated on the nebulosity and edge-source nulls. The null part of R1.6 is done (0/2,995 per family).
3. **R6 numerical options (K4).** Exact work is done (about 2.5x faster end to end). Strided mesh subsampling and border-band evaluation are numerical changes that need the full harness. Weighted memory admission and the G5 workload set remain open.
4. **Yearly re-runs with v2,** after R5.0. The 2011 tree is incomplete and has to be recovered before it is re-processed.
5. **Small corners (K3)** after G4 strata are available. **PIXACT3** reason plane, deferred because reason planes are transient (built, verified and removed) and nothing consumes them; **R3.3 morphology** only if G4 shows holes. `fitsverify` checks are done (0 warnings, 0 errors).

### 0R.5 Review passes 15–19

| Pass | Result |
| --- | --- |
| 15 — default-promotion safety | Material: D5 removed the only guard against shipping an unvalidated default. Added K1–K6 and a rollback trigger registered before G4 data are seen. |
| 16 — statistical review | Material, three items. (a) Selecting the best of 64 grid cells inflates development performance (winner's curse), so the policy was confirmed on a held-out corpus evaluated once. (b) Held-out synthetic seeds are not independent of generator assumptions (K5), so R1.6 was added. (c) Enriching labels with disagreement frames biases estimates unless weighted, so R5.0 requires Horvitz–Thompson weights. |
| 17 — gates and full coverage | Material, two items. G3's single precision target is unattainable by construction for rectangular crops; split by action type. No production signal would detect domain drift after the default changed; R5.2 added. Re-checked A01–A31, D1–D5, the R-table and G1–G5. |
| 18 — full coverage after integration | Re-read §0R, §4A, §6, the audit addendum, README, configuration, user guide, algorithm, AGENTS, skills and memory against the shipped heuristics and code defaults. They agree, and no validation claim was introduced. No new material omission. |
| 19 — order and failure handling | Checked dependencies: R5.0 precedes G4 and yearly re-runs; R1.6 has no data dependency; the rollback trigger is fixed before data. Checked that each new risk has a signal and an owner item. No new material omission. |

**Revision-3 closure.** Passes 18 and 19 are two consecutive full passes without a material finding, which meets the §7 stop rule. Planning reopens on R5.0 or G4 results, a rollback-trigger event, a new failure, or a new observing domain. Further desk review is not expected to change safety, estimands, data requirements, method family, gates or order. Real data are the binding constraint.

## 0. Revision 2 — deep review of 2026-10-29

### 0.1 Reviewed baseline

- Branch `claude/eager-galileo-khb8fs`, head `71ec091` (merge of the GitHub history; Apache-2.0). Working tree clean at review start.
- Cargo package `0.1.0`. The base-100 build counter is machine-local: the user's host recorded release `0.2.74` for the latest validated crop run, while this fresh review container compiled `0.0.1` from the tracked `0.0.0` template. Neither identifies a scientific release.
- `cargo test --locked --all-targets`: 102 tests pass (69 unit, 29 end-to-end, 4 version). `cargo clippy --locked --all-targets -- -D warnings` and `cargo fmt --all --check` pass. Astropy is not installed in the review container, so the Python checkers were not rerun here.
- Read for this review: `AGENTS.md`, all nine project skills, README, the project memory, the 2026-10-27 audit, this plan, algorithm/validation records, the residual-tolerant, wrong-rectangle, multiscale, reference-model and yearly-remediation plans, and the detector, crop, pipeline, writer and configuration sources.

### 0.2 New evidence: deterministic synthetic probes on the shipped heuristics

Probes called `detect_border` directly with the unchanged `input/heuristics.toml` (tile 16, |z| ≥ 8, noise ratio 0.20, correlation 0.95) on 256×256 images. Sky is 1000 + N(0, 10²) from a fixed-seed LCG/Box–Muller generator. The probe source was temporary and has been removed from `src/`. The recipes are in the [audit addendum](scientific-audit-2026-10-27.md#revision-2-addendum-2026-10-29). These are counterexamples, not error-rate estimates.

| ID | Fixture (truth) | Observed decision | Consequence |
| --- | --- | --- | --- |
| A19 | Left 40 columns at level 300; σ=2 (quiet) vs σ=60 (flat-amplified noise) | σ=2: accepted, 8,192 removed, 2,048 artifacts kept (tile phase). σ=60: `no_border_candidate`, all 10,240 artifacts kept | Variance evidence is one-sided. Flat-divided vignetting in *calibrated* frames, the yearly production domain, is a plausible miss class. |
| A20 | Bright extended source (Gaussian σ=14 px, amplitude 3000) centred on the left edge; **no artifact** | Accepted; 2,816 valid pixels removed | Raw lag-one correlation of a smooth source profile satisfies the `correlated` branch. There is no source protection, so this is direct science loss. |
| A03 (now observed) | Smooth, still-illuminated edge vignetting, depth 400 or 800 ADU over 48 px; **no artifact** | Accepted; 15,616 (depth 400) and 28,672 (depth 800) valid pixels removed | Raw correlation responds to gradients, not to artifact structure. Removal is independent of the interior star in A18. |
| A18 | Identical border strip (level 700, σ=5, 48 columns) with and without an unrelated resolved interior star (amplitude 20,000, σ=8 px) 80 px away | Without the star: missed (12,288 artifacts kept). With the star: fully accepted | `central_tiles` uses the **maximum** interior tile σ as `typical_noise`. One interior source changes border decisions elsewhere, so decisions are not local and not reproducible across pointings. |
| A01 (re-measured) | Sharp zero strip of width 20 or 28 | 1,024 artifacts kept, or 1,024 valid removed | Tile-phase boundary error persists at the default tile size. |

Timing hint (release build, one 7100×5325 synthetic frame, 4-core container, single observation): `single_scale` detection took 1.68 s and `multiscale_consensus` 5.44 s. An O(n) `select_nth_unstable` median over all 147,852 16×16 tiles took 0.118 s. The current tile statistics allocate per tile and fully sort twice per tile (median and MAD). The same frame's mask overshoots a 300-column truth strip by 4 columns (1,618,800 vs 1,597,500 pixels).

### 0.3 New findings mapped to work

| ID | Severity | Evidence and consequence | Response (§4A item) |
| --- | --- | --- | --- |
| A18 | high | Observed: decision depends on unrelated interior sources through the max-σ quiet reference | R2.1 pixel-level noise map; median, not maximum, reference |
| A19 | high | Observed: high-variance (flat-amplified) artifacts are never candidates | R2.2 two-sided log-dispersion evidence |
| A20 | high | Observed: edge sources and galaxies are removed through raw correlation; no source protection | R2.3 source mask and protection constraint; R2.4 detrended correlation as corroboration only |
| A21 | high | Inspection: when the crop sacrifice cap fails, the source is kept unchanged, so 0% of the accepted artifacts are removed although mask mode would remove all of them at zero valid loss. This is a Pareto-dominated outcome | R4.3 explicit fallback policy (user decision D1) |
| A22 | medium | Inspection: a tolerant crop retains detector-flagged pixels as ordinary data (PIXACT2 code 4), not masked | R4.4 opt-in `crop_masked` hybrid (user decision D2) |
| A23 | medium | Inspection: the residual search covers only expansions that contain the strict anchor, so the global optimum can lie outside the family | R4.2 exact coarse-to-fine search with a stated gap bound |
| A24 | medium | Inspection: utility multiplies area fraction by a clipped z-ratio, which is dimensionally meaningless. Sacrifice tiers by border-component count have no relation to valid-loss acceptability | R4.1 explicit exchange-rate policy λ plus hard caps |
| A25 | medium | Inspection: `maximum_mask_fraction = 0.50` vetoes frames whose usable field is under half the array, keeping artifact-dominated images unchanged | R4.5 support-based abstention instead of a fraction veto |
| A26 | medium | Measured hint: full sorts and per-tile allocation dominate detection; multiscale is about 3.2× slower; no intra-image parallelism for 37.8-Mpx frames | R0.2–R0.4, R6 |
| A27 | low | Inspection: documentation drift. The `config.toml` comment mentioned "rectangle/circle" and 32 B/px; README/AGENTS described strictly clean crops; `algorithm.md` said WCS limitations keep the original; this plan cited removed manifests | Fixed in this revision (R0.6) |
| A28 | high (evaluation) | Inspection: no labels exist, but the yearly tree links master `flat/` frames whose pixel response independently measures illumination. They are unused | R1.3 flat-derived illumination reference labels, evaluation-only by default (user decision D3) |
| A29 | high (evaluation) | Inspection: no false-alarm control. Thresholds are hand-set, and there is no null-frame false-acceptance rate | R1.2 null false-alarm calibration; thresholds derived from FAR targets |
| A30 | medium | Inspection: "valid" is undefined for partially illuminated, S/N-bearing pixels, so the probes in A03 cannot be scored without a declared label rule | R1.1 label definition (user decision D4) |
| A31 | medium | Inspection: every processed image writes, fsyncs, publishes and re-verifies a full-frame mask FITS sidecar that is then deleted. This is pure I/O on network storage (`/mnt/uxmal_groups`) | R0.5 verify the action mask in memory; write sidecars only on request |

### 0.4 Decisions required from the user

These change product contracts or label semantics, so they are not taken unilaterally. Until answered, the current behavior stays.

- **D1 — crop fallback** when no rectangle passes the sacrifice/utility gates: `keep` (current contract), `mask` (dimension-preserving exclusion of accepted artifacts), or `bbox_mask` (crop to the tight bounding box of retained support, mask the rest). Recommended: `bbox_mask`, opt-in first. It removes all accepted artifacts, loses no valid pixels, and reduces storage.
- **D2 — internal undefined samples in crop products.** Allow an opt-in `crop_masked` product: a tolerant rectangle whose retained flagged samples are set to BLANK/NaN. Recommended: yes, as an opt-in mode. The current `crop` stays fully defined.
- **D3 — master flats.** Use them as independent evaluation labels only (recommended), or also as an opt-in detector prior (a contract change: pixel evidence from a second image).
- **D4 — label rule for partial illumination.** Recommended: `I` where relative illumination < 0.20; `V` where ≥ 0.50; `Q` (uncertain) between. Freeze the values per science use before tuning.

Answered 2026-10-29: D1 `bbox_mask`, D2 yes, D3 evaluation labels only, D4 as recommended. Revision 3 adds **D5** (user decision): v2 is the default detector before G4 (§0R.1).


## 1. Objective, scope and non-negotiable contracts

Maximize **valid science pixels retained** and **truly invalid border pixels removed**, while bounding source loss, residual contamination, statistical uncertainty, runtime and memory. These objectives compete when the boundary is uncertain or a consumer requires a simple crop. Increasing the number of pixels labeled invalid is not evidence of improvement.

Keep Rust and pure-Rust FITS I/O. Use established Rust crates when they meet numerical, licensing, maintenance and concurrency needs; assess any new dependency at implementation time and pin it in Cargo.lock. Python/Astropy/Photutils remain independent research/validation tools, not required production dependencies. Avoid introducing a new framework for algorithms already expressible in small, testable modules.

Preserve the user's current contracts: pixel-derived classification; no descriptive-header detector features; single-HDU 2D processing; ignore compressed/multi-HDU inputs; input configuration under `input/`; documentation under `input/doc/`; preserved inputs and kept-image symlinks; unique sanitized output runs; ISO logging; provenance and checksums; English; bounded parallelism; coherent local Git commits. Crop products are fully rectangular: the strict anchor is the global maximum-area clean axis-aligned rectangle over detector-proposed and originally undefined samples, without a circle constraint; the shipped provisional `maximum_residual_artifact_fraction = 0.05` may then enlarge it within a bounded expansion family that retains detector-flagged (never originally undefined) samples. Verify `NAXIS=2`, `NAXIS1/2`, crop origin and supported `CRPIX` offsets. Metadata may be used for FITS decoding, WCS preservation and evaluation grouping, never as an instrument/object shortcut for classification. Dataset-specific policy profiles, if needed, must be explicitly selected by configuration, not auto-selected from headers.

Mask remains the default. Explicit crop mode writes an axis-aligned rectangle that contains no originally undefined sample and at most the configured residual fraction of detector-flagged samples (zero for legacy rule files), and retains the current keep-on-low-utility rule. It may sacrifice more defined pixels than a circular crop to avoid undefined pixels in the rectangular result. Any future `auto` policy or nonrectangular action needs an explicit format contract, validation plan and opt-in feature. No resampling, inpainting, intensity correction, deconvolution or inferred flux replacement is included. Smooth vignetting, nebulosity, faint sources and correlated sky are not automatically invalid.

## 2. Precise quantities and scientific loss

For an input pixel domain Ω, distinguish:

- `U0`: originally undefined samples from valid BLANK/NaN/non-finite handling.
- `D = Ω \ U0`: initially defined samples. Partially illuminated or uncertain samples are not silently added to U0.
- On annotated evaluation data only, disjoint `V` (usable science), `I` (instrumentally invalid border), and `Q` (uncertain label), partition D. Two independent annotators and adjudication define these labels; record boundary uncertainty and intended science use.
- `C`: proposed artifact mask; `A`: accepted artifact mask; `E`: additional pixels excluded solely by crop geometry. Keep C, A, E and U0 separate. Define E outside A to avoid double counting, and report their intersections with U0 separately.
- `M = (A ∪ E) ∩ D`: newly excluded defined pixels; `K = D \ M`: retained defined pixels. A stored FITS bounding box is not the same as retained science area.

Ground-truth metrics: removal recall `|M∩I|/|I|`, removal precision `|M∩I|/|M∩(V∪I)|`, valid loss `|M∩V|/|V|`, valid retention `1-valid_loss`, residual contamination `|K∩I|/|K∩(V∪I)|`, plus absolute areas, boundary distances and source-weighted loss. Report undefined denominators as not applicable, never perfect scores. Report Q fraction and best/worst-case label bounds; do not hide hard frames by excluding their uncertain area. Distinguish detector-only metrics on A from end-product metrics on M.

On unlabeled inputs use names such as `suspected_artifact_pixels` and `unclassified_pixels_sacrificed`. Do not label these ground-truth counts. The current evidence ratio is neither a probability nor a calibrated loss model.

For action selection, consider keeping, accepted freeform masks, and the permitted crop family. If calibrated probabilities `p_i=P(invalid | pixel evidence)` become demonstrably reliable, candidate utility may be evaluated as `Σ(i∈M) [b_i p_i − c_i(1−p_i)]`, relative to keep utility zero. Costs and benefits have declared units, positive valid-loss cost, and an instrument-independent default policy selected on development data. Protected sources impose constraints, not just small additive penalties. Before calibrated probabilities exist, use labeled-development risk estimates and validated confidence categories; never insert a clipped Z-score into this expression as p_i.

Select from the nondominated set: an action is dominated if another removes at least as much invalid area, retains at least as much valid area, and is strictly better in one, under the same constraints and uncertainty model. Apply hard safety limits first, then a frozen policy for the remaining tradeoff. Report alternative actions and reasons. Current crop outputs select the global maximum-area rectangle over the complete combined exclusion mask; fitted circles or other shape hypotheses cannot constrain this search. The current provisional configurable cap is selected by accepted contiguous border-connected area count (35%, 35%, 35%, 47% for one, two, three, four-or-more components); this guard is not calibrated and requires labeled-data evaluation. Keep the source when no positive benefit is established; otherwise break equivalent-utility ties by lower worst-case valid loss and greater retained defined area. With the same trusted artifact mask and no geometric requirement, freeform masking retains at least as much usable area as a clean rectangle: include it as a reference bound even in explicit crop mode.

## 3. Data, labels and evaluation protocol — before tuning

1. Treat the repeatedly inspected 25 supplied frames, including `science#2007_01_11T20_23_31...`, as development/regression material. The known circle result is a case study, not a mandated answer for every four-corner frame. New boundaries must be evaluated on their pixels and labels.
2. Build independent analytic fixtures and image simulations with known V/I/U0, seeds and generator versions. Vary boundaries independently of tile origin and use smooth/blurred transitions, stochastic Poisson/read noise, correlated noise, quantization, gradients, PSFs, crowding, saturation, trails and faint edge sources. Separate hard-boundary loss from ambiguous illumination transitions. Simulation assumptions and units must be explicit; source injection does not substitute for actual instrument artifacts.
3. Acquire licensed, representative real images across detector/instrument, night, observing sequence, filter, exposure/noise and calibration state. Use metadata only for dataset grouping and expert assessment. Human labels must reflect whether information is scientifically usable, not simply match the old detector.
4. Split by observing sequence/night/instrument before feature or threshold design. Near-duplicates, repeated exposures of the same artifact field and augmented siblings stay in one partition. Use development, calibration and untouched test groups; add leave-one-instrument-out evaluation for any cross-instrument claim. Lock hashes, grouping rules, seeds, annotator agreement, sampling weights and software versions.
5. Record a prevalence-representative set plus an explicitly enriched difficult-case set. Report both; enriched-case precision cannot be presented as deployment precision without prevalence adjustment. Include successful, abstained, ignored and failed frames in accounting.
6. Freeze the complete search procedure, hyperparameter budget, primary metrics, strata, confidence method, significance level and practical effect sizes before test access. Select parameters by grouped nested validation on development data. Test once per registered candidate release; a result used to revise the method becomes development evidence and requires a fresh hold-out.
7. Quantify uncertainty across independent observing groups. Use group resampling for corpus metrics and justified spatial blocks for within-frame uncertainty; never count millions of neighboring pixels as independent trials. Diagnose residual correlation length, test block-size sensitivity and check empirical interval coverage in simulations. Small numbers of independent groups mean insufficient evidence, even with many pixels.

No fabricated labels, effect sizes or invented dataset access. P0–P6 engineering and synthetic work can proceed while real labels are acquired; release gate G4 remains open until real evidence is available.

## 4. Ordered phases, deliverables and dependencies

| Phase | Deliverable | Completion gate | Findings |
| --- | --- | --- | --- |
| P0 — reproducible baseline | Freeze source/config/binary identities, truth fixtures, stage timers and baseline reports; reconcile docs | Every known probe reproducible; timing/memory and correctness baselines versioned | A01–A17 |
| P1 — correctness and contracts | Numerical/sentinel/compression/header/WCS/report/publication fixes and focused regressions | G1–G2; no known high-severity correctness failure left | A07–A11, A14–A16 |
| P2 — reference and source model | Source-aware, locally adaptive background/noise estimation with support/uncertainty maps | Null/confounder, numerical and source-injection gates on development data | A02, A08 |
| P3 — spatial evidence | Multiscale residual features, labeled components and calibration of complete selection | Component-level diagnostics, domain limits and measured discrimination | A02–A04 |
| P4 — refined masks | Pixel boundary refinement, uncertainty band and separately encoded mask reasons | Boundary and source-preservation gates; no tile-alignment artifact | A01, A04, A14 |
| P5 — action/crop optimizer | Common loss policy, constrained candidate set, explicit geometric optimality domain | Area truth metrics, oracle checks, risk/coverage curves and action comparisons | A05–A06, A14 |
| P6 — measured efficiency | Shared owned input, fewer allocations/reads, bounded scheduling and focused modules | G5; unchanged numerical/scientific decisions or separately justified algorithm change | A12–A13, A15, A17 |
| P7 — external validation and release | Frozen configuration, independent FITS/WCS/science evaluation, release manifest and updated guides/skills/memory | All applicable G1–G5 pass in declared domain; otherwise research-only status | all |

### Implementation status (2026-10-27)

- **P1 — partial:** centered raw integer/float analysis values; lazy integer sentinel selection; keep-source fallback when an integer action has no unused BLANK code; precision abstention for integer spans beyond exact f64 representation; exclusion of undefined pixels from detector masks and utility denominators; original undefined/proposed/applied counts; gzip signature exclusion; stale DATAMIN/DATAMAX removal; dimensionless mask units; geometry-specific bounded HISTORY; explicit dry-run state; crop abstention for lookup-table/distortion WCS conventions; schema 8 with total/published-image counts, stable decision reason codes and optional PIXACT1 reason-plane output path, candidate/proposed/applied artifact counts, C/A/E/U0 intersections, algorithm/policy IDs, Git revision/dirty state and executable/configuration hashes. The parsed FITS source is carried through detection, writing and sample verification, with a path-replacement regression. SHA-256 of parsed source bytes is recorded in per-image reports and version-3 completion manifests; each manifest product also records a content SHA-256. Detectable size/mtime changes during parsing fail the input. Per-file completion manifests were implemented and later removed at the user's request (2026-10-28); schema-11 summaries, CSV and the activity log are now the run records, and the manifest statements in this historical status entry are superseded. Stage-specific output-path, collision, cleanup, product-write, symlink, preview and manifest error codes are implemented. Science and mask products are now built and verified in sibling staging directories before final publication; staged bytes and final parent-directory entries are synced on Unix, ordinary second-rename failures restore the previous pair or remove newly published partials, and overwritten completion markers are removed with a directory sync first. The completion manifest remains authoritative because a process or power failure between final renames is not transactionally recoverable. The independent checker samples finite forward transforms on a 9×9 grid for primary and alternate WCS on crop outputs, including zero-origin crops, uses spherical angular error for celestial axes, and checks inverse residuals when Astropy converges. Synthetic alternate-WCS and non-finite controls are covered. It also compares preserved source-card values/comments against a documented per-product allowlist, rejects unexpected metadata cards, checks dimensionless masks and validates reason-plane codes against the science action mask and original undefined pixels. This audit passed on the requested 25-file development dataset. Broader metadata-card fixtures, full supported-convention/WCS output coverage, process/power-loss recovery tests and the complete structural-card policy remain. Directory fsync is best-effort and unavailable through this standard-library helper on non-Unix platforms; it is not a cross-file transaction or hardware durability guarantee. The source digest covers eligible parsed single-HDU files; malformed and ignored pass-through symlinks are content-hashed in their product manifests, while their parser-level source digest is null.
- **P4 diagnostic slice — engineering complete, science effect none:** optional `--reason-mask` writes a verified `PIXACT1` reason-code FITS plane for each accepted mask/crop action and records its path/hash in schema-7 reports/manifests. The codes distinguish retained defined samples, detector-selected exclusions, crop-only geometric sacrifices, and input undefined samples. They are operational bookkeeping, not labels or calibrated probabilities. Reason-plane FITS output has Rust writer verification, regression tests, and Astropy/fitsverify interoperability on the requested 25-image development dataset; see `validation.md`. A five-pair optional-feature cost comparison was too noisy to establish a resource change. This does not implement pixel-boundary inference or close independent-data calibration.
- **P0/P6 — measured partial:** release `0.1.1` ran on the supplied 25-image development set in crop mode without PNG. One declared warm-up preceded 10 randomized measured trials at each of one and four workers. Median wall times were 16.22 s (range 14.26–44.17) and 4.495 s (3.89–10.68); median peak RSS was 312,116 KiB (311,616–312,276) and 945,900 KiB (768,488–1,028,208). All 22 runs produced the same decision digest (`402f1ff5…`) and counts: 16 clipped, 4 kept, 5 malformed-header errors. The large timing tails and one host/repeatedly inspected corpus limit inference. This compares worker settings, not code revisions; it is neither an algorithm speedup nor proof of a hard memory bound. Cold/warm separation and broader workloads remain; G5 remains open. Full evidence and commands: [validation record](validation.md).
- **P2 — partial:** an opt-in robust affine plane fit over central interior tile medians is implemented and tested on synthetic plane/null and seam cases. `central_tiles` remains the default; no compact-source exclusion, uncertainty map, local mesh support, or independent calibration exists. This is an experimental reference candidate, not a source-aware or science-grade model.
- **P3 — partial:** edge-connected tile components are independently gated by tile support, area, evidence and provisional utility; reports expose candidate/accepted component counts. Lag-one correlation uses rescaled Welford moments without pair buffers. An opt-in up-to-three-width consensus now requires two-scale pixel support, covers a non-aligned boundary fixture and reapplies final image area/utility gates. It does not use shifted origins, directional diagnostics, spatial dependence calibration, source protection or independent science labels; it is experimental and adds up to three detector passes.
- **P4–P5 — partial:** the crop action now offers an explicit residual detector-flag fraction with a 100,000-candidate bounded expansion family around the exact maximum-clean rectangle; schema 11 and PIXACT2 code 4 report retained flags. Release 0.2.74 passed synthetic gates and independent FITS/preview validation on four repeatedly reviewed examples (run `20261029T080236.828463976Z`). The detector flags and 5% shipped ceiling are uncalibrated, the candidate family is not globally optimal, and these frames are not held-out validation. Pixel-level boundary inference, uncertainty, a common calibrated action objective and freeform reference action remain open; see [residual-tolerant crop plan](residual-tolerant-rectangle-plan.md).
- **P6 — partial:** correlation pair-buffer allocations were removed, source-path rereads were eliminated from generated-product writing/verification, small-grid exhaustive geometry oracles were added, and source hashing happens during FITS parsing with bounded memory, including forward-skipped FITS padding. Completion markers now hash all generated products and kept symlinks; this adds a bounded-memory output read. One current four-worker observation took 3.88 s / 950,508 KiB versus 3.86 s / 973,060 KiB in the preceding release, which is not a controlled comparison. No paired measurement of product-hash overhead, stage profile, or hard memory cap exists and G5 remains open.
- **P7 — open:** release `0.1.33` products passed independent Astropy/fitsverify checks for 20 science images, 16 masks and 16 reason planes from the repeatedly inspected development set; four unchanged symlinked images were also inspected. The checker now validates preserved metadata cards and reason-code/action-mask consistency, in addition to FITS checksums, samples, geometry and tested WCS behavior. This does not validate scientific correctness, all supported WCS conventions or an untouched holdout. No independent science labels or held-out data are available.

P0 precedes tuning. P1 and the data protocol precede promotion of new algorithms. P2→P3→P4→P5 supplies scientific stages. P6 profiling begins at P0; exact allocation/I/O improvements can be developed after P1 while scientific work proceeds. Benchmark and validate the final combined pipeline again at P7. Each phase ends with a scoped commit and evidence table; do not silently mark research targets complete after a unit test passes.

### P1. Correctness fixes required before optimization

- Preserve raw samples and use centered, scale-aware analysis values. Avoid reintroducing a large BZERO into residual arithmetic; keep the physical conversion as metadata. Check finite arithmetic, overflow, signed/negative BSCALE, tiny scales, wide int64 spans and representability loss. Adopt a documented precision error bound or abstain when distinctions cannot be represented. Use stable midpoint and covariance calculations, not uncontrolled raw sums.
- Handle existing BLANK lazily; do not allocate a new sentinel for keeping, dry runs or rectangular crops that introduce no new undefined samples. Choose a code absent from **retained defined** samples after the final mask is known. Never change existing undefined interpretation or silently convert int64 to float. If no sentinel is possible for the chosen action, report an explicit representation failure and keep the source under a documented fallback policy.
- Detect compression signatures and FITS tiled-compression metadata solely for eligibility. Ignore compressed/multi-HDU inputs without unpacking payloads. Malformed unrecognized data remain errors; do not claim universal format detection from filename suffixes.
- Define allowed science-card changes and separate dimensionless mask-card policy. Recompute/remove stale extrema, preserve original observational metadata and commentary, record actual geometry, and split long provenance text across valid cards. Audit all other parsed cards; document unsupported conventions and serialization limits.
- Build a crop WCS support matrix: standard linear/celestial and tested SIP/alternate conventions first; report unsupported distortion/lookup references and use a dimension-preserving mask fallback for accepted pixel detections rather than passing the frame through unchanged. Header-driven output eligibility must not affect pixel candidate classification. WCS-absent images remain eligible under a documented no-WCS contract. Verify finite equivalent coordinates on retained points, including alternate solutions and zero-origin crops. Never use matching NaNs as evidence of equivalence.
- Version report semantics. Separate candidate, proposed action, applied action, original-invalid and geometry-loss counts; record dry-run, review/abstention, unsupported-WCS, source identity/digest, schema/algorithm/policy version and effective config hashes. Kept/dry-run actions have zero applied removal even when proposals are nonzero.
- Publish only after staged pixel/header/checksum verification. Per-input completion manifests are no longer produced (user decision, 2026-10-28); interrupted runs are recognized by the absence of a final summary, and this limitation must stay documented. Multiple filesystem renames are not a transactional guarantee. Test interrupted/partial writes, permissions, disk-full behavior, source replacement between reads and preview failure; retain already valid products with explicit state when appropriate.
- Keep the requested base-100 build counter, but distinguish Cargo package version, local build number, Git revision/dirty state, algorithm/schema version and executable/config hashes. Define counter behavior after failed builds, stale locks and concurrent target directories; local build numbers alone are not reproducible science identifiers.

### P2–P3. Detection methods to evaluate

**Reference and source model.** Start with masked robust local meshes and a low-order background surface in analysis space. Exclude U0 and high-confidence pixel-derived compact sources/saturated cores from estimator fits, not from the science image. Use multiple spatially distributed reference patches and robust consensus; the center has no privileged validity. Reject insufficient finite coverage, unsupported interpolation and disagreement between independent reference regions. Preserve uncertainty where usable interior support cannot be established. Source detection iterations are capped; threshold changes and stopping reasons are logged.

Estimate local dispersion from actual residual pixel distributions, using MAD and a robust alternative on quantized/zero-MAD samples. A scale floor must reflect representable precision and measured noise/support, not only machine epsilon times a global offset. The MAD normalization is a Gaussian consistency convention, not proof of Gaussian noise. Fit nuisance background gradients separately from sharp artifact boundaries, and test whether the fit absorbs real borders or erases diffuse science. Validate this stage against independent reference estimators on controlled fields; exact agreement with a reference package is not required when models differ. [Photutils background documentation](https://photutils.readthedocs.io/en/stable/user_guide/background.html) motivates source-masked local estimation; the chosen estimator still requires project-specific evaluation.

**First implementable estimator, before alternative research.** Use source-excluded finite samples in overlapping meshes to compute exact median and scaled MAD, with explicit valid fraction/count. Fit a robust local affine background to eligible mesh summaries using bounded Huber reweighting in centered coordinates; restrict interpolation to regions with adequate neighboring support. Use the robust median of eligible residual-noise estimates, with local adaptation, as the noise reference rather than its maximum. Refit at most a configured number of times while excluding high-confidence border seeds and source footprints; if exclusion changes the reference substantially or support collapses, keep/review. Compare this first model with a source-masked median-mesh interpolant in a registered ablation. Do not add higher-order surfaces unless they improve null controls without absorbing injected artifacts.

Use a numerically conditioned solve and reject rank-deficient reference layouts. [Statsmodels' Huber weight definition](https://www.statsmodels.org/stable/generated/statsmodels.robust.norms.HuberT.html) provides an independent numerical reference for the bounded-iteration fit; it supplies no accuracy guarantee for astronomical border classification. The robust-fit tuning constant and convergence tolerance are registered science/numerical parameters, not unexplained defaults.

Separate **effect size** from **significance**: for a candidate/reference pair define `d = |median(residual_candidate) − median(residual_reference)| / max(s_reference, s_floor)`. Record noise ratio and directional residual correlations separately; d is not a Z-test statistic. Estimate uncertainty in the median difference by the registered spatial/group procedure, not by dividing an assumed pixel sigma by the square root of total region area. For a boundary profile, compare predictive robust loss under a shared smooth-background model and a two-region level/variance-change model, using spatial validation blocks and identical source exclusions. Evaluate the added boundary parameters, scanned locations and scale selection within the full calibration procedure. These definitions make the initial method executable while leaving thresholds to measured development evidence.

**Multiscale proposals.** Evaluate a small frozen family of physical pixel window sizes, with overlap/shifted origins and border strips. Record medians, robust dispersion, valid fractions, residual gradients, cross-boundary contrast and horizontal/vertical correlations at selected lags. Record pair counts and uncertainty for each direction. Compare abrupt-change alternatives against smooth-background and no-artifact models. Use ratios/log ratios with explicit zero-variance handling; avoid adding differently scaled evidence terms without calibration. Correlation alone never authorizes clipping.

**Components.** Seed each edge and corner independently, label connected regions and keep component identities stable. Specify 4/8 connectivity and test diagonal contact; default to the validated topology, not whichever yields the largest mask. Grow only through locally supported pixels; require each component to clear area, valid-support and boundary-evidence gates. Do not count unrelated components together to pass a minimum-size rule. Test persistence across scales and mild perturbations without calling that persistence a probability. Large components must not conceal weak subregions or bridges across protected sources.

**Statistical calibration.** Begin with interpretable, low-dimensional feature rules and compare a simple calibrated classifier only if labeled data support it. Fit/tune on grouped development data; calibrate on separate groups; inspect reliability, false removals, recall and out-of-domain behavior. If reporting p-values, simulate/resample the complete proposal, scale-selection and boundary-refinement procedure under justified null models; selection on the same data invalidates a naive fixed-region test. Correct the registered search family across regions/scales, and demonstrate empirical calibration under spatial correlation. Otherwise publish diagnostic evidence scores and empirical risk curves without p-value/FDR claims. [Lahiri and Zhu](https://projecteuclid.org/journals/annals-of-statistics/volume-34/issue-4/Resampling-methods-for-spatial-regression-models-under-a-class-of/10.1214/009053606000000551.pdf) illustrates that spatial resampling validity depends on sampling assumptions; it is not a ready-made guarantee for this detector.

**Model limits.** A single frame can contain astrophysical and instrumental structures with indistinguishable pixel statistics. Require keep/review for that ambiguity or for unsupported data domains. Region darkness, smoothness, symmetry, source absence and high correlation are evidence features only. No unlabeled score establishes that a faint signal is scientifically useless.

### P4. Boundary refinement and masks

Refine only a bounded band around candidate boundaries at native resolution using local change profiles or constrained region growth. Estimate the width/location of the transition and a boundary uncertainty band. Extend removal only where evidence supports it. Pixel counts and decisions must be stable against tile-grid shifts within declared boundary uncertainty. Do not interpolate the science pixels or upscale a binary tile mask as if it were a measurement.

Test a constrained graph/contour method only if simpler profile/growth refinement misses the predeclared practical gain. A geometry fit to a circle, ellipse or rectangle can propose boundaries but cannot override contradictory pixel evidence. Ellipses, rotated or mixed borders first belong in segmentation/validation; exporting extra crop shapes is a separate bounded extension, not a prerequisite for preserving them with masks.

Morphology is disabled by default until evaluated. Every fill/dilation/erosion has a finite radius in pixels, a declared purpose and a loss budget; it cannot bridge a source or delete a valid island merely to smooth the mask. Represent source-protected, original-invalid, accepted-artifact, uncertain-boundary and geometry-sacrifice reasons separately, with a documented precedence/overlap table. Keep an ordinary binary exclusion product for existing consumers; a versioned reason plane can be another single-HDU FITS under `masks/`, with unique suffix and report link. Proposal/reason diagnostics for abstained cases are optional, bounded outputs; keep output-directory organization unchanged.

Evaluate whole-image masks and component masks. A weak component may remain uncertain while an independently supported component is accepted; reports must show the residual uncertainty and actual action. Do not hide an unreviewed residual region behind a simple “clipped successfully” scientific claim.

### P5. Retained-area and crop decisions

1. Generate C and component evidence before applying either mask-mode or crop-mode utility. Make geometry candidates from the frozen accepted/uncertain-mask policy, not from a different unrecorded threshold for each shape.
2. Count defined, invalid and unknown pixels separately. Exclude U0 from science area scores. Pre-existing bad pixels need not force destruction of a large otherwise useful region; decide whether the consumer requires a fully defined crop or permits internal masks and encode that choice explicitly.
3. Keep the O(width×height) exact maximal axis-aligned rectangle as a reference, verified by exhaustive small-grid enumeration. State that it is optimal only for its binary admissibility mask and rectangle family. A weighted-valid-area rectangle is a different problem; do not apply the existing proof to it.
4. Keep the linear-time squared Euclidean distance transform for fast circle proposals, with reusable scratch buffers and exact small-grid reference checks. The [Felzenszwalb–Huttenlocher algorithm](https://theoryofcomputing.org/articles/v008a019/) establishes the transform's complexity; it does not prove global optimality of this application's subsequent circle policy. Define pixel-center versus whole-pixel-cell containment consistently. If subpixel centers are searched, bound center-grid resolution and clearance error; compare against exhaustive small cases and report any remaining optimality gap. Use integer-center geometry as the deterministic baseline.
5. Compare the global maximum clean rectangle with keep using the same loss accounting. A circle may be evaluated as detector evidence, but it cannot constrain rectangle geometry. For an explicit crop request choose that rectangle or keep; do not silently switch product mode. A future explicit auto policy can select masking when simple geometry sacrifices too much. A tight bounding-box plus interior freeform mask may be evaluated as an opt-in crop family because it retains more samples without resampling; report storage size separately from valid area.
6. Construct a bounded candidate Pareto frontier when allowing controlled residual artifacts or partial component removal. Require a registered residual-contamination limit; relaxing it merely to win area is prohibited. Start with zero accepted-artifact leakage for existing clean-crop semantics. Prefer exact algorithms for a restricted family over an unbounded shape search. Every approximate search has a computation budget, tie rule, feasibility check and gap estimate where possible.
7. Protect detected science apertures/trails with an explicit local loss limit and record intersections. Unknown faint sources remain part of corpus/injection risk assessment; protecting detected bright stars alone is insufficient. If no feasible action has demonstrated benefit within loss constraints, keep the source and retain diagnostics.

### P6. Performance, memory and maintainable implementation

Measure stage time, bytes read/written, allocation/peak-live-buffer estimates, queue delay, CPU utilization and process RSS. Separate preflight, decode, reference fit, features, refinement, geometry, writing/checksums, verification and PNG. Compare mask/crop with and without PNG, single versus multiple workers, small/mixed/large frames and every storage family. Estimate work from dimensions and chosen stages before memory admission.

Prioritized experiments, each compared with the same correctness baseline:

| Candidate | Expected mechanism; not a promised gain | Acceptance constraint |
| --- | --- | --- |
| Owned decoded frame plus source identity | Reuse headers/raw samples for writing and verification instead of reparsing the source repeatedly | Stable source snapshot/identity, exact retained samples, bounded ownership |
| Exact selection / small-integer histograms | Avoid full sorts for median/anchor/BLANK search where valid | Same specified median/MAD and sentinel semantics, overflow-safe midpoint |
| Reusable tile scratch and stable online covariance | Remove repeated sample/pair vector allocations | Correlation/reference agreement against a stable two-pass oracle within frozen tolerance; identical threshold decisions or separately evaluated change |
| Reuse EDT and row-stack buffers | Reduce two full distance arrays if lifetimes allow, and per-row/column scratch allocations | Exact distances and crop feasibility; no global-optimality claim beyond the proven domain |
| Single mask ownership and on-demand geometry scans | Avoid duplicate full-frame exclusion buffers | Identical final masks/counts, optional bit packing only if profiles justify conversion cost |
| Stream/buffer FITS serialization and checksums | Reduce clones and serialized copies while keeping verified staging | Independent full-HDU checksum, header and raw-pixel equivalence |
| Two-pass finite ZScale sampling | Count then collect the same strided samples without cloning every finite value | Exact sampling order and matching reference PNGs |
| Weighted memory admission | Use per-job peak stage estimates plus shared overhead reserve, instead of only maximum-image slots | Account for all live jobs, verification/PNG buffers and nested stage concurrency; no deadlocks or unbounded queue |

Use one bounded work pool. Parallelize independent images first; for a few large images, evaluate chunk-level work only under the same global CPU/memory budget. Fix reduction order and per-image seeds; remove scheduling-dependent scientific decisions. Separate I/O saturation from CPU limits. SIMD or reduced precision is a later experiment requiring measured benefit and new numerical evidence; do not enable fast-math to win a benchmark.

Target module boundaries follow ownership: image/preflight/normalization, reference/features/components/boundary/decision, mask/crop geometry, writer/header/checksum/verification, orchestrator/memory/report. Split the current long writer and pipeline during relevant changes, keeping substantial structs with their implementation and normally fewer than about 300 lines per production file. Add a small library surface only where it enables direct oracle/property tests; avoid a framework rewrite.

## 4A. Revision-2 technical roadmap (R0–R6)

The R-series reorders P0–P7 into implementable slices with explicit estimators, equations and gates. The P-phases and gates G1–G5 remain the release contract; each R item names the P-phase it advances. Every science-changing item first ships under a new opt-in algorithm identity (`border-detector-v2-…-experimental`) and is promoted to default only through §6 and §7. Speed-only items must give **bit-identical** masks, decisions and products on the frozen fixture and corpus set.

### R0 — speed and contract hygiene without science change (P0/P6)

| Item | Change | Equivalence / acceptance |
| --- | --- | --- |
| R0.1 | Stage timers and per-stage live-buffer estimates in the activity log: decode, statistics, reference, components, geometry, write, verify, preview | Overhead < 1% of median wall time; schema documented |
| R0.2 | Exact median/MAD by `select_nth_unstable_by(total_cmp)` on reusable per-worker scratch. Two-element even median uses the same `a·0.5 + b·0.5` midpoint. Exact histogram median/MAD for integer data with span ≤ 2^16 | Identical f64 values to the sorting implementation on property tests (random, ties, NaN-excluded, even/odd n) and identical corpus decisions. Target ≥ 30% detector-stage reduction on 37.8-Mpx frames; registered only after measurement |
| R0.3 | Tile statistics and correlation computed per tile row in parallel inside the existing bounded pool, with ordered collection | Per-tile results are independent, so outputs are bit-identical for 1/2/4 workers; nested parallelism is counted in the memory admission |
| R0.4 | Multiscale reuses base-scale tiles. Scales are computed in one traversal with shared scratch | Bit-identical consensus masks |
| R0.5 | Verify the binary action mask and reason plane **in memory** against the final science array. Write mask/reason FITS sidecars only when `--reason-mask` (or a future mask product) is requested | Identical science products; one fewer full-frame write + fsync + read per processed image; independent Python checker unchanged |
| R0.6 | Documentation drift fixes (A27) | Done in this revision for config comment, README, algorithm guide and this plan |

### R1 — evaluation infrastructure that needs no hand labels (P0/P7 prerequisites)

**R1.1 Label semantics.** Freeze `V/I/Q` rules per science use (D4). Relative illumination `η = F/median(F_trusted)` from a matched master flat `F` gives `I = {η < η_I} ∪ U0_F`, `V = {η ≥ η_V}` and `Q` otherwise. Non-illumination artifacts (overscan, bleed at edge, readout glow, occulting structure differing from the flat) need human labels on a stratified subset.

**R1.2 Null false-alarm calibration.** For a candidate procedure Π with thresholds θ, the frame-level false-acceptance rate is `FAR(θ) = P(Π accepts ≥1 component | frame has no I-pixels)`. Estimate it on (a) seeded synthetic null families: flat, gradients ≤ registered depth, vignetting with η ≥ η_V, bright edge sources, nebulosity, correlated noise, quantized and zero-MAD data; and (b) real frames whose matched flat certifies `I = ∅`. Choose the smallest thresholds whose one-sided 95% upper bound on FAR ≤ α_frame (proposed α_frame = 10⁻³ for synthetic families, so ≥ 2,995 null frames per family with zero accepted components, since 1 − 0.05^(1/n) ≤ 10⁻³ requires n ≥ 2,995; real-frame FAR is reported with its group-bootstrap bound). Power is then measured, never tuned, on injections at those thresholds. This replaces hand-set `minimum_abs_z_score`, `maximum_tile_noise_ratio` and `minimum_spatial_correlation` with values traceable to an error rate.

**R1.3 Flat-derived reference labels (A28).** For each processed science frame, locate its master flat through the calibration pipeline's provenance (directory linkage of `rust_om_cal`, used for evaluation grouping only, never as detector input), compute η, and emit V/I/Q truth planes. This is independent of the detector, which never sees the flat. Limitations to report: a flat cannot represent pointing-dependent occultation, scattered light or overscan; dome flats may differ from sky illumination; it is defined only for frames whose flat is matched by night and filter.

**R1.4 Injection–recovery.** Transplant realistic border patterns into flat-certified clean real frames. Use measured `η` maps from other nights, and add shot noise consistent with the image's own pixel-derived mean–variance relation: σ²(μ) = a + μ/g, fitted robustly over trusted meshes. Calibrated frames get the flat-amplified form σ_cal = σ_raw/η. Inject sources (point, extended, trailed TNO-like) across boundaries. Truth is exact; groups are by night.

**R1.5 Metrics and splits.** Implement the §2 metrics, plus boundary distance (pixel-centre convention, symmetric 95th-percentile Hausdorff), component recall, source-loss count and flux bias. Groups are by night/instrument with frozen hashes. The validation code stays in `tools/` (Python/Astropy) as an independent implementation; Rust never imports it.

### R2 — reference, noise and evidence model (P2/P3)

**R2.1 Background and noise maps.** On meshes of b×b pixels (default b = 64, bounded 16 ≤ b ≤ min(W,H)/4):

1. Per mesh, run iterative κσ clipping (κ = 3, ≤ 5 iterations, stop when the retained set is unchanged) about the median, with σ̂ = 1.4826·MAD. If MAD = 0 on quantized data, use σ̂ = max(1.2533·mean|x−median|, q/√12), with stored-value step q (1 for integers).
2. Exclude U0, source-mask pixels (R2.3) and the current candidate set. A mesh is *trusted* if the valid fraction is ≥ 0.5 and n ≥ 64.
3. Model the background as a robust low-order surface B(x,y): Huber IRLS on trusted mesh medians, polynomial degree ≤ 2 in centred/scaled coordinates, rank- and condition-checked (κ₂ ≤ 10⁸). Choose the degree by grouped cross-validation over meshes, preferring the lower degree within 1 SE. Compare it with a 3×3-median-filtered mesh interpolant in an ablation.
4. Noise map σ(x,y) is a bilinear interpolation of trusted-mesh σ̂ (median-filtered 3×3). Structure scatter σ_s is the robust scatter of trusted mesh medians about B, minus their sampling variance (floored at 0).
5. Extrapolate to border meshes only up to D_max = 2 meshes from trusted support. Beyond that, the prediction SE is inflated by the extrapolation leverage, and when leverage exceeds a registered limit the component abstains (`reference_extrapolation_abstained`).
6. Iterate at most 3 times: detect, remove accepted candidates from the support, refit. Stop when the accepted mask changes by < 0.5% or support collapses (abstain).

This replaces both the central-tile median and the max-interior-σ rule (A02, A18). The centre has no privileged status: any trusted mesh supports the model.

**R2.2 Two-sided per-tile evidence.** For tile t with n_t finite pixels, residual median r_t = median(x − B) and dispersion σ_t:

- Effective sample size: n_eff = n_t·(1 − ρ̂)/(1 + ρ̂), where ρ̂ is the lag-one autocorrelation of detrended residuals pooled over trusted meshes (AR(1) approximation; clamp ρ̂ ∈ [0, 0.9]).
- Level statistic: z_L = r_t / √(σ_s² + SE_B(t)² + (π/2)·σ(t)²/n_eff). The factor π/2 is the asymptotic variance ratio of the median to the mean for Gaussian noise.
- Dispersion statistic: z_V = ln(σ_t/σ(t)) / √(1.36/n_eff + 1.36/n_ref,eff). The constant 1.36 is the asymptotic variance of ln(MAD) × n for Gaussian samples (MAD relative efficiency ≈ 0.37). It is two-sided: z_V ≪ 0 means unexposed/overscan/zero-flat plateaus; z_V ≫ 0 means flat-amplified noise, glow or hot edges (A19).
- Morphology: fraction of the tile in the source mask; detrended positive-excess peakedness max(x−B)/σ; per-axis detrended lag-one correlations with pair counts.

Initial candidate rule (interpretable; thresholds from R1.2, starting values only):

- **(V)** |z_V| ≥ T_V and |z_L| ≥ T_L, *or*
- **(D)** z_L ≤ −T_L,strong with a passed boundary test (R3.1), *or*
- **(C)** σ_t = 0 against a non-constant reference (exact plateau).

Positive-excess tiles that are source-dominated cannot seed components (R2.3). Starting values for the FAR search: T_L = 5, T_V = 4, T_L,strong = 8.

**R2.3 Source mask and protection.** Seed with pixels where (x − B)/σ > 5 in 4-connected groups of ≥ 5 pixels. Estimate a pixel-derived FWHM from the median second moment of isolated, unsaturated detections (bounded 1–20 px). Dilate by r = ⌈1.5·FWHM⌉; cap iterations at 3. Uses: (a) exclusion from mesh statistics; (b) a **constraint**: an accepted artifact component may not remove a source footprint unless that footprint lies wholly inside a region independently satisfying rule (V) with z_V < 0 (a dark, plateau-like region cannot host a genuine source). Removed source footprints are reported per frame. Unknown faint sources stay in the injection risk budget (G4).

**R2.4 Correlation.** Use only detrended per-tile residuals (subtract a robust per-tile plane). Report horizontal and vertical directions separately with pair counts. Correlation can corroborate (V)/(D) but can never create a candidate (A03, A20).

### R3 — native-resolution boundaries and mask construction (P4)

**R3.1 Boundary change-point test.** For each accepted component, order pixels by the distance transform d to the frame edge within the component's edge segment. Build robust 1-px residual profiles in blocks of L = 32 along the edge. Fit (a) a smooth model (quadratic in d) and (b) a change-point model: a constant/linear level and dispersion on each side of d*, with an optional linear ramp of width w ≥ 0. Use Huber loss and choose by 5-fold blocked cross-validated loss. The profile-likelihood interval {d : Δloss ≤ χ²₁,0.95/2} gives d̂* with an uncertainty band. Smooth-model wins mean "no step" (gradient), which kills probe class A03. Rule (D) requires the change-point model to win with ΔCV-loss above a registered margin.

**R3.2 Hysteresis growth at native resolution.** Seeds are pixels of accepted components at least one tile from the tile-mask boundary. The pixel score is s(x) = median_{5×5}((x−B)/σ), computed only inside a band of ±(tile + uncertainty) around the tile boundary. Grow 4-connected (8-connected only if validated) through pixels with |s| ≥ T_low and the component's sign/dispersion signature. Stop at protected sources. Pixels with T_low ≤ |s| < T_high and outside the R3.1 interval form the uncertain band Q̂. T_high/T_low come from R1.2 at pixel level. The work is O(band area), not O(N).

**R3.3 Morphology.** Off by default. Only a registered hole-fill of enclosed holes with area ≤ π·(FWHM)² whose pixels also satisfy |s| ≥ T_low/2; never across sources.

**R3.4 Mask reasons.** PIXACT3 (when requested): 0 retained, 1 accepted artifact, 2 crop sacrifice, 3 original undefined, 4 retained flag in tolerant crop, 5 uncertain band retained, 6 uncertain band excluded, 7 protected source. Precedence: 3 > 7 > 1 > 6 > 2 > 5 > 4 > 0. The binary mask product is unchanged. `boundary_policy = retain_uncertain | exclude_uncertain` is explicit (default `retain_uncertain`: valid retention first).

### R4 — action selection and area optimality (P5)

**R4.1 Policy replaces evidence-weighted utility (A24).** Hard constraints come first: no U0 in any product; protected-source loss = 0; residual flagged fraction ≤ ρ; sacrificed valid pixels ≤ hard cap c_max. Then accept a crop iff `sacrificed_valid ≤ λ · artifacts_removed_from_storage`. λ is the user's declared exchange rate: how many unflagged defined pixels may be discarded per detector-flagged pixel removed (proposed starting λ = 1). Component-count tiers become a deprecated compatibility path. Old rule files keep their current meaning until migrated with an explicit schema version.

**R4.2 Exact coarse-to-fine tolerant rectangle (A23).** Problem: maximize area(R) subject to U0∩R = ∅ and flags(R) ≤ ρ·area(R). For fixed rows [t,b] with column flag counts c_j and column-undefined indicators, this is the longest subarray with Σ(c_j − ρh) ≤ 0 within undefined-free column runs, solvable in O(W) with prefix sums and a monotone prefix-minimum stack. Exhaustive cost is O(H²W). Run it exactly on s×s block counts (s chosen so (H/s)²(W/s) ≤ 2·10⁸), then refine each side of the best few coarse solutions at full resolution within ±s. Gap bound: when feasibility is preserved under shrinking (ρ = 0, or an absolute flag-count limit), snapping the true optimum inward to the block grid gives a coarse-feasible rectangle, so area(R*) − area(R̂) ≤ s·(W + H). Blocks containing any U0 sample are forbidden at the coarse level, so the coarse pass stays conservative for U0. For the fractional constraint, shrinking can raise the flagged fraction, so no a-priori bound is claimed. Report the empirical gap against exhaustive search on fixtures and against the current anchor family. Always also evaluate the anchor family and take the better feasible result, so R4.2 can never return a smaller crop than the current method. Small-grid exhaustive oracles (all rectangles, ≤ 6×6 masks with 3-state pixels) are mandatory.

**R4.3 Fallback when crop fails (A21, D1).** `crop_fallback = keep | mask | bbox_mask`. The default stays `keep` until the user decides. Reports always list the dominated alternatives.

**R4.4 Hybrid products (A22, D2).** `bbox_mask`: tight bounding box of K ∪ Q̂ with exclusions set to BLANK/NaN. `crop_masked`: the R4.2 rectangle with retained flags set to BLANK/NaN. Both reuse the lazy sentinel rules and the no-sentinel keep fallback. On the same trusted mask, `bbox_mask` retains exactly as many valid pixels as mask mode and removes every flagged pixel (Pareto-optimal on the two area objectives). The rectangle families trade valid area for a fully defined array.

**R4.5 Support-based abstention (A25).** Replace `maximum_mask_fraction` with minimum trusted support: ≥ N_mesh,min trusted meshes (proposed 9) spanning ≥ 2 mesh widths in each axis. A frame whose usable field is small but well supported is processed; a frame without a trustworthy reference abstains with an explicit code.

### R5 — calibration, held-out evaluation and promotion (P7)

Freeze R2–R4 thresholds on development groups using R1.2 (FAR) and R1.4 (power, boundary, source loss). Verify on calibration groups. Evaluate once on untouched groups with flat labels (R1.3) and the human-labelled subset. Claim a scientific release of v2 only if G1–G5 and the additional gates in §6 pass in the declared domain. (Revision 3: decision D5 already made v2 the default; this rule now governs the release claim, and the §0R.1 rollback trigger governs the default.)

### R6 — performance hardening (P6, after R2–R4 stabilise)

- **Border-band evaluation.** Fine-tile statistics only within the outer band and adjacent to candidates; background meshes on deterministic strided subsamples (≥ 1,024 samples per mesh). This is a numerical change evaluated as such.
- **f32 analysis buffer** when centred integer spans ≤ 2²⁴ (exact) or float data tolerate it; otherwise f64. A numerical change, evaluated as such.
- **Bit-packed or u8 label planes** with single ownership; integral images only for crop search, released before writing.
- **Weighted memory admission** from per-stage peak estimates (R0.1). On network output roots, batch directory syncs while keeping per-product data sync.
- **G5 benchmark** on registered workloads: 25-frame development set; one 37.8-Mpx frame; 500 mixed yearly frames; mask/crop; with and without PNG; 1/4 workers; cold and warm.

### R-order and dependencies

R0 (independent; do first) → R1 (blocks all promotion) → R2 → R3 → R4 (R4.2–R4.5 can start after R0 because they consume any mask) → R5 → R6 (R6 profiling starts at R0.1). Every R item ends with a scoped commit, an evidence row in `validation.md`, and status in this section.


### Revision 2 implementation status (2026-10-29)

The user approved D1 = `bbox_mask` fallback, D2 = opt-in `crop_masked`, D3 = master flats as evaluation labels only, and D4 = eta_I 0.20 / eta_V 0.50 with Q in between, then asked for implementation. Commits on branch `claude/eager-galileo-khb8fs` (local, not pushed): `12036af`, `00122c5`, `6e1e4c3`, `3c8f62b`, `c6b8f5b`, `1c7b7ac`, `6bce4e3`, `9d60043` and the documentation commit that records this section. Evidence is in [validation](validation.md#revision-2-implementation-evidence-2026-10-29).

| Item | Status | Evidence / remaining |
| --- | --- | --- |
| R0.1 stage timers | done | `stage_ms` {read, detect, geometry, write} in summary JSON/CSV. Early error paths can omit some stages. |
| R0.2 exact selection | done | 2,000-trial bit-identity property test. Decision digest over 48 fixtures x 3 detector models is identical to the previous revision. 7100x5325 detection 1.69 s to 1.17 s single-threaded (-31%, meets the registered >= 30%). |
| R0.3 intra-image parallelism | done | Ordered per-tile-row collection; same digest; 0.345 s with 4 threads. |
| R0.4 multiscale reuse | done (exact) | Scales are evaluated concurrently and merged in fixed order; lag-one correlation is computed only when it can change a candidate decision. Median/MAD statistics cannot be derived across tile widths, so exact reuse beyond this is not possible. 37.8 Mpx multiscale detection 1.25-1.4 -> 0.54-0.56 s; single-scale tile_v1 0.35 -> 0.17 s. |
| R0.5 in-memory mask verification | done | No mask sidecar file. Previous vs new release on 2x18 fixtures in mask and crop modes: 0 science differences; fsync 69 to 41 and 64 to 39; renames 27 to 18. |
| R0.6 doc drift | done | Revision 2 plus this implementation's docs. |
| R1.1 label semantics | done (synthetic) | V/I/Q/U0 codes with eta 0.20/0.50 in `tools/make_synthetic_corpus.py` and `tools/flat_labels.py`. |
| R1.2 null false-alarm calibration | done (synthetic) | Ignored Rust harness with exact Clopper-Pearson bounds over seven null families. G3a result below. Real flat-certified null frames are pending data access. |
| R1.3 flat-derived labels | tool done | `tools/flat_labels.py` with unit tests. Running it on the yearly tree needs the calibration provenance list and data access (not available in the review container). |
| R1.4 injection-recovery | done (synthetic) | Rust harness families: strips, corners, raw and flat-divided circular stops. Python corpus with exact truth. |
| R1.5 metrics and splits | done | `tools/evaluate_masks.py` reconstructs M from products only; Q excluded with best/worst bounds; pooled and macro values; group-bootstrap intervals. |
| R2.1 background/noise | done (v2) | Meshes, trusted coherent cluster, Huber surface with blocked-CV degree, extrapolation limit, structure scatter, noise grids, detrended AR(1), support abstention. |
| R2.2 two-sided evidence | done, amended | Dispersion uses second-difference noise; minimum-effect test against the D4 boundary; noise-only rule added. |
| R2.3 source mask/protection | done, amended | Pixel-derived FWHM and dilation; local-background seed validation; protected positive source tiles. A20 probe: 0 pixels removed; 0/300 edge-source false alarms after the amendment. |
| R2.4 correlation demoted | done | v2 does not use correlation as evidence. Detrended correlation is used only in the AR(1) estimate. |
| R3.1 change-point test | done, amended | CV ratio >= 2 plus an abrupt-step requirement (jump >= 50% of contrast). |
| R3.2 hysteresis boundaries | done, amended | Exact banded signed distance. Pixel classes are anchored to D4 relative illumination (eta = 1 - r/r0 from the component core, or the noise ratio). A01 20/28-px strips and A18 are exact (0 FP, 0 FN). Raw circular-stop valid loss fell from 12.6% to 0.02%. |
| R3.3 morphology | not implemented | Off by design; no evidence yet that hole filling is needed. |
| R3.4 reasons / uncertain band | partial | `uncertain_boundary_pixels` and `uncertain_policy` implemented; the PIXACT3 reason plane is deferred (PIXACT2 still written). |
| R4.1 exchange-rate policy | done; shipped (Revision 3) | `crop_exchange_rate = 0.5`, flat cap 0.10, crop residual 0, `crop_masked_residual_fraction = 0.05`; tiers no longer shipped. Chosen on the synthetic grid and confirmed on a held-out corpus (§0R.2). Real-data calibration is open. |
| R4.2 global tolerant rectangle | done | Exhaustive 3-state oracle on 3,000 grids; never smaller than the anchor family. |
| R4.3 crop fallback (D1) | done | `crop_fallback = bbox_mask` shipped; legacy files keep `keep`. |
| R4.4 hybrid products (D2) | done | `--mode crop_masked`; the bounding-box product; independent checker verifies masked interiors. |
| R4.5 support-based abstention | done (v2) | `reference_support_abstained`; v1 keeps its mask-fraction veto for compatibility. |
| R5 calibration and promotion | open | Synthetic evidence: G3a met at 256 px (0/2,995 in all seven null families, bound <= 1e-3; 512 px in validation). G3 met for strips and raw stops (recall 1.000, precision >= 0.997), and for pooled valid loss <= 0.1% and per-image <= 1% on the corpus. Flat-divided stop recall is now 0.963-0.966 (>= 0.95) after source-seed validation. Not met: small corners below the minimum mask fraction (a policy gate). G4 needs flat labels and human labels on held-out night groups. v2 became the default by decision D5 (§0R.1); the release claim still waits for G4, and the rollback trigger governs the default. |
| R6 performance hardening | mostly done | Exact work (decisions unchanged): selection-based read centring, buffered hashing, pixel-grid background evaluators, refit caches, parallel banded distance, single on-disk verification, direct data-unit encoding, parallel crop families and summed-area tables, border-flood v2 tile evaluation (diagnostics `candidate_tiles`/`score` now cover evaluated tiles). Weighted per-image memory admission from a measured peak model replaces the fixed worker cap. Strided mesh statistics (`mesh_min_samples = 1024`) were evaluated as a numerical change and adopted: 0 false alarms in all 14 null cells at 512/1024 px, unchanged detections and recall, valid-loss differences of about 0.0003 percentage points; clean-frame v2 detection -26%. f32 analysis buffers are not adopted: memory gain about 4 of 26 B/px, at the cost of changed arithmetic for float data. |
| R1.6 out-of-model corpus | done for nulls; K2 measured | Five null families at 2,995 frames each: v2 0 false alarms per family (bound <= 1e-3); v1 25/2,995 on crowded edges. Labelled corner glow and light leak: v2 recall 0 (K2 confirmed), v1 0.11-0.23 with precision 1.0. A positive-artifact rule is not implemented; it must be opt-in and pass the nebulosity and edge-source nulls first. |
| R5.0 shadow comparison | tool done | `tools/shadow_compare.py`: stratified enriched design, Horvitz-Thompson ratio estimates, paired differences, rollback check; simulation-validated. Running it needs observatory data. |
| R5.2 drift monitoring | tool done | `tools/drift_monitor.py`: per-year rates against the median of other years, exact binomial tests, Holm control; false-flag rate 2.5-3% on homogeneous simulated years. |
| Product headers and crop provenance (user requests) | done | Source header cards copied verbatim except NAXIS1/2, CRPIX*, existing LTV1/2, a needed BLANK and CHECKSUM/DATASUM (user chose to keep the CRPIX shift so the WCS stays correct). All program information in MO_ keywords on every product; MO_CROP = T with MO_SEC, MO_ONAX1/2 and MO_X0/Y0 on crops. Full-frame rectangles become masks. `tools/apply_crop.py` applies the same rule to calibration frames (59/59 alignment check). |

**Design amendments forced by calibration evidence** (all v2, made while it was opt-in; each reproduced by the harness before and after the fix):
1. **Second-difference dispersion instead of residual MAD.** Unmodelled vignetting ramps were counted as noise: 155/200 false alarms at 512 px.
2. **One-sided minimum-effect test against the D4 boundary.** Significance alone flagged expected photon-noise changes in large tiles.
3. **Polynomial extrapolation up to two mesh widths beyond trusted support.** Clamping at mesh centres created edge kinks, causing 154/200 gradient false alarms once rho was detrended.
4. **Abrupt-step requirement in the change-point test.** Two-line models fit hinges and ramps.
5. **Mesh trust from the largest spatially coherent noise-consistent cluster.** A contaminated robust scale let a quadratic absorb circular field stops: 14/50 detected.
6. **Smooth clipped mean-absolute noise for quantized data, and at least 8x8 meshes.** Integer MAD granularity split the sky, and small frames abstained.

7. **Pixel refinement classes anchored to D4 relative illumination**, not only significance. Growth through any significant deficit removed 12.6% of valid pixels inside raw illumination ramps.

8. **Local-background validation of source seeds.** Peak > 5 sigma of ring second-difference noise. Noise clumps in flat-divided exteriors had become protected "sources", holding recall at 0.93-0.94.

The harness also showed that a null gradient family must keep illumination at or above eta_V as frame size grows, and that the Python corpus must use a physical noise model (sigma ~ sqrt(eta)). Both fixtures were corrected rather than tuned around.

## 5. Rules, heuristics and reproducibility configuration

Keep operational `input/config.toml` separate from versioned science policy `input/heuristics.toml`. Proposed schema groups (not current supported keys): `reference`, `sources`, `features`, `components`, `boundary`, `decision`, `geometry`, `uncertainty`. Give each field units, range, interactions, rationale, provenance and missing-data behavior. Bound tile sizes, lag sets, iteration counts, morphology radius, candidate count and uncertainty resamples. Add schema version and migration/error guidance; old configs must not silently acquire different scientific meaning.

Decision policy must expose valid-loss ceiling, residual-artifact ceiling, minimum retained usable area, protected-aperture loss limit, minimum expected/empirical benefit and abstention/domain criteria. Source-protection margins depend on validated PSF/tail evidence, not a magic fixed universal radius. Default thresholds are provisional until calibrated on the registered domain. Do not auto-tune thresholds per test image to force the preferred shape or clipping outcome.

**Revision-2 rule set (proposed `heuristics` schema 2; not yet supported keys).** Planned groups and their first fields:

- `reference`: `mesh_size_px`, `clip_kappa`, `clip_iterations`, `trusted_valid_fraction`, `surface_max_degree`, `extrapolation_meshes`, `refit_iterations`.
- `evidence`: `level_threshold` T_L, `dispersion_threshold` T_V, `strong_level_threshold`, `ar1_clamp`, `correlation_role = "corroborate"`.
- `sources`: `seed_snr`, `min_pixels`, `fwhm_bounds_px`, `dilation_fwhm`, `iterations`.
- `boundary`: `profile_block_px`, `cv_folds`, `min_cv_gain`, `hysteresis_low/high`, `connectivity`, `uncertain_policy`.
- `decision`: `exchange_rate_lambda`, `hard_sacrifice_cap`, `residual_fraction`, `crop_fallback`, `min_trusted_meshes`.
- `calibration`: `target_frame_far`, `far_confidence`, the null-family list, seeds.

Each threshold records its R1.2 provenance: data hash, FAR bound and power at injection strata. A rule file without `schema = 2` keeps the current v1 meaning exactly. No v1 key silently changes meaning. A migration tool prints the v1→v2 mapping and refuses ambiguous conversions. Rules are selected explicitly by configuration, never inferred from headers.

Record effective config and hashes, policy/algorithm/schema version, source commit/dirty status, build counter, executable identity, input hash, random seeds, numeric precision, toolchain/dependency versions, decision terms, alternatives, failure/abstention reasons and outputs. Use compact per-component records plus optional bounded diagnostic arrays under existing run directories. Sensitive/unrelated headers need not be copied into logs merely to record provenance.

## 6. Acceptance gates and test matrix

Numbers below are **proposed registration targets**, not measurements or universal tolerances. Freeze domain-specific values in P0/P2 with scientific justification before held-out evaluation. If no responsible basis or enough independent data exists, mark the corresponding gate unevaluated and retain research status. Passing synthetic targets cannot substitute for the real-data gate.

| Gate | Required evidence |
| --- | --- |
| G1 — integrity and contracts | 100% retained raw sample identity for supported storage, correct original/new null semantics, one-HDU outputs, immutable inputs, preserved allowed cards, valid checksums, complete accounting, kept/dry-run applied counts zero. Every observed audit probe becomes a regression. Unsupported metadata/format conventions have explicit outcomes. |
| G2 — numerical/geometry | Seeded property tests plus independent brute-force small-grid rectangle/EDT/circle-domain oracles; finite arithmetic and safe dimension bounds. Affine scaling/offset equivalence within representable precision, transpose/reflection consistency, stable tile-shift behavior and deterministic threads. WCS-equivalent retained points across the declared support matrix; proposed reference tolerance ≤1e-6 pixel inverse residual and ≤1e-8 degree angular separation where numerically meaningful, tightened/relaxed only with documented projection/conditioning evidence. All sampled WCS values must be finite in the tested domain. |
| G3 — synthetic science | Zero removals on deterministic exact no-artifact controls; ≥95% artifact recall and ≥99% removal precision for registered clearly separable noisy pattern families; ≤0.1% pooled valid loss and ≤1% per-image valid loss, with hard-source constraints; boundary-distance 95th percentile ≤1 pixel for high-contrast sharp boundaries. Report IoU, component recall, uncertainty-band coverage, worst-family results and every abstention/failure. Undefined precision on a zero-removal frame is not a pass for recall. Diffuse boundaries use label uncertainty, not the sharp-edge target. |
| G4 — real science | Group-aware one-sided 95% bounds meeting frozen valid-loss/removal-precision/recall/coverage and contamination targets in each claimed domain; report macro per-image, pixel-weighted and worst-stratum values. Proposed automatic-action coverage ≥90% of supported clearly separable artifact-bearing frames prevents “keep everything” from passing; artifact-free frames instead require correct keep behavior. Declare support/clarity strata from independent labels before seeing decisions. Compare against current baseline and keep-all/freeform references; a truth-mask reference is an oracle only on labeled data. For apertures/trails wholly inside retained valid support, preserve exact direct sums; independently assess background/selection changes, normalized flux bias and recovery, with proposed median bias <0.1 reference sigma, ≤1 percentage-point completeness loss, and no unreported protected-aperture truncation. Any user-domain science tolerance takes precedence before registration. |
| G3a — null false alarms (Rev. 2) | For each registered synthetic null family (flat, gradients, illuminated vignetting η ≥ η_V, bright edge sources, nebulosity, correlated and quantized noise), zero accepted components in ≥ 2,995 seeded frames. This gives a one-sided 95% FAR bound ≤ 10⁻³ per frame. Report real flat-certified null frames with a group-bootstrap bound. Probe fixtures A03, A18 and A20 become permanent regressions: no removal for the A03/A20 nulls, and an identical decision with or without the A18 interior star. |
| G3b — source protection (Rev. 2) | Zero accepted removal of injected sources with peak S/N ≥ 10 whose footprint lies in V. Report completeness and flux bias for fainter injections, and every protected-source intersection. |
| G3c — evidence two-sidedness (Rev. 2) | Recall on high-dispersion (flat-amplified, z_V > 0) border families meets the same G3 recall target as low-dispersion families. |
| G5 — performance | Release-mode paired repeated measurements with ≥10 measured runs per registered workload/worker setting after a declared warm-up, randomized/interleaved order, cold/warm conditions separated. Record median, tail latency and confidence intervals for time/throughput, peak RSS, allocation/I/O counts and failures. Target ≥20% time or RSS reduction in a measured bottleneck workload, no >5% median-time regression on other registered critical workloads without explicit scientific tradeoff. Peak process RSS must stay within the documented budget including reserved overhead; oversized jobs fail before full allocation. No scientific gate may regress for a speed-only change. |

The 95% confidence choice and many stratum checks require a registered multiplicity strategy or simultaneous bounds; do not select a flattering subset afterward. Independent groups, not correlated pixels, determine power. Example sample-size check: with zero failures in n genuinely independent trials, the exact one-sided 95% upper failure-rate bound is `1 − 0.05^(1/n)`; approximately 299 trials are needed to bound a 1% rate. This calculation does not apply directly to millions of adjacent pixels or dependent exposures. Plan real sample sizes from the actual target metric and group dependence.

Revision-2 additions to G2: exhaustive 3-state (defined, flagged, undefined) rectangle oracles up to 6×6 for the tolerant search; property tests that R0 exact-selection statistics equal the sort-based reference bit for bit; 1/2/4-worker bit-identity with intra-image parallelism. Addition to G5: R0 must be bit-identical in products and decisions, and its registered target is ≥ 30% detector-stage time reduction on the 37.8-Mpx workload. R0.5 must report bytes written and fsync counts before and after.

Required matrix includes:

- Geometry: one/two/four corners, individual and opposite sides, unequal strips, off-center circles, ellipses, partial arcs, rotated boundaries, mixed components, thin slivers, diagonal contact, islands/holes, small/odd/large dimensions and all tile-origin phases.
- Pixel statistics: Poisson/read and colored noise, directional stripes, variable variance, bimodal backgrounds, quantization/zero MAD, gradients/vignetting, all-invalid/mostly-invalid input, sparse valid tiles, central obstruction and no trustworthy reference.
- Science confounders: bright edge stars/halos, nebulosity/galaxies, crowding, saturation/bleed, faint/TNO sources and trails crossing uncertain boundaries, seeing-dependent aperture margins and changing occultation flux. Record detections lost as well as fluxes of survivors.
- Storage/provenance: all BITPIX, signed/unsigned scaling, large BZERO, negative/tiny BSCALE, int64 near extremes, finite overflow, full uint8/int16 code occupancy, existing/invalid BLANK, NaN payload/signed-zero preservation policy, compressed signatures, multi-HDU, source replacement, FITS commentary/long strings and primary/alternate/distorted WCS.
- Operational: dry run, recursion, collisions, sanitization, symlink targets, output isolation, disk/permission/logger/PNG failure, interruption, report schema compatibility, worker equivalence and weighted-memory stress.

Independent tools verify file **meaning** and science outcomes, not merely the same implementation twice. Improve the existing checker to compare canonical raw bytes where required, actual mask metadata, every supported WCS solution on retained points, and NaN-aware rectangular crops without treating invalid coordinates as successful WCS agreement. Use [FITS 4.0](https://fits.gsfc.nasa.gov/fits_standard.html) for format contracts and heed the [Astropy cutout distortion limitation](https://docs.astropy.org/en/stable/api/astropy.nddata.Cutout2D.html) when choosing reference checks.

## 7. Bounded experiments and stopping rules

Do not add every plausible algorithm. Evaluate a ladder: B0 current baseline; B1 correctness fixes; B2 robust local/source reference; B3 multiscale/component evidence; B4 boundary refinement; B5 common action policy; B6 exact engineering optimizations. Perform ablations at each step on the same grouped development splits. Keep separate records of numerical/engineering equivalence and science-algorithm changes. Test optional graph refinement, subpixel geometry or calibrated classifiers only if simpler methods leave a measured gap.

Register practical gains before experiments: initially 0.1 percentage point less valid loss at fixed artifact recall, 1 percentage point greater artifact recall at fixed valid loss, 0.5 pixel improvement in boundary-distance percentile, or 10% time/RSS improvement with unchanged science. These are development decision thresholds to justify extra complexity, not relaxed release limits. Use confidence intervals and worst-domain results, not point estimates alone. Reject dominated candidates and unexplained safety regressions.

Limit a development round to the registered candidate/parameter budget (initially at most three alternatives per unresolved stage, with runtime and seed budgets recorded). Stop adding complexity when consecutive independent development rounds have upper improvement bounds below the registered practical gain, or lack sufficient evidence to distinguish candidates. “No significant improvement” with low power is inconclusive, not equivalence. Reopen only for a new failure, new observing domain or substantially new method/data. Never use repeated hold-out testing as the stopping loop.

**Planning stop condition:** perform separate review passes for science/objective, correctness/provenance, statistical evaluation and implementation/performance. A material finding changes safety, estimands, data requirements, method choice, acceptance gate, order/dependency or failure handling. Map every such finding to an action and evidence requirement. Finish this plan when two consecutive full coverage passes reveal no new material omission; record deferred research instead of asserting all future improvement is impossible. The completion of this condition is documented below and in the audit.

## 8. Plan review ledger and remaining dependencies

| Pass | Material improvements incorporated |
| --- | --- |
| 1 — source/design reconciliation | Distinguished planned source/multiscale work from fixed-tile implementation; made U0/V/I/Q and candidate/action sets explicit; narrowed geometry claims; introduced a common constrained loss objective and per-component decisions. |
| 2 — adversarial correctness review | Added measured tile under/over-removal, full-code BLANK failure, large-BZERO precision loss, content-based compression exclusion and mask/science header corrections. Included WCS, publication, dry-run reporting and immutable build identity gaps. |
| 3 — statistical and feasibility review | Added group leakage controls, complete-pipeline selection calibration, uncertain-label bounds, coverage/abstention accounting, prevalence and multiplicity, source-completeness metrics, memory-stage accounting and exact optimization equivalence. |
| 4 — adversarial plan review | Reserved final real test data, constrained source/protected-area handling, bounded morphology/candidate search, specified geometry domains and uncertainty, defined quantitative gain/stopping thresholds and diagnostic/schema compatibility. Added an explicit removal-precision gate, deterministic action ties and independently defined coverage strata to prevent gaming of recall/abstention metrics. |
| 5 — implementation-specific review | Specified the first local-reference estimator, bounded robust fit, effect-size equation and competing boundary hypotheses; kept uncertainty distinct from effect size and selected-model calibration. These replace an otherwise underspecified instruction to simply improve statistics. |
| 6 — full coverage review | Rechecked objectives, all A01–A17 findings, phase dependencies, metric denominators, uncertainty/label handling, geometry limits, source protection, storage/report semantics and performance gates. Every material item has an action and required evidence; no further material plan omission found. |
| 7 — final consistency review | Rechecked current-versus-planned claims across README, architecture/config guides, original plans, skills and memory; verified relative documentation links and skill structure. Confirmed source/default thresholds unchanged and implementation/release dependencies explicit. No further material plan omission found. |

| 8 — code-and-probe review (Revision 2) | Read the detector, crop, pipeline, writer and configuration sources against the documented contracts. Ran six deterministic probes. Added observed findings A18 (max-σ reference makes decisions non-local), A19 (one-sided variance misses flat-amplified borders), A20 (edge sources removed) and the observed form of A03 (vignetting gradients removed). Re-measured A01 and a large-frame timing baseline. |
| 9 — statistical design review | Replaced uncalibrated scale choices with explicit standard errors: median SE with π/2, ln-MAD variance 1.36/n, an AR(1) effective sample size, and structure scatter σ_s. Made variance evidence two-sided. Defined thresholds through a null false-alarm rate with the rule-of-three sample size (A29). Demoted correlation to corroboration. Added the change-point boundary test that separates gradients from steps. |
| 10 — area-optimality and action review | Identified the dominated keep fallback (A21), retained flags in tolerant crops (A22), the anchor-restricted search (A23), the dimensionless utility and component tiers (A24), and the mask-fraction veto (A25). Added the exact coarse-to-fine rectangle search with a gap bound, an exchange-rate policy λ, `bbox_mask`/`crop_masked` hybrids, and support-based abstention. Recorded user decisions D1–D2. |
| 11 — performance and I/O review | Measured the sort-based tile-statistics cost and the multiscale multiplier. Identified the transient sidecar write+fsync+verify (A31). Added exact selection, scratch reuse, ordered intra-image parallelism, in-memory verification, border-band evaluation and f32 analysis. Speed-only items are bit-identical; numerical items are evaluated separately. |
| 12 — evaluation-data review | Found an independent pixel truth source already in the production tree: matched master flats (A28). Defined V/I/Q from relative illumination (D3–D4) and listed the classes a flat cannot label. Added injection–recovery with pixel-derived mean–variance noise, including flat amplification, and frozen night-grouped splits. |
| 13 — full coverage review | Rechecked every A01–A31 item for an action, gate and dependency. Checked units (stored-value units; dimensionless η, z, λ), coordinate conventions (zero-based internal, pixel-centre distance), failure and abstention codes, schema/identity changes (v2 opt-in IDs, PIXACT3, schema bump), contracts (header-free detection; flats used only as evaluation labels unless D3 says otherwise), and documentation drift (A27 fixed). One material item was added: R4.5 replaces the 0.5 mask-fraction veto, which otherwise contradicts the valid-retention objective for small fields. |
| 14 — final consistency review | Re-read §0, §4A, §5–§7 and the audit addendum together with README, AGENTS, skills and memory. Confirmed that every new threshold is a registered starting value, that no claim of validation was introduced, that defaults are unchanged pending D1–D4, and that the order is consistent (R1 before any promotion). No new material omission. |
| 15–19 — Revision 3 | D5 risk register and rollback trigger, out-of-model corpus, weighted enriched labelling, drift monitoring and action-specific G3 targets; passes 18–19 found no new material omission. See §0R.5. |

**Revision-2 closure.** Passes 13 and 14 were consecutive full reviews; pass 14 found no new material omission. Pass 13 added one item, R4.5, so the stop rule (§7: two consecutive passes without a material finding) required pass 14 to be checked again after R4.5 was integrated. It was, with no further change. Further planning is therefore expected to change details or calibrated numbers only, not safety, estimands, data requirements, method family, gates or order. Planning reopens on new failure evidence, a new observing domain, the user's answers to D1–D4, or R1 measurements that contradict an assumption (for example, flats unusable as labels or FAR unattainable at useful power). The plan remains a design: no new algorithm is implemented, calibrated or validated by this revision.

Historical note on passes 6–7: the two final coverage reviews of Revision 1 satisfied the planning stop condition at that time. This is a bounded review conclusion, not proof that no future method or data could improve the plan. Scientific release still depends on an independently labeled corpus, a declared target domain/error budget, implemented corrections, validated new algorithms and measured performance. These are explicit implementation prerequisites, not missing design decisions to conceal with optimistic claims.
