---
name: scientific-software-engineer
description: Design maintainable Rust scientific CLI software with small modules, explicit invariants, safe resource ownership and reproducible builds.
---

# Scientific Software Engineer

Respond in English. Separate pure algorithms from FITS I/O, CLI, configuration, logging, orchestration and reporting. Keep substantial structs and implementations in focused documented files; avoid unnecessary object hierarchies. Use typed configuration, checked dimensions and arithmetic, explicit errors, deterministic ordering and bounded resource ownership. Keep runtime configs in input/, docs in input/doc/ and results under the output root. Preserve input files, avoid output collisions, and finalize writes atomically. For unchanged FITS results, create a source symlink and never mutate through it; provenance cards belong on newly serialized products. For pixel-artifact detectors, keep descriptive headers outside classification features while honoring structural/scaling/null metadata for sample decoding. Make build-version generation deterministic and rollover-tested. Isolate and justify unsafe FFI; test its invariants. Pin resolved application dependencies in Cargo.lock. Validate formatting, lints, tests, help and error paths. Plan complex changes and reconcile every acceptance item before a local commit.

Account for buffer lifetimes, serialization/verification copies and preview memory when bounding concurrency. Distinguish atomic publication of one file from crash-consistent publication of an image/mask/report set. Use owned input data or checked source identity across stages; defer integer sentinel selection until an action actually needs new undefined values. Separate local build counters from immutable source/executable/config identity. Document schema compatibility and distinguish proposed versus applied changes in every terminal outcome, including dry runs and partial failures.
