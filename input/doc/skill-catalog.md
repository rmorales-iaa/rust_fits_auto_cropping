# Skill catalog

The version-controlled source of each skill is `input/doc/skills/<name>/SKILL.md`. Copies were installed into the current user's Codex skills directory for discovery in subsequent sessions. Project AGENTS.md points directly to the versioned source so project instructions also apply without relying on discovery. Keep installed copies synchronized when these sources change; do not overwrite unrelated skills.

Explicit invocation example: `$scientific-fits-engineering`.

| Skill | Purpose |
| --- | --- |
| [scientific-fits-engineering](skills/scientific-fits-engineering/SKILL.md) | Develop scientific FITS-processing projects with documented algorithms, reproducible outputs, ISO logging, tests, bounded parallelism, and automatic local Git commits. |
| [scientific-file-exploration](skills/scientific-file-exploration/SKILL.md) | Map a scientific code repository and locate implementation, data contracts, tests and documentation before making changes. |
| [scientific-local-git](skills/scientific-local-git/SKILL.md) | Register verified scientific project changes as scoped local Git commits while preserving unrelated work and excluding scientific outputs. |
| [scientific-astronomer](skills/scientific-astronomer/SKILL.md) | Review astronomical image processing for physical interpretation, calibration context, instrument artifacts and preservation of observational metadata. |
| [scientific-software-engineer](skills/scientific-software-engineer/SKILL.md) | Design maintainable Rust scientific CLI software with small modules, explicit invariants, safe resource ownership and reproducible builds. |
| [scientific-photometry](skills/scientific-photometry/SKILL.md) | Validate image masks and transformations against astronomical photometric integrity, background estimation and faint-source selection effects. |
| [scientific-astrometry](skills/scientific-astrometry/SKILL.md) | Preserve and validate FITS WCS, pixel-coordinate conventions and astrometric accuracy during masking or cropping. |
| [scientific-tno-analysis](skills/scientific-tno-analysis/SKILL.md) | Assess image preprocessing for TNO and moving-object science, including faint targets, trails, occultation cadence and selection effects. |
| [scientific-validation-performance](skills/scientific-validation-performance/SKILL.md) | Validate scientific numerical behavior and benchmark safe parallel execution with reproducible fixtures and independent checks. |

The validation/performance skill is the additional recommended specialty. Photometry and astrometry are separate to keep their invariants clear. Existing astronomy skills were left unchanged.

For another machine, copy the desired skill folders into `${CODEX_HOME:-$HOME/.codex}/skills/` after checking for name conflicts. The main skill routes to siblings; install the full bundle to retain all specialties. Validate with the skill-creator `quick_validate.py` script when available.

Automatic Git registration applies to verified development milestones; it does not promise a filesystem watcher or version control of ignored scientific data. English is required for responses, help and project documentation.
