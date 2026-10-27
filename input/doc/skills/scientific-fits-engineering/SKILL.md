---
name: scientific-fits-engineering
description: Develop scientific FITS-processing projects with documented algorithms, reproducible outputs, ISO logging, tests, bounded parallelism, and automatic local Git commits.
---

# Scientific Fits Engineering

Work and respond in English. Apply this contract to scientific FITS projects.

## Project contract
- Documentation belongs in `input/doc/`; maintain a root `README.md` for remote repository readers.
- Runtime configuration belongs in `input/`. Tool-required manifests and lockfiles (for example Cargo.toml and Cargo.lock) remain in their standard locations; document exceptions.
- Derived data belongs in `output/` by default. An explicitly selected output directory becomes the output root, including its `logger/` subdirectory.
- Implement a central structured activity logger. Use UTC ISO-8601 timestamps in every entry and ISO-derived safe filenames, e.g. `2026-10-27T10-30-00.123Z_<run-id>.jsonl`. Record invocation, configuration hash, source/version, input identity, stages, decisions, warnings, failures, outputs, durations, resources and completion. Include administrative work when practical. Never record secrets. Flush on orderly termination and report logging failures.
- Keep each substantial class or Rust struct with its implementation in one focused, documented file. Small companion enums/types may remain together. Prefer files below approximately 300 lines; split by responsibility, not arbitrary line count.
- Maintain comprehensive CLI help, README, architecture, algorithm, configuration and validation documentation, and a .gitignore excluding generated products, raw local data, credentials and caches.

## Work cycle
1. Explore existing code and Git state before editing. Preserve unrelated work.
2. Before complex work, save a phased implementation plan in `input/doc/` with assumptions, acceptance criteria and tests. Track evidence against each phase. A planning-only request stops after delivering the plan.
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
