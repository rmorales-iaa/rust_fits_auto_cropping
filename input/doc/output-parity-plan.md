# One output image per discovered FITS input

## Objective

For every FITS path discovered in a normal run, publish exactly one corresponding `images/<relative-path>` entry. Successfully processed inputs keep the existing image behavior. Malformed/read-failure and unsupported/ignored files are preserved byte-for-byte through a symlink and remain annotated with their error or ignored reason. Dry-run deliberately produces no image products.

## Phases and acceptance criteria

1. **Define report contract:** add an `output_images` count to the summary and bump its schema. The count is the number of outcomes with a published `output_image` path; `total` remains discovered FITS inputs.
2. **Implement pass-through:** centralize fallback for error/ignored outcomes without an image product. Create a canonical source symlink, include it in the per-input completion manifest, preserve the original status/reason, and convert fallback publication failures into explicit errors.
3. **Regressions and docs:** test malformed input and compressed/multi-HDU inputs, assert symlink targets/bytes/manifests and `output_images == total` on successful normal runs, and document the dry-run exception.
4. **Validate and register:** run formatting, the locked Rust test suite and strict Clippy; execute the requested test corpus and confirm one output image per input with FITS/preview validation; commit the coherent change locally.

## Status

- Phase 1: complete. Summary schema 8 reports `total` and `output_images`.
- Phase 2: complete. Malformed and ignored outcomes now publish canonical source symlinks and completion manifests while retaining their annotations; publication failures are explicit errors.
- Phase 3: complete. End-to-end cases cover malformed FITS, multi-HDU and compressed inputs, raw byte identity, symlink targets, manifests, output counts and dry-run behavior. README, help, user guide, project instructions and skill are updated.
- Phase 4: complete. The locked Rust suite passed 83 tests; 14 independent Python checker tests passed; formatting and strict Clippy passed. Clean release `0.1.44` at revision `a1ef5a97967b` produced 25/25 output images from the requested 25-input corpus. Astropy/fitsverify checked 20 parsed FITS images, 16 masks and 16 reason planes; five malformed inputs were verified as source-target symlinks. The preview checker verified all 16 PNGs. The program returned status 1 because the five malformed inputs remain annotated errors.
