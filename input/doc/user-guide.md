# User guide

## Current scope

The Rust CLI processes FITS files with exactly one two-dimensional primary image. It accepts unsigned 8-bit and signed 16/32/64-bit integer images and floating images (BITPIX -32/-64). Multi-HDU files, cubes, zero-sized arrays, non-image primary HDUs and gzip-compressed FITS inputs are reported as ignored. Corrupt files and resource violations are errors. On normal runs, every discovered FITS input receives one corresponding image entry: processed images are generated as configured, while malformed, unsupported and ignored files are annotated and preserved as symlinks to their original files. The summary reports `total` inputs and `output_images`; these counts should match unless output publication itself fails. Dry-run deliberately produces no image entries.

The detector is an initial conservative tile-based method. It compares robust tile medians and noise with an interior reference, requires strong contrast plus spatial coherence or reduced noise, and floods only candidate tiles connected to the image border. It can describe asymmetric corners, one-sided strips, rectangular and partly/circular patterns through the resulting mask. Detection evidence comes from pixels only: observation labels, WCS, instrument names and other descriptive header cards do not affect pattern classification. FITS structural, storage-scaling and null-value cards are still needed to decode and interpret the sample array. Detection scores are diagnostic Z-like scores, capped at 1,000,000, not probabilities. Constant regions have no defined Pearson correlation; a clear contrast plus zero measured dispersion can still qualify. Real instrument examples and held-out evaluation are required before scientific use.

Mask mode marks clipped samples as undefined in a copy of the original image, preserves dimensions and WCS, and writes a companion uint8 FITS mask. Crop mode selects the exact maximum-area axis-aligned rectangle over the combined detector-exclusion and originally undefined-pixel mask. It writes a rectangular or square two-dimensional FITS image with `NAXIS1/2` equal to the selected width and height; the search is not constrained to a circle. Primary and alternate `CRPIX1/2` values shift by the zero-based crop origin. Reports give geometry, origin, artifact pixels removed, unmasked pixels sacrificed, and net utility. The crop-sacrifice cap is selected by the number of accepted contiguous artifact areas touching an image border: 35% for one, two, or three areas and 47% for four or more. A connected area touching multiple borders counts once. If the crop exceeds its selected cap, the original is kept unchanged; the loss-utility rule can also keep it. Malformed non-structural FITS cards are ignored in the parser's temporary view, annotated, and restored byte-for-byte in generated products; the structural cards needed to locate pixel data must remain readable. Both modes regenerate and verify FITS checksums and preserve retained raw samples.

The [deep audit](scientific-audit-2026-10-27.md) identifies known numerical, sentinel, boundary and header/report limitations. These remain open work under the [scientific improvement plan](scientific-improvement-plan.md). Candidate-mask counts are not ground-truth invalid-area measurements; the rectangle is exact only for the implemented detected/undefined mask and axis-aligned geometry family.

## Install and run

Install Rust 1.88 or newer with a stable toolchain. FITS I/O uses a pure-Rust crate and requires no CFITSIO installation. Operational settings and science rules are separate files. Build and run from the repository root:

```bash
cargo build --release
cargo run --release -- --input-dir input/data --output-dir output --config input/config.toml
cargo run --release -- --input-dir input/data --output-dir output --recursive --config input/config.toml --heuristics input/heuristics.toml
```

For a dry run that writes reports and logs without science image or mask products:

```bash
cargo run --release -- --input-dir input/data --output-dir output --dry-run
```

Use `--recursive` to search subdirectories, `--heuristics PATH` to select a separate science-rule file, `--mode crop` to write the maximum-area fully clean rectangle (`mask` is the default), `--threads N` to cap workers, `--memory-budget-mib N` to set the total image workspace budget, and `--overwrite` to permit a path collision within the current run. `--help` lists the current options. Input and output trees may not overlap. Originals are never edited.

To review accepted crop geometry visually, add `--png` while running in crop mode:

```bash
cargo run --release -- --input-dir input/data --output-dir output --mode crop --png --recursive
cargo run --release -- --input-dir input/data --output-dir output --reason-mask
```

PNG previews are disabled by default. Pass `--png` in crop mode to write `output/runs/<run-id>/previews/<input-relative-path>.png` for each accepted crop: grayscale source at left and the exact final FITS crop at right, each rendered with an independently estimated ZScale from its own pixels and FITS lower-origin orientation. The crop panel is uniformly scaled with nearest-neighbor sampling to fill the source panel height, preserving aspect ratio and avoiding empty black margins; its label reads `CROP FIT` because displayed pixel scale can differ. On the source panel, a green line marks retained crop pixels and a red line marks adjacent pixels removed by the crop. The right-hand FITS rendering remains grayscale; display scaling does not alter FITS samples. The `previews/` directory is created only when a preview is written. Invalid source values render black except where diagnostic boundary lines are drawn. Previews are diagnostic displays; FITS files remain the science products. Preview paths appear in JSON/CSV records. The flag is rejected unless the effective processing mode is `crop`.

## Products

