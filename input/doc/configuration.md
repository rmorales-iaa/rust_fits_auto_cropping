# Configuration reference

Operational settings live in `input/config.toml`. Scientific decision thresholds live separately in `input/heuristics.toml`; the CLI accepts an alternate rule file with `--heuristics PATH`. A missing explicitly selected file is an error. Unknown TOML keys are rejected.

## Processing configuration: `input/config.toml`

| Key | Default | Meaning |
| --- | ---: | --- |
| `processing.threads` | 4 | Worker-thread ceiling. Set to `0` to request all available logical CPUs. The final count is capped by CPUs available to the process and the configured memory budget. |
| `processing.recursive` | false | Search subdirectories. `--recursive` enables this for a run. |
| `processing.memory_budget_mib` | 8192 MiB | Shared estimated image workspace used to cap concurrent workers. |
| `processing.maximum_image_mib` | 1536 MiB | Per-image estimated workspace limit using 40 bytes per pixel to budget the crop search's two integral-count planes; this is not a measured peak-allocation or process-RSS cap. Sized to admit a 7100×5325 image (about 1,441 MiB by this estimate). |
| `processing.mode` | `mask` | `mask` preserves dimensions and marks candidate pixels undefined; `crop` selects a high-area axis-aligned rectangle under residual-pattern, valid-pixel sacrifice, and utility limits. Originally undefined samples are always excluded. FITS `NAXIS1/2` match the selected rectangle. CLI `--mode mask` or `--mode crop` overrides this setting. |

CLI overrides: `--threads`, `--memory-budget-mib` and `--recursive`. Processing runs in parallel through a bounded Rayon worker pool. `processing.threads = 4` is the default; `0` requests all available logical CPUs, subject to the CPU quota and per-run memory limit. The run log records the configured value and effective worker count. `--png` is an opt-in crop-mode display output, not a configuration key. By default, each invocation creates a new isolated run directory; `--flat-output` places products at the selected root. `--overwrite` allows product collisions to be replaced, and `--sanitize-output` deletes all existing root contents before processing.

## Decision rules: `input/heuristics.toml`

| Key | Default | Meaning |
| --- | ---: | --- |
| `heuristics.reference_model` | `central_tiles` | `central_tiles` is the current baseline. `robust_affine` is an experimental Huber-fit plane over finite interior tile medians; it is opt-in, has no source mask or uncertainty map, and is not calibrated. |
| `heuristics.evidence_model` | `single_scale` | `single_scale` preserves the baseline. `multiscale_consensus` runs up to three bounded tile widths and requires every proposed pixel to be selected at two or more scales; it is experimental and adds detector work. |
| `heuristics.tile_size` | 16 px | Tile width for robust median/MAD evidence. Smaller tiles can preserve boundary pixels but need adequate samples and cost more CPU. |
| `heuristics.minimum_border_tiles` | 2 | Minimum candidate tiles required independently for each edge-connected component. |
| `heuristics.minimum_abs_z_score` | 8 | Minimum absolute tile-median contrast in robust scale units. |
| `heuristics.maximum_tile_noise_ratio` | 0.20 | Low-noise support threshold relative to the maximum central-tile MAD sigma, not a robust typical noise estimate. |
| `heuristics.minimum_spatial_correlation` | 0.95 | Lag-one correlation support threshold. Constant-tile correlation is undefined. |
| `heuristics.minimum_mask_fraction` | 0.005 | Minimum pixel fraction of a border-connected candidate. |
| `heuristics.maximum_mask_fraction` | 0.50 | Maximum candidate fraction; larger components are retained for review. |
| `heuristics.minimum_evidence_ratio` | 1.25 | Aggregate selected-tile median contrast divided by `minimum_abs_z_score` required before considering a clip. |
| `heuristics.maximum_evidence_ratio` | 5.0 | Cap on the diagnostic evidence ratio so a single extreme tile cannot dominate the decision. |
| `heuristics.valid_pixel_loss_cost` | 1.5 | Penalty weight for potentially valid science pixels lost. |
| `heuristics.minimum_net_utility` | 0.001 | Minimum benefit-minus-loss score required to clip. |
| `heuristics.maximum_residual_artifact_fraction` | 0.05 in shipped file | Maximum fraction of defined pixels inside a crop that may be detector-flagged and retained. Zero requests a strict clean rectangle. Provisional and not calibrated. |
| `heuristics.maximum_crop_sacrifice_fraction` | 0.35 | Legacy fallback for heuristic files without the component-count table. |
| `heuristics.crop_sacrifice_fraction_by_border_component_count.one` | 0.35 | Crop sacrifice cap for one accepted contiguous artifact area touching an image border. |
| `.two` | 0.35 | Cap for two accepted border-touching areas. |
| `.three` | 0.35 | Cap for three accepted border-touching areas. |
| `.four_or_more` | 0.47 | Cap for four or more accepted border-touching areas. |

```toml
[heuristics]
# Fallback for older configurations without the tier table.
maximum_crop_sacrifice_fraction = 0.35

[heuristics.crop_sacrifice_fraction_by_border_component_count]
one = 0.35
two = 0.35
three = 0.35
four_or_more = 0.47
```

For candidate fraction `f` and capped evidence ratio `e`, the current detector computes artifact benefit `f*e`, valid-pixel loss `cost*f/(1-f)`, and net utility `benefit-loss`. Each disconnected border component must independently pass minimum tile count, minimum/maximum fraction, minimum evidence and minimum utility; aggregate gates also apply to accepted components together. The score is a decision index, **not a calibrated probability or scientific loss estimate**. Crop mode uses the exact largest clean rectangle as its anchor, then evaluates a deterministic bounded family of rectangles expanded from that anchor. The shipped residual ceiling is 5%; legacy custom heuristic files that omit the key default to zero residual, preserving strict behavior. Candidates must contain no original undefined sample and pass the configured component-count sacrifice and utility limits. Search stops at 100,000 candidate rectangles per image and reports the evaluation count and pixel step. The result is maximum-area only within the evaluated expansion family, not a global optimum over all residual-tolerant rectangles. These thresholds are not calibrated against independent science labels; test on untouched held-out frames before production use.

Malformed non-structural source header cards are never repaired or normalized. Same-width placeholders exist only in the parser's temporary in-memory view; the original 80-byte cards are restored byte-for-byte in generated FITS headers and the issue is recorded in the per-image reason. Pixel processing continues. Valid structural layout values (`BITPIX`, `NAXIS*`, and related data-size fields) are still required to locate the image samples safely.
