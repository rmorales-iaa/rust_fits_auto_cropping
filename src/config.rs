//! Typed detector and processing settings, loaded before any science files are opened.
use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub detector: DetectorConfig,
    pub processing: ProcessingConfig,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DetectorConfig {
    pub tile_size: usize,
    pub minimum_border_tiles: usize,
    pub minimum_abs_z_score: f64,
    pub maximum_tile_noise_ratio: f64,
    pub minimum_spatial_correlation: f64,
    pub minimum_mask_fraction: f64,
    pub maximum_mask_fraction: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessingConfig {
    pub threads: usize,
    pub recursive: bool,
    pub memory_budget_mib: usize,
    pub maximum_image_mib: usize,
    pub mode: String,
}

impl Config {
    pub fn load(path: Option<&Path>) -> anyhow::Result<(Self, String)> {
        let selected = path.map(Path::to_path_buf).or_else(|| {
            Path::new("input/config.toml")
                .is_file()
                .then(|| Path::new("input/config.toml").to_path_buf())
        });
        let text = match selected.as_deref() {
            Some(p) => {
                fs::read_to_string(p).with_context(|| format!("reading config {}", p.display()))?
            }
            None => DEFAULT_CONFIG.to_owned(),
        };
        let config: Self = toml::from_str(&text).context("parsing TOML configuration")?;
        config.validate()?;
        Ok((config, text))
    }

    fn validate(&self) -> anyhow::Result<()> {
        let d = &self.detector;
        if d.tile_size < 4 || d.tile_size > 65_536 || d.minimum_border_tiles == 0 {
            bail!("tile_size must be in [4, 65536] and minimum_border_tiles must be positive");
        }
        if !d.minimum_abs_z_score.is_finite() || d.minimum_abs_z_score <= 0.0 {
            bail!("minimum_abs_z_score must be finite and positive");
        }
        if !(0.0..=1.0).contains(&d.maximum_tile_noise_ratio)
            || !(0.0..=1.0).contains(&d.minimum_spatial_correlation)
        {
            bail!("maximum_tile_noise_ratio and minimum_spatial_correlation must be in [0, 1]");
        }
        if !(0.0..=1.0).contains(&d.minimum_mask_fraction)
            || !(0.0..=1.0).contains(&d.maximum_mask_fraction)
            || d.minimum_mask_fraction > d.maximum_mask_fraction
        {
            bail!("mask fraction thresholds must satisfy 0 <= minimum <= maximum <= 1");
        }
        if self.processing.maximum_image_mib == 0
            || self.processing.memory_budget_mib < self.processing.maximum_image_mib
        {
            bail!(
                "memory_budget_mib must be at least maximum_image_mib, and both must be positive"
            );
        }
        if self.processing.mode != "mask" {
            bail!("only processing.mode = 'mask' is currently supported");
        }
        Ok(())
    }
}

const DEFAULT_CONFIG: &str = r#"
[detector]
tile_size = 32
minimum_border_tiles = 2
minimum_abs_z_score = 8.0
maximum_tile_noise_ratio = 0.20
minimum_spatial_correlation = 0.95
minimum_mask_fraction = 0.01
maximum_mask_fraction = 0.50
[processing]
threads = 0
recursive = false
memory_budget_mib = 2048
maximum_image_mib = 512
mode = "mask"
"#;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_invalid_mask_fraction_order() {
        let text = DEFAULT_CONFIG.replace(
            "minimum_mask_fraction = 0.01",
            "minimum_mask_fraction = 0.75",
        );
        let c: Config = toml::from_str(&text).unwrap();
        assert!(c.validate().is_err());
    }
    #[test]
    fn rejects_unknown_configuration_keys() {
        let text = DEFAULT_CONFIG.replace("[processing]", "unexpected = true\n[processing]");
        assert!(toml::from_str::<Config>(&text).is_err());
    }
    #[test]
    fn rejects_crop_until_geometry_updates_exist() {
        let text = DEFAULT_CONFIG.replace("mode = \"mask\"", "mode = \"crop\"");
        let c: Config = toml::from_str(&text).unwrap();
        assert!(c.validate().is_err());
    }
}
