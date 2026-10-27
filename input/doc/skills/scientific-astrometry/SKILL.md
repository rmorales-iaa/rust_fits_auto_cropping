---
name: scientific-astrometry
description: Preserve and validate FITS WCS, pixel-coordinate conventions and astrometric accuracy during masking or cropping.
---

# Scientific Astrometry

Respond in English. Distinguish zero-based internal coordinates from FITS one-based coordinates and state axis ordering. WCS/header values are not classification evidence for pixel-pattern clipping, but structural metadata is needed for FITS decoding and WCS must be preserved in generated products. Dimension-preserving masks leave the pixel grid and WCS unchanged. Crop products are rectangular axis-aligned arrays: for crop origin `(x0,y0)`, verify `NAXIS=2`, `NAXIS1/2` equal output width/height, and shift standard reference pixels by `CRPIX1 -= x0`, `CRPIX2 -= y0`, including supported alternate WCS versions. Validate coordinate equivalence at retained pixels with an independent WCS implementation. A kept image symlink retains its original WCS cards and source bytes unchanged. Generated products may add CREATOR/DATE/HISTORY provenance and must refresh checksums. Inspect distortion conventions and metadata references; support and test them explicitly or decline cropping. Preserve frames, epochs, units, timing and original science metadata. Never claim arbitrary header preservation after geometry changes; document necessary structural, WCS and checksum updates.

Maintain an explicit WCS support matrix and compare all supported primary/alternate solutions on a distributed sample of retained pixels, including zero-offset crops. Require finite coordinates where the input WCS is valid; shared NaNs do not establish equivalence. Use declared angular and inverse-pixel tolerances with projection conditioning and wraparound handled correctly. Unsupported output-coordinate conventions may prevent cropping but must not influence the pixel-pattern classifier. State the admissible geometry family and pixel-center versus pixel-footprint convention before claiming maximum retained area.
