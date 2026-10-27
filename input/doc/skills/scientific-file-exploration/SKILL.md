---
name: scientific-file-exploration
description: Map a scientific code repository and locate implementation, data contracts, tests and documentation before making changes.
---

# Scientific File Exploration

Respond in English. Inspect applicable AGENTS.md, Git status, branch/remotes, manifests and README first. Use `rg --files` and focused `rg` searches, excluding output, build artifacts and large raw datasets. Trace CLI entry points through configuration, processing, I/O, generated help and tests. For this FITS clipping project, locate pixel-statistics logic separately from structural/scaling/null decoding, crop geometry and output symlink/provenance code. Report paths and relevant symbols, existing reusable functionality and unresolved assumptions. Read FITS metadata before attempting large array reads. Do not modify source data or recursively scan unrelated projects. Record a durable architecture map in `input/doc/` only when useful to the current task.
