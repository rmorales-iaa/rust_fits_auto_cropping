# Crop mode and pixel-only detector decision plan

Date: 2026-10-27. This plan implements the user's correction after inspecting
`science#2007_01_11T20_23_31...11jan_0011.fits`.

Historical implementation record. The current circle search is restricted to integer pixel centers with conservative clearance from masked pixel footprints; it is not a proven maximum over arbitrary subpixel circles or arbitrary usable regions. Counts below refer to detector candidates, not labeled invalid science. See the [audit](scientific-audit-2026-10-27.md) and [active plan](scientific-improvement-plan.md) for current limitations and future acceptance gates.

## Observed issue

The initial mask-mode run rejected the input before detection because its `BITPIX=16` header has
`BLANK=32768`, outside the stored signed range. The data are signed 16-bit with
`BSCALE=1` and `BZERO=32768`; the raw stored pixels contain no zero values, so
the out-of-range BLANK card matches no stored sample. An analysis-only copy with
that invalid card removed produced a pixel-only corner-pattern mask. Its
largest rectangle containing no proposed mask pixels is `[x=352,1264),
y=[80,1408)` (912 by 1328), retaining 1,211,136 sample centers. The largest
artifact-free circle is centered at `(840,840)` with radius `712.1133` pixels;
it retains 1,593,141 sample centers, 31.5% more than the rectangle. Its FITS
bounding box is 1425 by 1425, with samples outside the circle undefined. This
circle removes 530,752 candidate pixels and sacrifices 503,723 unmasked
pixels. The configured crop utility is positive (0.6542), so this frame is
cropped in circle mode. The crop bounds and sacrificed pixels are in the run
report.

## Intended behavior

1. Detection decisions use image sample statistics and spatial pattern only.
   FITS structural metadata remains necessary to decode dimensions, pixel
   storage and HDU boundaries; WCS and other semantic keywords do not enter the
   pattern score. Invalid `BLANK` must not prevent pixel analysis. It is not
   treated as a sample sentinel unless it is representable in the stored pixel
   type and actually matches stored data.
2. Add an explicit `--mode crop` alongside the existing dimension-preserving
   `--mode mask`. Keep mask as the default. Crops are derived from the detected
   pixel mask, not from FITS instrument or WCS keywords.
3. For crop mode, compare two pixel-derived geometries: (a) the maximum-area
   axis-aligned rectangle containing no detected mask pixels, and (b) the
   largest artifact-free circle. Choose the geometry that retains more image
   samples, with rectangle winning ties. Use deterministic center/tie breaks.
   A circular result is represented as its rectangular FITS bounding box with
   pixels outside the circle marked undefined. This retains the valid arcs a
   rectangle would discard.
4. Crop raw stored samples without rescaling. Preserve non-structural headers,
   update NAXIS dimensions, adjust primary/alternate `CRPIX1/2` for the crop
   origin, regenerate checksums and remove an invalid BLANK card. For circle
   products, write valid BLANK/NaN values outside the circle. Record geometry,
   origin/dimensions, removed candidate pixels and unmasked pixels sacrificed.
5. Crop behavior is opt-in and does not convert or edit source data. Each crop
   product remains an independent FITS file with one image HDU.

## Acceptance checks

- A valid one-HDU image with an invalid, unmatched BLANK header is analyzed;
  input remains byte-identical, and output does not carry the invalid BLANK.
- A synthetic four-corner pattern selects the geometry retaining more image
  samples (rectangle or circle), preserves retained raw pixels, and writes
  correct undefined markers outside a selected circular boundary.
- No-pattern inputs are kept in crop mode rather than needlessly cropped.
- A mask pixel cannot occur within retained geometry; ties are deterministic.
- Crop dimensions and CRPIX values are correct; Astropy WCS world coordinates
  before and after cropping agree for corresponding retained pixels, including
  a SIP fixture.
- FITS checksums, HDU count, storage type, remaining headers and report totals
  pass Rust and independent-reader checks.
- The target image is run with `--mode crop`; it selects the circle because it
  retains more sample centers than the maximal rectangle. Its output still
  needs human scientific review because it excludes a visible illuminated rim.

The crop selector reports geometric loss rather than inferring that every
unmasked pixel outside retained geometry is disposable. Crop utility compares
evidence-weighted artifact removal with unmasked pixels sacrificed; a
sub-threshold utility keeps the source unchanged.
