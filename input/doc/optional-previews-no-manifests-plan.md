# Optional previews and manifest-free output plan

## Goal

Do not generate PNG previews unless `--png` is passed, and do not create or publish a `manifests/` directory or completion-marker files. Keep the per-run summary, CSV, and activity log as the reporting record.

## Phases and acceptance criteria

1. **Output layout:** create only required product directories eagerly; remove manifest paths/categories; create `previews/` lazily only when an accepted crop actually requests a preview. Unit tests prove no manifest directory and no default preview directory.
2. **Pipeline and reports:** remove completion-marker publication/invalidation and manifest-specific errors; preserve output collision/overwrite checks for actual products. Remove manifest paths from outcomes and CSV; update schema and tests.
3. **Help and documentation:** state that `--png` is opt-in and previews are absent unless requested; remove completion-manifest claims and document the summary/log as run records.
4. **Verification:** run Rust and Python tests, formatting, strict Clippy, and the supplied 25-image corpus both with and without `--png`; assert no manifest directories/files and no previews by default, then independently validate FITS and requested previews.

## Status

- Phase 1: complete. New runs eagerly create `images/` and transient `masks/`; `previews/` is lazy and `manifests/` has no output category or path.
- Phase 2: complete. Completion-marker publication/invalidation and manifest report fields were removed; schema version is 9. Summary/CSV/activity reporting remains.
- Phase 3: complete. README, CLI help, user guide, architecture, project memory and the repository FITS engineering skill now state that previews are opt-in and manifests are not produced.
- Phase 4: complete. Rust tests (57 unit, 28 end-to-end, 4 version), Python checker tests (16), formatting and strict Clippy pass. The 25-image corpus without `--png` produced no `previews/` or `manifests/`; the `--png` run produced 21 previews and no `manifests/`. Both runs had 25/25 image entries (21 clipped, 4 kept, 0 ignored/errors); Astropy/fitsverify passed all 25 FITS entries, and the requested-preview run passed 500-sample validation for all 21 PNGs.
