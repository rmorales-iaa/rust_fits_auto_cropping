# Rectangular FITS crop contract

Status: implemented and verified on 2026-10-28. Historical card names: `CLIPSHAP`/`CLIPX0`... were later replaced; cropped products now carry `MO_CROP`, `MO_SEC`, `MO_ONAX1/2` and `MO_X0/Y0` (see README "Product headers"). The exact rectangle maximizes retained area under the detector-plus-undefined mask; independent science-label evaluation remains open.

## Requirement and interpretation

Every generated crop product must be a fully rectangular FITS pixel array (a square is a special case). Every sample inside selected bounds must be outside detector exclusions and originally undefined pixels. Select the exact maximum-area axis-aligned rectangle over the complete combined exclusion mask. Do not constrain the search to a fitted circle: a larger rectangle elsewhere in the clean detector mask is valid for this contract and may retain more samples. The crop origin and `NAXIS1/2` must describe exactly the selected rectangle.

Select from the union of detector-proposed exclusions and originally undefined input samples. The maximal-rectangle algorithm uses a histogram/monotonic-stack search and is exact over all axis-aligned rectangles in that binary mask. This establishes a geometric optimum under the program's pixel mask, not that the mask matches ground truth or that the result maximizes true astrophysical information. The existing loss/utility gate remains in force and may keep the source image unchanged.

## Implementation steps

1. Select only the global maximum-area all-unmasked rectangle over the complete detector-plus-undefined mask; do not let fitted circle geometry constrain the search.
2. Ensure the writer serializes only rectangle samples, updates `NAXIS1/2`, shifts supported `CRPIX` values, and does not need a synthetic circle `BLANK` value.
3. Update reports, preview boundary geometry, CLI help, README, algorithm/user/configuration docs, active plan and project memory to describe rectangular crops and the valid-pixel tradeoff.
4. Replace circle-output integration expectations with regressions proving dimensions, global maximum area, no added undefined corner samples, preserved pixels, WCS offset and preview alignment. Keep exhaustive rectangle-oracle coverage and run end-to-end FITS/Astropy checks on the supplied dataset.

## Acceptance criteria

- The production crop selector always returns `CLIPSHAP=RECTANGLE` for accepted crop products.
- Output axes and serialized `NAXIS1/2` exactly equal selected rectangle width and height; no detector-flagged or originally undefined sample lies inside the selected crop, and no output sample is undefined solely because it lies outside a circular boundary.
- Every finite defined output sample maps exactly to its source coordinate within rectangle bounds; `CLIPX0/1`, `CLIPY0/1`, `NAXIS1/2`, and supported primary/alternate `CRPIX1/2` agree.
- Exhaustive small-grid comparison confirms rectangle area optimality over the complete axis-aligned family for every tested mask.
- The configured utility gate can decline a rectangle when its sacrifice is not justified.
- Rust tests, strict Clippy, formatting, Astropy and `fitsverify` pass. Development-corpus results remain engineering evidence, not a labeled science-accuracy claim.

