# Yearly large-image remediation plan

Date: 2026-10-29
Status: implementation and selected-year reruns in progress.

## Scope and observed cause

Reprocess only 2009, 2011, 2012, 2017, 2018, 2019, 2021, 2023, 2024 and 2026 from `/mnt/uxmal_groups/image_storage/rust_om_cal/<year>/science_calibrated/` into `/mnt/uxmal_groups/image_storage/cropped/<year>/science_calibrated/` using the checked-in yearly script. The old 512 MiB per-image workspace estimate rejected 35, 58, 305, 25, 60, 22, 567, 0, 1,575 and 100 images respectively in years with prior summaries; these are size-policy rejections, not FITS parse failures. 2023 has 4,515 inputs and no prior summary. Two 2024 inputs have truncated headers and cannot be decoded.

## Phases and acceptance criteria

1. **Capacity setting:** admit the largest observed 7100×5325 frame using the documented 32-byte/pixel estimate and retain a conservative four-worker cap. Configuration and reference docs must agree; do not call the estimate a hard RSS bound.
2. **Executable:** build the release executable from the updated configuration and check its version/help without changing the scientific heuristics.
3. **Selected reruns:** invoke the existing script once for each requested year, never with its all-years default. Its explicit sanitation clears only the dedicated output root for that selected year. Keep PNG previews enabled as the script defines.
4. **Audit:** for each year compare summary `total` and `output_images` against source FITS count; confirm workspace-limit errors are zero, every discovered input has an output entry, and the two truncated 2024 files are annotated source symlinks. Summarize clipped/kept/ignored/error counts and run paths. A residual malformed-source error is acceptable only when preserved and clearly explained.
5. **Local registration:** update project memory and validation evidence with observed run IDs, counts, elapsed time and any genuine residual input errors; inspect diff and commit only task-owned tracked changes. Do not stage generated output or source observations.

## Operational and scientific limits

The host reports 125 GiB total memory and 93 GiB available at planning time; this supports a larger bounded workload, but the configured estimate does not measure actual worker RSS. The crop detector and thresholds remain unchanged. This remediation addresses resource-based pass-throughs, not scientific classification accuracy. Source files remain untouched. Each selected output directory is explicitly sanitized by the script; pre-existing results there will be replaced.

The 2023 source has no `dark/` directory. The yearly script reports this missing ancillary source and continues processing science images; it never invents a dark-frame symlink. Existing calibration-link conflicts still stop that year's processing.

## Evidence checklist

- [x] Existing summaries and source inventory inspected for the requested years.
- [x] Workspace policy raised to cover the largest observed frame; four configured threads retained.
- [ ] Release binary built.
- [ ] Ten requested years rerun with per-year progress.
- [ ] Summary parity, remaining errors and expected symlinks checked.
- [ ] Actual elapsed time and memory measured/reported where available.
- [ ] Documentation and memory updated with final evidence; verified local commit created.
