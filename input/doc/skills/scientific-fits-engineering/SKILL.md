---
name: scientific-fits-engineering
description: Develop scientific FITS-processing projects with documented algorithms, reproducible outputs, ISO logging, tests, bounded parallelism, and automatic local Git commits.
---

# Scientific Fits Engineering

Work and respond in English. Apply this contract to scientific FITS projects.

## Project contract
- Documentation belongs in `input/doc/`; maintain a root `README.md` for remote repository readers.
- Runtime configuration belongs in `input/`. Tool-required manifests and lockfiles (for example Cargo.toml and Cargo.lock) remain in their standard locations; document exceptions.
- Derived data belongs in `output/` by default. An explicitly selected output directory becomes the output root. Isolate each run under `runs/<run-id>/` with images, optional previews, per-input completion manifests, reports and its ISO-named activity log together. Binary mask/reason-mask trees may be used as transient write/verification sidecars but must be removed before final reports and process exit; persistent manifests/reports must not point to them. Validate the output root, managed paths and relative components; reject symlinked paths and input/output overlap before product writes. Publish each completion marker atomically only after retained products pass verification; include SHA-256 for the source and every published product, and invalidate an old marker before explicit overwrite. A missing marker means incomplete or failed products. Sync staged file data and publication directories where the platform supports it, while documenting that this does not make multi-file publication transactional or guarantee device-level power-loss durability. Preserve older runs and unrelated root files; never purge outputs implicitly. Store build state under ignored `output/.state/`.
- Implement a central structured activity logger. Use UTC ISO-8601 timestamps in every entry and ISO-derived safe filenames, e.g. `2026-10-27T10-30-00.123Z_<run-id>.jsonl`. Record invocation, configuration hash, source/version, input identity, stages, decisions, warnings, failures, outputs, durations, resources and completion. Include administrative work when practical. Never record secrets. Flush on orderly termination and report logging failures.
- Keep each substantial class or Rust struct with its implementation in one focused, documented file. Small companion enums/types may remain together. Prefer files below approximately 300 lines; split by responsibility, not arbitrary line count.
- Maintain comprehensive CLI help, README, architecture, algorithm, configuration and validation documentation, and a .gitignore excluding generated products, raw local data, credentials and caches.
- For pixel-artifact detection, derive classification evidence from image samples and mask geometry, not descriptive header cards. Header/WCS metadata limitations must be reported but cannot alone skip a decodable image; when a limitation blocks safe crop geometry, apply accepted detections as a dimension-preserving mask. Use FITS structural, scaling and null metadata only as required to decode samples. In normal runs, create one `images/` entry per discovered FITS input: accepted products are generated, and unchanged, malformed or unsupported inputs are annotated and preserved as symlinks to canonical sources. Never write provenance through symlinks. Dry-run intentionally produces no image entries. Add `CREATOR`, UTC `DATE` and timestamped `HISTORY` only to generated FITS products, and record run version/time in summary and logs.
- Crop comparison previews must estimate an independent ZScale interval from the exact final crop samples; source and crop display stretches may differ. Preview resampling is display-only and must not alter FITS samples.
- Carry the parsed FITS source snapshot used for detection through product writing and verification. Do not reopen a mutable source path to obtain output pixels or expected retained values; this can make the action differ from the classified frame. Keep the unchanged-source symlink path separate.
- Hash eligible source bytes during parsing through the same open handle, including FITS padding skipped by forward seeks; record the digest in the per-image report and completion manifest. Recheck a kept-source symlink before publishing its marker so it cannot certify a different source generation.
- Keep reports explicit about decisions versus products: proposed exclusions and applied exclusions are separate counts; dry-run proposals must never be counted as completed clipping. Crop geometry sacrifices and original undefined pixels need separate accounting before any metric is labeled valid-area loss. A larger proposal mask alone is not an improvement.
- This project uses a tracked `input/program-version.txt` template and local ignored `output/.state/program-version.txt` counter. Cargo package recompilations advance the three-part base-100 version. Check current branches/remotes before authorized merge/push; never invent a remote URL.

## Scientific improvement reviews

In rust_fits_auto_cropping, read the project memory, latest scientific audit and active scientific improvement plan under `input/doc/` before extending algorithms. Historical plans and passing tests are not proof that every stated invariant is implemented. Distinguish proposed artifact pixels, accepted exclusions, original undefined samples, geometric sacrifices and independently labeled valid/invalid science. Optimize valid retention and artifact removal under declared risk and source-preservation limits; never maximize mask size as a proxy for quality. Freeze data splits, estimands, tolerances and meaningful improvement thresholds before tuning. A planning review ends with an auditable plan and explicit unresolved implementation/data gates, not a scientific certification.

## Work cycle
1. Explore existing code and Git state before editing. Preserve unrelated work.
2. Before complex work, save a phased implementation plan in `input/doc/` with assumptions, acceptance criteria and tests. When the user asks for implementation, continue from that plan, track evidence against each phase and update its status; do not stop after planning.
3. Specify units, coordinates, time scales, numerical precision, uncertainty, failure modes and a reproducible validation dataset. Cite primary references; never call an algorithm scientifically validated without evidence.
4. Implement the smallest complete phase. Preserve originals, use atomic output writes and fail conservatively on ambiguous scientific decisions.
5. Apply efficient I/O, allocation reuse and bounded parallelism where safe and beneficial. Benchmark serial and parallel paths; preserve deterministic science results and cap memory. Do not parallelize unsafe library handles.
6. Add substantial unit, integration, property, edge-case, synthetic-science and regression tests. Include independent reference checks, serial/parallel equivalence and end-to-end error handling.
7. Run relevant tests and quality checks, inspect the diff, and automatically commit each verified coherent change to the local repository. Initialize Git if absent. Include documentation and tests; exclude secrets, raw data, logs and outputs. Do not push or rewrite history without explicit authorization. Never invent author identity; report a genuine commit blocker.
8. Before declaring implementation complete, map every plan requirement to code and validation evidence. Report incomplete or deferred items honestly, along with commit and output locations.

## Supporting skills
Read the relevant sibling skill in this bundle when its specialty applies:
- [scientific-file-exploration](../scientific-file-exploration/SKILL.md): repository mapping and focused discovery.
- [scientific-local-git](../scientific-local-git/SKILL.md): automatic local change registration.
- [scientific-astronomer](../scientific-astronomer/SKILL.md): observational interpretation.
- [scientific-software-engineer](../scientific-software-engineer/SKILL.md): Rust architecture and reliability.
- [scientific-photometry](../scientific-photometry/SKILL.md): flux preservation and background bias.
- [scientific-astrometry](../scientific-astrometry/SKILL.md): pixel coordinates, WCS and distortion.
- [scientific-tno-analysis](../scientific-tno-analysis/SKILL.md): moving and faint targets.
- [scientific-validation-performance](../scientific-validation-performance/SKILL.md): recommended numerical validation and measured optimization.
These are expertise workflows; they do not require spawning agents. Load only the relevant ones.
