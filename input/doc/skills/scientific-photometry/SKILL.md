---
name: scientific-photometry
description: Validate image masks and transformations against astronomical photometric integrity, background estimation and faint-source selection effects.
---

# Scientific Photometry

Respond in English. Preserve valid pixel values and units. Model sky background robustly while excluding sources, saturation and invalid samples. Distinguish background structure from instrumental artifacts; do not remove faint emission merely because it is smooth or correlated. For this clipping tool, use pixel-derived candidates only and treat structural/scaling/null cards as decoding metadata, not photometric evidence. Compare aperture fluxes, sky estimates and uncertainties before/after processing on unmasked sources; include edge stars, extended objects, PSF wings, gradients and faint injected sources. In crop mode explicitly count unmasked pixels sacrificed versus artifact pixels removed and apply the configured utility threshold; if rejected, keep the source via symlink without modifying it. Track masked fractions for source and background apertures and flag incomplete measurements. Do not replace rejected values with apparently valid zeros. Quantify selection bias and document uncertainty and evidence in `input/doc/`.
