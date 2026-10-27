---
name: scientific-astrometry
description: Preserve and validate FITS WCS, pixel-coordinate conventions and astrometric accuracy during masking or cropping.
---

# Scientific Astrometry

Respond in English. Distinguish zero-based internal coordinates from FITS one-based coordinates and state axis ordering. WCS/header values are not classification evidence for pixel-pattern clipping, but structural metadata is needed for FITS decoding and WCS must be preserved in generated products. Dimension-preserving masks leave the pixel grid and WCS unchanged. For a crop starting at zero-based (x0,y0), standard reference pixels shift by CRPIX1 -= x0 and CRPIX2 -= y0, including supported alternate WCS versions. Validate coordinate equivalence at retained pixels with an independent WCS implementation. A kept image symlink retains its original WCS cards and source bytes unchanged. Generated products may add CREATOR/DATE/HISTORY provenance and must refresh checksums. Inspect distortion conventions and metadata references; support and test them explicitly or decline cropping. Preserve frames, epochs, units, timing and original science metadata. Never claim arbitrary header preservation after geometry changes; document necessary structural, WCS and checksum updates.
