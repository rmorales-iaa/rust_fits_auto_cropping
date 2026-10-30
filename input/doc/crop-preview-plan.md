# Optional side-by-side crop preview plan

Status: implemented; crop-fit display revision verified on 2026-10-28.

## Goal

Add `--png` as an opt-in output for accepted crop-mode images. For each image actually cropped, write `output/runs/<run-id>/previews/<input-relative-path>.png` (since 2026-10-30 without the FITS extension; see README) with the full input on the left and the actual crop on the right. Keep FITS processing and the science crop decision unchanged.

## Rendering contract

- Render grayscale with a deterministic implementation of IRAF/Astropy ZScale sampling and rejection defaults: up to 1000 evenly strided finite samples, contrast 0.25, up to five rejection iterations, 2.5-sigma rejection, and fallback to sample extrema when the accepted sample count is too small. Use one interval computed from the full source for both panels to make their pixel intensities directly comparable.
- Keep the source panel at native scale. Fit the rectangular crop uniformly to the source panel height with nearest-neighbor sampling, preserving its aspect ratio and adding no blank crop-panel margins. Label this panel `CROP FIT` so differing display scale is explicit. Use astronomical orientation (`y=0` at the bottom), and render invalid pixels as black.
- `--png` requires crop mode. Dry runs, masked images, kept/symlinked images, ignored files, and crops rejected by the loss utility do not produce PNGs. Existing output collision and `--overwrite` behavior applies to PNG files.
- Write PNGs atomically and record their paths in the per-image report. A preview write failure is a per-file error; any already-verified FITS crop/mask products remain valid and are retained.

## Implementation phases

1. Add the PNG-only Rust `image` dependency, a small ZScale implementation, and a renderer module that composites labeled grayscale panels with checked dimensions.
2. Add the `--png` CLI flag, require crop mode, publish previews under `output/runs/<run-id>/previews/`, and include the paths in JSON/CSV reporting and activity logs.
3. Add deterministic ZScale numerical tests, preview geometry/orientation/blank tests, CLI behavior and overwrite tests. Independently cross-check rendered samples and ZScale limits against Astropy; exercise the supplied FITS set and check time/RSS impact.
4. Update help, README, user/algorithm/architecture docs, skill guidance and validation evidence. Run format, complete tests, Clippy, release help smoke test and the FITS/PNG interoperability checker, then commit the verified change locally.

## Acceptance checks

- With `--mode crop --png`, every accepted crop has exactly one corresponding side-by-side PNG; rejected crops and kept files do not.
- Both panels use identical `vmin/vmax`; the source keeps native scale and the crop panel is uniformly fitted to source-panel height with nearest-neighbor sampling, its displayed scale is labeled, dimensions match source width plus fitted crop width and a labeled top band, and vertical axis direction matches FITS pixel coordinates.
- Astropy independently confirms representative grayscale values and ZScale limits; PNG decoder confirms valid dimensions/data.
- No input or FITS product bytes change due to preview generation. Without `--png`, the run's `previews/` directory remains empty.

## Completion record

- Added the opt-in `--png` flag and rejected it unless the effective processing mode is crop.
- Added a pure-Rust PNG renderer using the Rust `image` crate, atomic publication and source-derived shared ZScale display range.
- Recorded output paths in JSON, CSV and per-file activity events. FITS crop outputs are retained if preview encoding fails and the file is reported as an error.
- Added unit and end-to-end tests plus `tools/cross_validate_previews.py` for independent Astropy/Pillow validation.
- All 16 accepted crops in the supplied real dataset passed independent rendering and geometry checks. Measured one-run throughput and memory are recorded in `validation.md`.
- `cargo fmt --all --check`, `cargo test --all-targets` (46 tests), `cargo clippy --all-targets -- -D warnings`, release build and release CLI help/version smoke checks passed. All tasks use the local Git commit workflow after verification.
