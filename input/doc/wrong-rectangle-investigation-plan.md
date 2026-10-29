# Reported wrong crop rectangles: investigation plan

Date: 2026-10-29
Status: diagnosis complete; no rectangle-geometry defect reproduced. Science-policy choice for the detector masks remains open.

## Inputs

Inspect these four exact science frames and their existing FITS crops/previews:

- `science#2004_02_16T19_03_20_000`
- `science#2004_02_17T01_06_26_000`
- `science#2004_02_17T02_05_12_000`
- `science#2005_01_07T18_26_24_000`

Use pixel samples and mask geometry only; header descriptions are not detector evidence.

## Phases and acceptance criteria

1. **Reproduce and trace:** complete. Captured reason planes during a four-file diagnostic run and independently verified exact source slices and `NAXIS1/2`.
2. **Separate causes:** complete. An independent histogram-stack implementation over each reason plane reproduced all four rectangles exactly. The rectangle selector is correct for its supplied mask; the proposed detector masks drive the narrow crops.
3. **Apply a scoped fix:** no code or threshold change is justified by these unlabeled frames alone. Three 2004 masks are broad circular/annular exclusions; the 2005 mask is fragmented and sacrifices 46.5% of the remaining defined pixels. Ground-truth usability labels or a user decision on allowing residual detected pixels are needed before changing the science policy.
4. **Regenerate and validate:** complete in a separate diagnostic root with release `0.2.57`. All four proposals exactly reproduce the existing `0.2.49` summaries. Astropy confirms exact source slices and dimensions; `fitsverify` reports zero errors. Temporary mask and reason-mask directories were removed; the rendered overlay remains as a review artifact.

## Existing observations

The existing 2004/2005 summaries report rectangles 1024×960, 624×1184, 560×1120 and 608×736. Independent Astropy checks confirm each generated array exactly equals the reported source slice, and its `NAXIS1/2` equals the crop width/height. `fitsverify` reports zero errors on all four. Thus the first investigation gate is the mask-conditioned optimality and detector proposal geometry, not FITS serialization.

The fresh diagnostic run's exact reason-code counts `(retained, detector exclusion, geometric sacrifice)` are respectively `(983040, 598590, 458976)`, `(738816, 744288, 336896)`, `(627200, 859936, 332864)`, and `(447488, 212224, 388864)`. Its reason planes independently select the same `(x,y,width,height)` as the Rust implementation: `(224,400,1024,960)`, `(448,80,624,1184)`, `(464,160,560,1120)`, `(128,288,608,736)`. The overlay is `/fast_disk/tmp/clippling_test/rectangle_investigation_20261029_fpj28i5i/reason_mask_overlay.png`; red is detector exclusion, orange is defined-pixel crop sacrifice, and green is the selected crop. Reason-mask copies were deleted after producing the overlay.

The original directory products were not modified by this diagnosis. A larger rectangle would include pixels labeled as detector artifacts under the current mask; deciding whether that residual is acceptable is a science-policy choice, not a rectangle-search correction.
