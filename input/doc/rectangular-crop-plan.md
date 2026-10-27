# Rectangular FITS crop contract

Status: implemented and verified on 2026-10-28. Independent science-label evaluation remains open.

## Requirement and interpretation

Every generated crop product must be a fully rectangular FITS pixel array (a square is a special case). It must not be a rectangular storage box containing a circular region with undefined corner samples. Every sample inside selected bounds must be outside detector exclusions and originally undefined pixels. When a conservative clean circle retains more sample centers than the unrestricted maximum rectangle, select the maximum-area axis-aligned rectangle wholly inside that circle; otherwise use the unrestricted maximum clean rectangle. This lets the output retain a circularly supported field without exporting a circle or its invalid bounding-box corners. The crop origin and `NAXIS1/2` must describe exactly the selected rectangle.

Select crop support from the union of detector exclusions and originally undefined input pixels. A conservative circle is another pixel-mask-derived support proposal; it is never written directly. If its clean-pixel count exceeds the unrestricted maximum rectangle area, compute the exact maximum-area axis-aligned rectangle within the circle; otherwise use the unrestricted maximum clean rectangle. These are precise geometric optima for the stated discrete families, not a claim that the detector mask matches ground truth or that the result maximizes true astrophysical information. A circular clean field therefore yields a square/rectangle with fewer pixels than a circular array would; this is the cost of excluding the circle's unsupported corners while meeting the requested FITS shape contract. The existing loss/utility gate remains in force and may keep the source image unchanged.

## Implementation steps

1. Keep pixel-mask-derived circle geometry as an optional clean-support candidate, but make the crop selector return only a maximum-area all-unmasked rectangle; never serialize a circle mask or its bounding box.
2. Ensure the writer serializes only rectangle samples, updates `NAXIS1/2`, shifts supported `CRPIX` values, and does not need a synthetic circle `BLANK` value.
3. Update reports, preview boundary geometry, CLI help, README, algorithm/user/configuration docs, active plan and project memory to describe rectangular crops and the valid-pixel tradeoff.
4. Replace circle-output integration expectations with regressions proving dimensions, exact bounds, no added undefined corner samples, preserved pixels, WCS offset and preview alignment. Keep exhaustive circle/rectangle geometry oracles and run end-to-end FITS/Astropy checks on the supplied dataset. **Complete:** 54 unit, 26 end-to-end and 4 version tests pass; Astropy/fitsverify checked all parseable products from run `20261027T230523.268308343Z`.

## Acceptance criteria

- The production crop selector always returns `CLIPSHAP=RECTANGLE` for accepted crop products, including when conservative circle support wins.
- Output axes and serialized `NAXIS1/2` exactly equal selected rectangle width and height; no detector-flagged or originally undefined sample lies inside the selected crop, and no output sample is undefined solely because it lies outside a circular boundary.
- Every finite defined output sample maps exactly to its source coordinate within rectangle bounds; `CLIPX0/1`, `CLIPY0/1`, `NAXIS1/2`, and supported primary/alternate `CRPIX1/2` agree.
- Exhaustive small-grid comparisons confirm circle support geometry and rectangle area optimality over both the unrestricted and circle-constrained axis-aligned families.
- The configured utility gate can decline a rectangle when its sacrifice is not justified.
- Rust tests, strict Clippy, formatting, Astropy and `fitsverify` pass. Development-corpus results remain engineering evidence, not a labeled science-accuracy claim.

