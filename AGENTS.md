# Project instructions

Always work and respond in English.

Read `input/doc/skills/scientific-fits-engineering/SKILL.md` before project work. It is the authoritative project contract. Read relevant sibling specialty skills when needed; no delegation is required.

Documentation lives in `input/doc/`. Runtime configuration lives in `input/`. Default science products and run reports live in `output/`; logs live in `output/logger/` with UTC ISO timestamps in entries and filenames. An explicit output directory becomes the output root.

Keep the root README current. Tool-required Cargo files remain at the project root. Preserve unrelated work and automatically commit each verified, coherent change locally. Never push unless requested.

The implementation follows `input/doc/implementation-plan.md`. Detection decisions use image pixel statistics; header cards do not serve as evidence, though structural/storage metadata is required to decode FITS samples and null values. The Rust CLI supports single-HDU 2D integer and floating FITS images in dimension-preserving mask mode or opt-in crop mode, which selects the higher-area artifact-free rectangle/circle and applies the configured sacrifice-versus-removal utility. Unchanged eligible images are symlinks under `output/images/`; never modify their sources. Generated FITS products record program/version and UTC creation provenance. The tracked version template is `input/program-version.txt`, and ignored build state is `output/program-version.txt`. Compressed FITS inputs are ignored. Check branches/remotes before authorized merge or push; never invent a remote URL. Do not claim the detector scientifically validated until it passes held-out observatory data review; threshold calibration remains outstanding.