By default each run gets a UTC run ID and a new directory under `output/runs/`. Pass `--flat-output` to write directly under the specified output directory; the yearly workflow uses this mode to place `images/`, optional `previews/`, `summary.json`, `per-image.csv`, and the ISO-dated activity log directly in `/mnt/uxmal_groups/image_storage/cropped/<year>/science_calibrated/`. Flat output can reuse existing managed directories; existing image products require `--overwrite` before replacement. The yearly script enables that option for repeat processing. PNG previews are disabled by default. No `manifests/` directory or completion-marker files are generated. Summary and activity records provide per-input status, reasons and output paths. Transient `masks/` and `reason_masks/` sidecars are deleted before reports are finalized and before exit. Unchanged, malformed and ignored inputs are symlinks to canonical sources under the original input-relative name; generated crop images add `_cropped` before the FITS extension. Generated FITS products include `CREATOR`, UTC `DATE`, and a nanosecond UTC `HISTORY` entry. Science files are written and verified before publication. Version-9 `summary.json` reports input/output image counts; the per-image CSV records each input output path, source digest and decision fields. Previous run directories are preserved in the default layout. Before writes, the application rejects output-root and managed-directory symlinks, path traversal, and input/output overlap.

`--reason-mask` builds a temporary byte FITS diagnostic under `reason_masks/<input-relative-path>` for accepted actions; the sidecar is deleted before exit and is not a retained output. `RSCHEME=PIXACT1` defines code `0` as retained defined input, `1` as detector-selected artifact exclusion, `2` as a defined pixel sacrificed by crop geometry, and `3` as pre-existing undefined input. Code 3 takes precedence. These are processing-action diagnostics, not ground-truth science-validity labels or calibrated probabilities. The reason plane is not available after processing.

In each file report, `candidate_mask_pixels` counts finite samples in border-connected candidate tiles, `proposed_mask_pixels` counts samples in detector-accepted masks, and `applied_artifact_pixels` counts artifact samples excluded in completed products. Their U0 intersections are reported as `candidate_undefined_pixels`, `proposed_undefined_pixels`, and `applied_artifact_undefined_pixels`; the detector currently guarantees these are zero because it selects finite samples only. `candidate_proposed_overlap_pixels` records C∩A. For crop proposals, `artifact_pixels_removed` records A∩crop-excluded, `unmasked_pixels_sacrificed` is E∩defined, and `crop_excluded_undefined_pixels` is E∩U0. `crop_applied_undefined_pixels` reports that last intersection only when the crop is applied; `crop_artifact_pixels_retained` reports accepted artifact pixels that would remain inside the proposed crop and should be zero for a clean crop. `masked_pixels` counts newly excluded defined samples. `reason_code` is a stable machine-readable decision category; `reason` retains explanatory text. Dry runs set `dry_run: true`, report accepted decisions as `proposed`, and leave applied counts at zero. These are bookkeeping sets, not labels of astrophysical truth.

Current reason codes include `compressed_ignored`, `multi_hdu_ignored`, `not_2d_ignored`, `empty_image_ignored`, `integer_precision_ignored`, `reference_fit_abstained`, `multiscale_scale_unavailable`, `multiscale_no_consensus`, `multiscale_consensus_rejected`, `input_read_error`, `input_fallback_symlink_error`, `insufficient_data`, `no_border_candidate`, `candidate_rejected`, `dry_run_proposal`, `mask_applied`, `crop_applied`, `crop_wcs_mask_fallback`, `crop_geometry_unavailable`, `crop_utility_rejected`, `crop_sacrifice_limit`, `integer_blank_unavailable_kept`, and stage-specific `output_path_error`, `output_exists_error`, `output_cleanup_error`, `product_write_error`, `kept_symlink_error`, `preview_write_error`. `processing_error` is the fallback for an unclassified processing failure. Codes are stable report categories; they do not replace the detailed reason text or imply scientific confidence.

The application build version starts at the three-part value in `input/program-version.txt`. Whenever Cargo recompiles the package, the patch component advances; after 99 it rolls over and advances minor, and after `0.99.99` it advances major. The current local counter is stored in ignored `output/.state/program-version.txt`; the next build migrates the legacy `output/program-version.txt` state file there when present. A Cargo invocation that has no compilation work does not advance the counter.

## Configuration

Edit `input/config.toml` for worker/discovery/memory and output mode settings and `input/heuristics.toml` for clipping decisions; see [configuration reference](configuration.md). Invalid values stop the run before input processing. Effective settings and hashes are recorded in the run-start log. Detector rules are reviewable thresholds, not validated instrument-independent settings or calibrated probabilities. Ambiguous images are kept unchanged.

## Safe interpretation

Inspect the mask FITS and report before scientific analysis. A mask describes rejected pixels; downstream photometry should exclude these values and account for masked aperture area. Smooth sky structure, nebulosity, extended sources, PSF wings and moving targets can resemble correlated patterns. Retain images when detector evidence is ambiguous; compare representative data with instrument-specific knowledge. This release is an engineering prototype, not a validated observatory pipeline.


Descriptive and WCS header cards do not decide whether a decoded image is sent to pixel-pattern analysis. If a lookup-table distortion prevents a safe rectangular crop, the summary records that limitation and accepted artifact detections are processed as a dimension-preserving full-frame mask; the image is not passed through unchanged for that header reason. Structurally unsupported inputs (including compressed and multi-HDU files) remain outside the current pixel-array contract.
