//! Runtime and science-decision configuration, intentionally stored separately.
use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub processing: ProcessingConfig,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessingConfig {
    pub threads: usize,
    pub recursive: bool,
    pub memory_budget_mib: usize,
    pub maximum_image_mib: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HeuristicsConfig {
    pub heuristics: HeuristicRules,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HeuristicRules {
    pub tile_size: usize,
    pub minimum_border_tiles: usize,
    pub minimum_abs_z_score: f64,
    pub maximum_tile_noise_ratio: f64,
    pub minimum_spatial_correlation: f64,
    pub minimum_mask_fraction: f64,
    pub maximum_mask_fraction: f64,
    pub minimum_evidence_ratio: f64,
    pub maximum_evidence_ratio: f64,
    pub valid_pixel_loss_cost: f64,
    pub minimum_net_utility: f64,
}

impl Config {
    pub fn load(path: Option<&Path>) -> anyhow::Result<(Self, String)> {
        let selected = path.map(Path::to_path_buf).or_else(|| {
            Path::new("input/config.toml")
                .is_file()
                .then(|| Path::new("input/config.toml").to_path_buf())
        });
        let text = read_config(selected.as_deref(), DEFAULT_CONFIG, "input/config.toml")?;
        let config: Self =
            toml::from_str(&text).context("parsing processing TOML configuration")?;
        config.validate()?;
        Ok((config, text))
    }

    fn validate(&self) -> anyhow::Result<()> {
        if self.processing.maximum_image_mib == 0
            || self.processing.memory_budget_mib < self.processing.maximum_image_mib
        {
            bail!(
                "memory_budget_mib must be at least maximum_image_mib, and both must be positive"
            );
        }
        Ok(())
    }
}

impl HeuristicsConfig {
    pub fn load(path: Option<&Path>) -> anyhow::Result<(Self, String)> {
        let text = read_config(path, DEFAULT_HEURISTICS, "input/heuristics.toml")?;
        let config: Self = toml::from_str(&text).context("parsing heuristic TOML configuration")?;
        config.validate()?;
        Ok((config, text))
    }

    fn validate(&self) -> anyhow::Result<()> {
        let h = &self.heuristics;
        if !(4..=65_536).contains(&h.tile_size) || h.minimum_border_tiles == 0 {
            bail!(
                "heuristics.tile_size must be in [4, 65536] and minimum_border_tiles must be positive"
            );
        }
        if !h.minimum_abs_z_score.is_finite() || h.minimum_abs_z_score <= 0.0 {
            bail!("heuristics.minimum_abs_z_score must be finite and positive");
        }
        if !(0.0..=1.0).contains(&h.maximum_tile_noise_ratio)
            || !(0.0..=1.0).contains(&h.minimum_spatial_correlation)
        {
            bail!("heuristics noise ratio and spatial correlation must be in [0, 1]");
        }
        if !(0.0..=1.0).contains(&h.minimum_mask_fraction)
            || !(0.0..=1.0).contains(&h.maximum_mask_fraction)
            || h.minimum_mask_fraction > h.maximum_mask_fraction
        {
            bail!("heuristics mask fractions must satisfy 0 <= minimum <= maximum <= 1");
        }
        if !h.minimum_evidence_ratio.is_finite()
            || h.minimum_evidence_ratio < 1.0
            || !h.maximum_evidence_ratio.is_finite()
            || h.maximum_evidence_ratio < h.minimum_evidence_ratio
            || !h.valid_pixel_loss_cost.is_finite()
            || h.valid_pixel_loss_cost < 0.0
            || !h.minimum_net_utility.is_finite()
            || h.minimum_net_utility < 0.0
        {
            bail!("heuristic evidence, loss-cost and utility values are invalid");
        }
        Ok(())
    }
}

fn read_config(path: Option<&Path>, default: &str, default_path: &str) -> anyhow::Result<String> {
    if let Some(path) = path {
        return fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()));
    }
    if Path::new(default_path).is_file() {
        fs::read_to_string(default_path).with_context(|| format!("reading {default_path}"))
    } else {
        Ok(default.to_owned())
    }
}

const DEFAULT_CONFIG: &str = r#"
[processing]
threads = 0
recursive = false
memory_budget_mib = 2048
maximum_image_mib = 512
"#;

const DEFAULT_HEURISTICS: &str = r#"
[heuristics]
tile_size = 16
minimum_border_tiles = 2
minimum_abs_z_score = 8.0
maximum_tile_noise_ratio = 0.20
minimum_spatial_correlation = 0.95
minimum_mask_fraction = 0.005
maximum_mask_fraction = 0.50
minimum_evidence_ratio = 1.25
maximum_evidence_ratio = 5.0
valid_pixel_loss_cost = 1.5
minimum_net_utility = 0.001
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_configuration_keys() {
        let text = DEFAULT_HEURISTICS.replace("[heuristics]", "[heuristics]\nunexpected = true");
        assert!(toml::from_str::<HeuristicsConfig>(&text).is_err());
    }

    #[test]
    fn rejects_invalid_mask_fraction_order() {
        let text = DEFAULT_HEURISTICS.replace(
            "minimum_mask_fraction = 0.005",
            "minimum_mask_fraction = 0.75",
        );
        let c: HeuristicsConfig = toml::from_str(&text).unwrap();
        assert!(c.validate().is_err());
    }

    #[test]
    fn rejects_crop_setting_as_unknown_processing_option() {
        let text = DEFAULT_CONFIG.replace("threads = 0", "threads = 0\nmode = \"crop\"");
        assert!(toml::from_str::<Config>(&text).is_err());
    }

    #[test]
    fn missing_explicit_heuristic_file_is_an_error() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("missing.toml");
        assert!(HeuristicsConfig::load(Some(&path)).is_err());
    }
}
