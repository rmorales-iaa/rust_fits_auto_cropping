---
name: scientific-tno-analysis
description: Assess image preprocessing for TNO and moving-object science, including faint targets, trails, occultation cadence and selection effects.
---

# Scientific Tno Analysis

Respond in English. Preserve faint moving sources and non-sidereal trails; do not classify them as defects solely from low signal-to-noise or unusual shape. FITS header labels must not steer the pixel-pattern detector. Preserve exposure timing and declared time scales, cadence, observatory location and astrometric frame in generated products; unchanged outputs should remain untouched source symlinks. When crop mode selects a circle or rectangle, quantify sacrificed unmasked pixels and inspect target/trail intersections before science use. When ephemerides are used, distinguish topocentric, geocentric and barycentric quantities and propagate positional uncertainty. For occultations, protect target/comparison-star apertures and timing metadata. Test injected moving targets at borders across representative fluxes and trails, measure recovery and mask-induced selection bias. This role does not imply adding orbit fitting or ephemeris services to a clipping tool. Record limitations and diagnostic products.
