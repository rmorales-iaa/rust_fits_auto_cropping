---
name: scientific-file-exploration
description: Map a scientific code repository and locate implementation, data contracts, tests and documentation before making changes.
---

# Scientific File Exploration

Respond in English. Inspect applicable AGENTS.md, Git status, manifests and README first. Use `rg --files` and focused `rg` searches, excluding output, build artifacts and large raw datasets. Trace CLI entry points through configuration, processing, I/O and tests. Report paths and relevant symbols, existing reusable functionality and unresolved assumptions. Read FITS metadata before attempting large array reads. Do not modify source data or recursively scan unrelated projects. Record a durable architecture map in `input/doc/` only when useful to the current task.
