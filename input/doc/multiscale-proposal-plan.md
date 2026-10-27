# Opt-in multiscale consensus phase

Status: experimental implementation and engineering validation complete; science calibration and promotion remain open.

## Objective and scope

Add a deterministic, bounded evidence path that requires an edge pattern to survive at least two of three tile widths derived from configured base width (`max(4, base/2)`, `base`, and a dimension-bounded `2*base`). Each scale runs the existing single-scale detector and its independent component gates. The pixel-level proposal is the intersection-vote mask supported at two or more accepted scales. Reapply image-level retained-area and provisional utility gates to that consensus. Any unsupported scale family, insufficient consensus, or rejected aggregate keeps the source.

The default `evidence_model = "single_scale"` must retain existing decisions and cost. The experimental mode is opt-in; reports/logs identify it as experimental. No Z-score is described as a probability, scale persistence is not calibrated confidence, and no FITS header becomes detector evidence.

## Implementation phases

1. **Configuration and identity:** add a serde-defaulted `evidence_model`; preserve old config compatibility and default behavior; include the evidence/reference combination in run and summary algorithm identity.
2. **Consensus engine:** keep single-scale behavior in an internal function. Run up to three deterministic scales with bounded dimensions. Require two-scale accepted support per pixel; preserve finite-mask semantics; recalculate area and utility for the final consensus; abstain when the support/area gates fail.
3. **Validation:** add analytic null, exact affine gradient, multi-corner and one-scale-only controls; verify deterministic repeated calls and existing baseline behavior. Exercise config/report/log identity end to end.
4. **Documentation:** record semantics, work multiplier, limitations, and non-calibrated status in algorithm/config/user docs, active plan, validation, README, memory, and validation skill.
5. **Verification and registration:** run all Rust tests, strict Clippy, formatting, and Python WCS checks; inspect the diff and commit this coherent phase.

## Acceptance and explicit limits

- Single-scale remains the default and retains its existing algorithm ID and output semantics.
- The multiscale path never accepts a candidate supported by fewer than two scales, and the exact affine/null fixture is kept.
- Every selected output pixel is finite in the source and supported by at least two scale masks.
- Final mask fraction and utility pass the configured limits after consensus.
- Algorithm provenance distinguishes all model combinations.
- This phase does not implement shifted grid origins, source masks, uncertainty estimates, pixel-boundary calibration, significance tests, or independent observatory evaluation. The method remains experimental and P3 remains partial.

## Evidence

All planned engineering phases passed: old configs default to single scale; the baseline algorithm ID remains unchanged; the experimental identity is present in summary and activity log; five analytic unit tests and one end-to-end test cover consensus, null retention, one-scale abstention, offset boundaries and provenance. The full project suite and strict static checks are recorded in `validation.md`. A release dry run on the unlabelled supplied development set proposed 16 frames, kept 4, and reported 5 pre-existing malformed headers; no outputs were written. This is not science validation.
