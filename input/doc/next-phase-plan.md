# Dataset validation and heuristic policy plan

Date: 2026-10-27. This plan follows the initial implementation. It treats the supplied observations as an evaluation set, not as labeled truth. Raw input images must remain unchanged.

Historical evaluation plan. The [scientific improvement plan](scientific-improvement-plan.md) now controls future research and optimization. The supplied frames have been repeatedly inspected during development and must not be reused as an untouched scientific hold-out.

## User requirements and decisions

1. Test against `/fast_disk/tmp/clippling_test/input/` and place run artifacts/products under `/fast_disk/tmp/clippling_test/output/`.
2. Expand synthetic and end-to-end tests for FITS edge cases, recursive traversal, artifact shapes, false-positive controls, and failure handling.
3. Cross-check generated files with an independent FITS stack. Astropy and `fitsverify` are available on this host; use these tools for a recorded local validation and provide a reusable script.
4. Measure wall time, throughput, and peak resident memory at multiple worker counts on the supplied set; preserve masks across worker counts.
5. Initial crop decision: defer crop mode until a concrete consumer and WCS checks exist. This decision was superseded by the user's review of the supplied four-corner frame. Crop mode is now opt-in, compares maximum-area artifact-free rectangle/circle geometries, and validates the FITS WCS independently; see [crop implementation plan](crop-mode-plan.md).
6. Ignore compressed FITS and every file with more than one HDU; add direct regression coverage for both.
7. Put science decision rules in a separate `input/heuristics.toml` file. Preserve conservative abstention: a candidate must have connected border support, contrast and spatial evidence, and stay within configurable area limits. A proposed mask is accepted only when evidence for artifact removal outweighs the configured penalty for losing potentially valid pixels. The current data has no ground-truth labels, so this policy can be evaluated for behavior and reviewability but its thresholds cannot be scientifically calibrated from this set alone.
8. Expose `--recursive` as a CLI input to traverse nested directories; keep recursive traversal opt-in.
9. Maintain a three-part local build counter from `input/program-version.txt` and add program/version/UTC creation cards to generated FITS products. Summaries must also identify the program/version so unchanged symlink products retain run provenance without modifying input headers.
10. Publish unchanged eligible images as symlinks under `output/runs/<run-id>/images/`, preserving input bytes; validate link targets independently.

## Phases and acceptance criteria

| Phase | Work | Acceptance evidence |
| --- | --- | --- |
| A. Baseline and dataset inventory | Hash inputs, inspect formats/HDU counts/shapes/headers, run a dry-run on the supplied directory | Input hashes unchanged; baseline report retained; all candidate outcomes accounted for |
| B. Explicit policy and recursion | Separate heuristic TOML from operational configuration; add `--heuristics`; expose and test `--recursive`; make clipping-vs-keep decision terms and reasons observable | Invalid/missing policy rejected clearly; CLI help documents recursive traversal; nested/flat tests pass; every accepted mask reports expected utility/evidence and rejected candidates report why retained |
| C. FITS regression matrix | Add compressed `.fits.gz` and tile-compressed suffix cases, multi-HDU, pre-existing BLANK, all supported BITPIX, scaling, malformed/truncated data, small/odd sizes, output collisions and unchanged inputs | Unsupported compressed/multi-HDU inputs are ignored without science products; storage/WCS/checksum invariants pass; summary counts balance |
| D. Independent interoperability | Validate produced science/mask FITS with Astropy and `fitsverify`; add a reusable checker and verify structural cards, data, checksums, WCS values, NaN/BLANK behavior | All test outputs and representative dataset outputs pass independent structural and checksum validation; any unsupported checker condition is documented |
| E. Real-set run and benchmark | Run serial and bounded-parallel dry runs on the exact supplied set; measure elapsed time, files/s, pixels/s and peak RSS; compare per-file decisions and masks | Reproducible benchmark report records host, software versions, settings and counts; serial and parallel decisions are identical; memory stays within the configured budget plus documented process overhead |
| F. Reconcile and commit | Update README/docs/help/skills/memory/validation record, inspect outputs and git diff, commit code/docs/tests only | Output artifacts remain outside Git; repository clean after a scoped local commit; open scientific limitations remain explicit |

## Decision policy to implement

The policy must distinguish the candidate mask from the final keep/clip choice. The implementation will expose interpretable evidence components rather than invent calibrated probabilities. An artifact score is based on robust contrast, lower or otherwise coherent tile noise, spatial continuity, and border contact. A valid-pixel-loss penalty increases with proposed area and weak/conflicting evidence. Configurable minimum evidence, minimum affected area, maximum affected area, and minimum utility margin determine `clip`; otherwise retain the image and report `kept/review_required` with the candidate fraction and evidence. Never extend a mask beyond the supported candidate component merely to increase clipped area. This preserves the user's preference for maximizing artifact removal subject to minimizing loss of valid science pixels.

Thresholds remain provisional until independently labeled instrument examples and held-out evaluation exist. Crop products are derived outputs only; input files are never rewritten.
