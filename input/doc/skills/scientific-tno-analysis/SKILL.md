---
name: scientific-tno-analysis
description: Assess image preprocessing for TNO and moving-object science, including faint targets, trails, occultation cadence and selection effects.
---

# Scientific Tno Analysis

Respond in English. Preserve faint moving sources and non-sidereal trails; do not classify them as defects solely from low signal-to-noise or unusual shape. FITS header labels must not steer the pixel-pattern detector. Preserve exposure timing and declared time scales, cadence, observatory location and astrometric frame in generated products; unchanged outputs should remain untouched source symlinks. Crop mode selects the global maximum-area clean axis-aligned rectangle over detector exclusions and originally undefined samples, without circle constraints. Quantify sacrificed defined pixels, confirm `NAXIS1/2`, crop origin and shifted WCS reference pixels, and inspect target/trail intersections before science use. A circular field may lose more samples under the rectangular-output constraint; the utility gate can keep the original. When ephemerides are used, distinguish topocentric, geocentric and barycentric quantities and propagate positional uncertainty. For occultations, protect target/comparison-star apertures and timing metadata. Test injected moving targets at borders across representative fluxes and trails, measure recovery and mask-induced selection bias. This role does not imply adding orbit fitting or ephemeris services to a clipping tool. Record limitations and diagnostic products.

Split evaluation by sequence/night so temporally related exposures do not leak between tuning and hold-out data. Measure frame rejection and mask-induced time-dependent selection alongside individual-source recovery; a changing occultation signal must not be removed because it changes the local statistics. Unknown faint targets are protected through injection/recovery risk limits, not only bright-source masks or header target labels.
