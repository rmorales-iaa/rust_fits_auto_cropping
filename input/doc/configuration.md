# Configuration reference

Runtime configuration is TOML in `input/config.toml` by default. No setting silently converts images, rescales retained pixels, or changes the WCS.

| Key | Default | Meaning |
| --- | ---: | --- |
| `detector.tile_size` | 32 px | Square tile width used for robust summaries. Smaller tiles refine boundaries but increase noise and computation. |
| `detector.minimum_border_tiles` | 2 | Minimum edge-connected candidate tiles before a mask can be accepted. |
| `detector.minimum_abs_z_score` | 8 | Minimum absolute tile-median contrast in robust scale units. |
| `detector.maximum_tile_noise_ratio` | 0.20 | Defines a low-noise tile relative to the highest finite interior tile dispersion. |
| `detector.minimum_spatial_correlation` | 0.95 | Lag-one pixel correlation that supports a high-contrast candidate tile. Correlation is undefined for constant tiles. |
| `detector.minimum_mask_fraction` | 0.01 | Reject smaller candidates as non-significant. |
| `detector.maximum_mask_fraction` | 0.50 | Abstain when the connected candidate would mask more of the image. |
| `processing.threads` | 0 | 0 selects the host's logical CPU count; actual workers are capped by the memory budget. |
| `processing.recursive` | false | Search nested input directories. `--recursive` can enable this for a run. |
| `processing.memory_budget_mib` | 2048 MiB | Shared estimated image-workspace budget. Workers are capped by this value. |
| `processing.maximum_image_mib` | 512 MiB | Per-image estimated workspace cap. The estimate is 32 bytes per pixel. Larger images report an error. |
| `processing.mode` | `mask` | Only dimension-preserving masking is currently available. Other values fail configuration validation. |

Fractions refer to the total tile area / image area. Scores are diagnostic and are not calibrated probabilities. Defaults require representative synthetic and real-data validation before production decisions. `--threads`, `--memory-budget-mib`, `--recursive` and `--overwrite` are command-line-only controls.
