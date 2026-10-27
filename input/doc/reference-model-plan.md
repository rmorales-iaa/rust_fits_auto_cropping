# Robust local reference model phase

Status: experimental implementation complete; external calibration remains open.

## Purpose and limit

The current detector compares every tile to a central-tile median. This phase adds a pixel-derived local reference candidate to address smooth affine background gradients without treating headers as classification evidence. The current central reference remains the default. The new method is an experimental configuration choice and will not be described as calibrated or science-grade.

## Method

Implemented in `src/detection/reference.rs` and selected through `heuristics.reference_model`; the baseline remains the default.

1. Reuse the detector's existing finite-sample tile medians and tile dispersions.
2. Select the central interior mesh region using the existing deterministic tile coordinates. Fit `median = b0 + bx*x + by*y`, with x/y centered and scaled to [-1, 1]. Fit only finite interior medians using bounded Huber iteratively reweighted least squares (maximum 10 passes); solve the 3×3 weighted normal equations with pivoting.
3. Reject insufficient, non-finite or rank-deficient fits. Do not silently fall back to central reference after explicit experimental selection; report a conservative no-decision result.
4. Use robust residual dispersion of interior tile medians, combined with the median interior tile MAD and a relative floating-point floor, as the contrast scale. Use median—not maximum—interior tile MAD for the quiet-tile reference.
5. Evaluate the same component, area, and action gates against each tile's predicted local reference. Add deterministic synthetic gradient and plane-outlier fixtures with known values. Do not change output mask semantics or user-visible default.

## Acceptance

- **Passed:** exact-plane null controls remain unaccepted by the experimental detector.
- **Passed:** a separable synthetic border seam on a known smooth affine background is detected, and robust-fit tests resist a single interior outlier.
- **Passed:** rank-deficient and insufficient reference layouts abstain.
- **Passed:** the central-reference baseline remains the default; regression suite covers deterministic worker equivalence.
- **Passed:** configuration, algorithm documentation, report/log identity and validation notes identify the experimental method and its limits.

## Deferred gates

This phase does not add compact-source masks, uncertainty maps, spatially blocked calibration, source/aperture loss evaluation, or independent observatory labels. The robust plane is a nuisance-background candidate, not evidence that any selected pixel is astrophysically invalid. Keep it opt-in pending P0–P4 synthetic and real-data gates.
