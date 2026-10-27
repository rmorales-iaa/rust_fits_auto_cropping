---
name: scientific-validation-performance
description: Validate scientific numerical behavior and benchmark safe parallel execution with reproducible fixtures and independent checks.
---

# Scientific Validation Performance

Respond in English. Define measurable acceptance gates before tuning. Use seeded synthetic ground truth and separately held-out real examples; avoid fitting and assessing on the same frames. Quantify false removal of valid science pixels, artifact recall, boundary error and uncertainty across instruments, shapes and noise levels. Include null images and confounders. Check pixel identity, metadata/WCS invariants, generated FITS CREATOR/DATE/HISTORY, kept-image symlink targets, reproducibility, serial/parallel equivalence and fault recovery. Cross-check FITS interoperability with an independent reader and fitsverify when available. For optional FITS visualizations, independently compare display scaling, panel geometry, orientation and representative samples against a reference implementation. Benchmark wall time, throughput, peak memory and I/O across image sizes and worker counts with recorded hardware/configuration. Optimize measured bottlenecks without weakening science thresholds; parallelize independent files with memory bounds and verified library thread safety. Document results under output/ and the reproducible procedure in `input/doc/`. Mark empirical claims unvalidated until evidence exists.
