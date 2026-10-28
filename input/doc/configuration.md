# Configuration reference

Operational settings live in `input/config.toml`. Scientific decision thresholds live separately in `input/heuristics.toml`; the CLI accepts an alternate rule file with `--heuristics PATH`. A missing explicitly selected file is an error. Unknown TOML keys are rejected.

## Processing configuration: `input/config.toml`

| Key | Default | Meaning |
| --- | ---: | --- |
| `processing.threads` | 0 | 0 selects available logical CPUs; workers are capped by the memory budget. |
| `processing.recursive` | false | Search subdirectories. `--recursive` enables this for a run. |
| `processing.memory_budget_mib` | 2048 MiB | Shared estimated image workspace used to cap concurrent workers. |
| `processing.maximum_image_mib` | 512 MiB | Per-image estimated workspace limit using 32 bytes per pixel; this is not a measured peak-allocation or process-RSS cap. |
| `processing.mode` | `mask` | `mask` preserves dimensions and marks candidate pixels undefined; `crop` writes the exact maximum-area axis-aligned rectangle containing no detector-proposed artifacts or originally undefined samples. The rectangle is optimal over the full image mask, with no circle constraint. FITS `NAXIS1/2` match its width/height. CLI `--mode mask` or `--mode crop` overrides this setting. |

CLI overrides: `--threads`, `--memory-budget-mib` and `--recursive`. `--png` is an opt-in crop-mode display output, not a configuration key. Each invocation creates a new isolated run directory; `--overwrite` only permits collisions inside that run and does not remove older runs.

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
| `heuristics.maximum_crop_sacrifice_fraction` | 0.25 | Legacy fallback for heuristic files without the component-count table. |
| `heuristics.crop_sacrifice_fraction_by_border_component_count.one` | 0.25 | Crop sacrifice cap for one accepted contiguous artifact area touching an image border. |
| `.two` | 0.30 | Cap for two accepted border-touching areas. |
| `.three` | 0.35 | Cap for three accepted border-touching areas. |
| `.four_or_more` | 0.41 | Cap for four or more accepted border-touching areas. |

```toml
[heuristics]
# Fallback for older configurations without the tier table.
maximum_crop_sacrifice_fraction = 0.25

[heuristics.crop_sacrifice_fraction_by_border_component_count]
one = 0.25
two = 0.30
three = 0.35
four_or_more = 0.41
```

For candidate fraction `f` and capped evidence ratio `e`, the current diagnostic decision computes artifact benefit `f*e`, valid-pixel loss `cost*f/(1-f)`, and net utility `benefit-loss`. Each disconnected border component must independently pass minimum tile count, minimum/maximum fraction, minimum evidence and minimum utility; aggregate gates also apply to accepted components together. The score is a decision index, **not a calibrated probability or scientific loss estimate**. No rule expands the mask beyond the border-connected candidate. Crop mode is reached only after this mask utility passes; it recomputes loss from additional unmasked geometry sacrifice and chooses the configured cap from the count of accepted border-connected components and keeps the source when its sacrifice exceeds that cap. Each counted component is connected to at least one outer image edge; one component that touches several edges still counts once. The caps are an operational heuristic and have not been calibrated against independent science labels. The [audit](scientific-audit-2026-10-27.md) identifies remaining inconsistency between mask/crop policy and uncalibrated utility. The [active plan](scientific-improvement-plan.md) defines a future versioned, common decision policy; its proposed keys are not supported by this release. Calibrate defaults on development/calibration groups and assess them on untouched held-out frames before production use.
