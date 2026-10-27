# Rust FITS Auto Clipping

Planned Rust command-line tool to identify non-scientific border regions in single-HDU FITS images and write new images while preserving valid science pixels and relevant metadata.

**Status:** planning only. No Rust application or detector has been implemented or scientifically validated.

The design supports asymmetric corners/sides, circular, rectangular and partly symmetric instrument boundaries. Conservative masking preserves the image grid; optional rectangular cropping requires WCS updates. Ambiguous detections remain unchanged and are reported for review. Multi-HDU files are ignored.

## Documentation

- [Implementation plan and acceptance gates](input/doc/implementation-plan.md)
- [Reusable project skill](input/doc/skills/scientific-fits-engineering/SKILL.md)
- [Skill catalog and installation](input/doc/skill-catalog.md)
- [Primary technical references](input/doc/references.md)

## Project layout

- `input/`: runtime configuration and optional local data in `input/data/`.
- `input/doc/`: methods, plans, help, validation procedures and versioned skills.
- `output/`: default generated images, masks, diagnostic and summary products.
- `output/logger/`: UTC ISO-timestamped activity logs.
- `src/`, `tests/`, `benches/`: planned implementation, tests and benchmarks.

An explicit output path becomes the output root, including logs and reports. Cargo manifests and lockfiles will remain at the project root. Raw observations, generated products, logs and credentials are excluded from Git.

## Planned interface (not yet executable)

```text
fits-auto-clipping --input-dir input/data --output-dir output --config input/config.toml
```

The plan defines configuration, masking versus cropping, validation, reporting and exit behavior. Build/install commands, full CLI examples, configuration reference and measured performance will be added with implementation. Development uses scoped local commits; no remote is configured or published by this setup.
