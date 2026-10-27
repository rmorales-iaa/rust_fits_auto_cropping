# Configuration reference

Operational settings live in `input/config.toml`. Scientific decision thresholds live separately in `input/heuristics.toml`; the CLI accepts an alternate rule file with `--heuristics PATH`. A missing explicitly selected file is an error. Unknown TOML keys are rejected.

## Processing configuration: `input/config.toml`

| Key | Default | Meaning |
| --- | ---: | --- |
| `processing.threads` | 0 | 0 selects available logical CPUs; workers are capped by the memory budget. |
| `processing.recursive` | false | Search subdirectories. `--recursive` enables this for a run. |
| `processing.memory_budget_mib` | 2048 MiB | Shared estimated image workspace used to cap concurrent workers. |
| `processing.maximum_image_mib` | 512 MiB | Per-image allocation cap. Current preflight estimate is 32 bytes per pixel. |
| `processing.mode` | `mask` | `mask` preserves dimensions and marks candidate pixels undefined; `crop` compares the maximum-area clean rectangle and largest artifact-free circle, then selects the geometry retaining more pixel centers. A circle is stored in its rectangular bounding box, with pixels outside the circle undefined. CLI `--mode mask|crop` overrides this setting. |

CLI overrides: `--threads`, `--memory-budget-mib` and `--recursive`. `--png` is an opt-in crop-mode display output, not a configuration key. Each invocation creates a new isolated run directory; `--overwrite` only permits collisions inside that run and does not remove older runs.

## Decision rules: `input/heuristics.toml`

| Key | Default | Meaning |
| --- | ---: | --- |
| `heuristics.tile_size` | 16 px | Tile width for robust median/MAD evidence. Smaller tiles can preserve boundary pixels but need adequate samples and cost more CPU. |
| `heuristics.minimum_border_tiles` | 2 | Minimum edge-connected candidate tiles. |
| `heuristics.minimum_abs_z_score` | 8 | Minimum absolute tile-median contrast in robust scale units. |
| `heuristics.maximum_tile_noise_ratio` | 0.20 | Low-noise support threshold relative to interior tile dispersion. |
| `heuristics.minimum_spatial_correlation` | 0.95 | Lag-one correlation support threshold. Constant-tile correlation is undefined. |
| `heuristics.minimum_mask_fraction` | 0.005 | Minimum pixel fraction of a border-connected candidate. |
| `heuristics.maximum_mask_fraction` | 0.50 | Maximum candidate fraction; larger components are retained for review. |
| `heuristics.minimum_evidence_ratio` | 1.25 | Candidate component median contrast divided by `minimum_abs_z_score` required before considering a clip. |
| `heuristics.maximum_evidence_ratio` | 5.0 | Cap on the diagnostic evidence ratio so a single extreme tile cannot dominate the decision. |
| `heuristics.valid_pixel_loss_cost` | 1.5 | Penalty weight for potentially valid science pixels lost. |
| `heuristics.minimum_net_utility` | 0.001 | Minimum benefit-minus-loss score required to clip. |

For candidate fraction `f` and capped evidence ratio `e`, the current diagnostic decision computes artifact benefit `f*e`, valid-pixel loss `cost*f/(1-f)`, and net utility `benefit-loss`. It clips only if minimum size, maximum size, minimum evidence and minimum utility rules all pass. The score is a decision index, **not a calibrated probability or scientific loss estimate**. No rule expands the mask beyond the border-connected candidate. These defaults are provisional and must be calibrated with labeled, held-out frames before production use.
