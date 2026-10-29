# Residual-tolerant rectangular crops

Date: 2026-10-29
Status: implemented and validated on synthetic tests and the four requested development frames.

## Objective

The user selected a crop policy that may retain a small number of pixels flagged by the detector when a larger rectangle preserves more image area. Keep original undefined samples as hard exclusions, retain the existing valid-pixel-sacrifice and utility gates, and expose the residual-pattern allowance in `input/heuristics.toml`.

## Frozen initial policy

- The shipped `maximum_residual_artifact_fraction = 0.05` is measured as detector-flagged, defined samples retained divided by defined samples inside the crop. This is a provisional configurable ceiling, not a calibrated contamination probability or science-grade threshold. Legacy custom heuristic files that omit the key default to `0.0` and preserve strict behavior.
- Search only rectangles that contain the current exact maximum clean rectangle. Generate expansions on each of its four sides on a deterministic pixel grid; choose the largest feasible rectangle. Limit evaluations to 100,000 per image and report the chosen pixel step and evaluation count. If no tolerance candidate is feasible, the strict clean rectangle remains the fallback.
- A candidate is feasible only if it contains no original undefined samples, its residual detector fraction is at or below the configured ceiling, and it passes the existing component-count crop-sacrifice cap and minimum net-utility gate.
- Rank feasible candidates by area, then fewer residual detector pixels, higher net utility, then topmost/leftmost origin. This is maximum area within the bounded expansion candidate family, not a global optimum over all rectangles with residual pixels.
- Report retained detector pixels and their fraction separately from artifact pixels removed and unmasked pixels sacrificed. Add reason-plane code 4 for a detector-flagged sample retained inside a tolerant crop. Keep codes 1–3 meanings unchanged. Implemented as `PIXACT2` in Rust and the independent Python checker.

## Phases and evidence

1. Add the validated heuristic key, defaults, help and documentation. **Complete.**
2. Implement deterministic bounded expansion search with checked integral counts, undefined-sample hard constraints, candidate budget and diagnostics. **Complete.**
3. Integrate it into crop selection and reporting without changing mask mode or detector proposals. **Complete.**
4. Add unit/oracle tests for residual-budget boundaries, undefined samples, deterministic ties, candidate budget and sacrifice/utility rejection; run Rust formatting, tests and Clippy. **Complete: 69 unit, 29 end-to-end and 4 version tests; 17 Python checker tests; strict Clippy and formatting pass.**
5. Reprocess only the four named 2004–2005 frames in an isolated diagnostic directory. Compare crop areas, residual flags, source-slice identity, FITS dimensions/WCS and independent FITS validation. Do not modify whole-year summaries or the stopped 2011 run. **Complete: release 0.2.74, run `20261029T080236.828463976Z`; all four accepted; Astropy/fitsverify and 500-sample preview validation passed.**
6. Record the provisional policy and limitations, inspect the diff, and make a scoped local commit. **Complete: implementation and validation were committed as `590b375`; no Git remote is configured for a push.**

## Stop conditions and limitations

Do not raise the 5% ceiling from these four repeatedly inspected frames. If the bounded search finds no materially larger feasible crops, report the measured Pareto tradeoff instead of silently weakening the limit. The detector mask remains unlabeled and must not be described as ground-truth invalidity. Existing whole-year products are outside this scoped regeneration; the current 2011 output remains incomplete as recorded in the yearly-remediation plan.
