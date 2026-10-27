# User guide

## Current scope

The Rust CLI processes FITS files with exactly one two-dimensional primary image. It accepts unsigned 8-bit and signed 16/32/64-bit integer images and floating images (BITPIX -32/-64). Multi-HDU files, cubes, zero-sized arrays, non-image primary HDUs and gzip-compressed FITS inputs are reported as ignored. Corrupt files and resource violations are errors.

The detector is an initial conservative tile-based method. It compares robust tile medians and noise with an interior reference, requires strong contrast plus spatial coherence or reduced noise, and floods only candidate tiles connected to the image border. It can describe asymmetric corners, one-sided strips, rectangular and partly/circular patterns through the resulting mask. Detection evidence comes from pixels only: observation labels, WCS, instrument names and other descriptive header cards do not affect pattern classification. FITS structural, storage-scaling and null-value cards are still needed to decode and interpret the sample array. Detection scores are diagnostic Z-like scores, capped at 1,000,000, not probabilities. Constant regions have no defined Pearson correlation; a clear contrast plus zero measured dispersion can still qualify. Real instrument examples and held-out evaluation are required before scientific use.

Mask mode marks clipped samples as undefined in a copy of the original image, preserves dimensions and WCS, and writes a companion uint8 FITS mask. Crop mode compares the maximum-area clean axis-aligned rectangle with a conservative integer-centered circle using pixel-mask geometry alone, then chooses the shape retaining more pixel centers (rectangle wins ties). The circle is stored in its rectangular FITS bounding box, with pixels outside the circle marked undefined. Crop mode updates dimensions and primary/alternate `CRPIX1/2` values while retaining other headers. Reports give geometry, origin, artifact pixels removed, unmasked pixels sacrificed, and net utility. If the configured loss penalty outweighs artifact-removal evidence, the original is kept. Both modes regenerate and verify FITS checksums and preserve retained raw samples.

The [deep audit](scientific-audit-2026-10-27.md) identifies known numerical, sentinel, boundary and header/report limitations. These remain open work under the [scientific improvement plan](scientific-improvement-plan.md). Candidate-mask counts are not ground-truth invalid-area measurements, and circle optimality is limited to the implemented discrete clearance model.

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

Use `--recursive` to search subdirectories, `--heuristics PATH` to select a separate science-rule file, `--mode crop` to choose the larger pixel-derived circle/rectangle (`mask` is the default), `--threads N` to cap workers, `--memory-budget-mib N` to set the total image workspace budget, and `--overwrite` to permit a path collision within the current run. `--help` lists the current options. Input and output trees may not overlap. Originals are never edited.

To review accepted crop geometry visually, add `--png` while running in crop mode:

```bash
cargo run --release -- --input-dir input/data --output-dir output --mode crop --png --recursive
```

Each accepted crop gets `output/runs/<run-id>/previews/<input-relative-path>.png`: source image at left, crop result at right, shared source-derived ZScale, labeled panels, native pixel scale and FITS lower-origin orientation. Invalid values and pixels outside a circular crop render black. Previews are diagnostic displays; FITS files remain the science products. The path is included in each JSON/CSV record. The flag is rejected unless the effective processing mode is `crop`.

## Products

Each run receives a UTC run ID and a new directory under `output/runs/`. The run directory contains `images/`, `masks/`, optional `previews/`, `manifests/`, `summary.json`, `per-image.csv`, and one ISO-dated `activity_*.jsonl` file. Processed files preserve input-relative paths; unchanged eligible images are symlinks to their canonical sources. Generated FITS products include `CREATOR` (`rust_fits_auto_cropping` and build version), UTC `DATE`, and a nanosecond UTC `HISTORY` entry. Science and mask files are written and verified in temporary sibling directories before either final file is published; ordinary publication errors restore the old pair or remove newly published partials. A per-input version-3 `.complete.json` manifest is atomically published only after expected outputs have been written and verified. A crash between the two final renames can leave unmanifested files, so treat the marker as the completion authority. It includes the source digest and, for each product, its path, byte length, SHA-256 and symlink state. Version-6 `summary.json` and `per-image.csv` record source digest, decision fields, and explicit candidate/proposed/applied/crop set intersections. If a product has no completion manifest, treat its output set as incomplete or failed. Hashes identify exact file bytes but do not make several output-file renames transactional. Previous run directories are preserved and never mixed with new products. Before writes, the application rejects output-root and managed-directory symlinks, path traversal, and any input/output overlap. The output root itself may contain unrelated files; the application does not delete them.

In each file report, `candidate_mask_pixels` counts finite samples in border-connected candidate tiles, `proposed_mask_pixels` counts samples in detector-accepted masks, and `applied_artifact_pixels` counts artifact samples excluded in completed products. Their U0 intersections are reported as `candidate_undefined_pixels`, `proposed_undefined_pixels`, and `applied_artifact_undefined_pixels`; the detector currently guarantees these are zero because it selects finite samples only. `candidate_proposed_overlap_pixels` records C∩A. For crop proposals, `artifact_pixels_removed` records A∩crop-excluded, `unmasked_pixels_sacrificed` is E∩defined, and `crop_excluded_undefined_pixels` is E∩U0. `crop_applied_undefined_pixels` reports that last intersection only when the crop is applied; `crop_artifact_pixels_retained` reports accepted artifact pixels that would remain inside the proposed crop and should be zero for a clean crop. `masked_pixels` counts newly excluded defined samples. `reason_code` is a stable machine-readable decision category; `reason` retains explanatory text. Dry runs set `dry_run: true`, report accepted decisions as `proposed`, and leave applied counts at zero. These are bookkeeping sets, not labels of astrophysical truth.

Current reason codes include `compressed_ignored`, `multi_hdu_ignored`, `not_2d_ignored`, `empty_image_ignored`, `integer_precision_ignored`, `reference_fit_abstained`, `input_read_error`, `insufficient_data`, `no_border_candidate`, `candidate_rejected`, `dry_run_proposal`, `mask_applied`, `crop_applied`, `crop_unsupported_wcs`, `crop_geometry_unavailable`, `crop_utility_rejected`, `integer_blank_unavailable_kept`, and stage-specific `output_path_error`, `output_exists_error`, `output_cleanup_error`, `product_write_error`, `kept_symlink_error`, `preview_write_error`, and `manifest_write_error`. `processing_error` is the fallback for an unclassified processing failure. Codes are stable report categories; they do not replace the detailed reason text or imply scientific confidence.

The application build version starts at the three-part value in `input/program-version.txt`. Whenever Cargo recompiles the package, the patch component advances; after 99 it rolls over and advances minor, and after `0.99.99` it advances major. The current local counter is stored in ignored `output/.state/program-version.txt`; the next build migrates the legacy `output/program-version.txt` state file there when present. A Cargo invocation that has no compilation work does not advance the counter.

## Configuration

Edit `input/config.toml` for worker/discovery/memory and output mode settings and `input/heuristics.toml` for clipping decisions; see [configuration reference](configuration.md). Invalid values stop the run before input processing. Effective settings and hashes are recorded in the run-start log. Detector rules are reviewable thresholds, not validated instrument-independent settings or calibrated probabilities. Ambiguous images are kept unchanged.

## Safe interpretation

Inspect the mask FITS and report before scientific analysis. A mask describes rejected pixels; downstream photometry should exclude these values and account for masked aperture area. Smooth sky structure, nebulosity, extended sources, PSF wings and moving targets can resemble correlated patterns. Retain images when detector evidence is ambiguous; compare representative data with instrument-specific knowledge. This release is an engineering prototype, not a validated observatory pipeline.
