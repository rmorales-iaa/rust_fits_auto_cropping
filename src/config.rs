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
    #[serde(default)]
    pub mode: ProcessingMode,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum ProcessingMode {
    #[default]
    Mask,
    Crop,
    /// Tolerant rectangle whose retained detector-flagged samples are set
    /// undefined (decision D2); internal BLANK/NaN samples are allowed.
    CropMasked,
}

impl ProcessingMode {
    /// True for modes that select a rectangular product geometry.
    pub fn is_crop(self) -> bool {
        matches!(self, Self::Crop | Self::CropMasked)
    }
}

/// Action when no crop rectangle passes the sacrifice/utility gates
/// (decision D1). `bbox_mask` crops to the tight bounding box of retained,
/// defined, unflagged samples and marks remaining flagged samples undefined.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CropFallback {
    /// Legacy behaviour for rule files without the key: keep the source.
    #[default]
    Keep,
    Mask,
    BboxMask,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HeuristicsConfig {
    pub heuristics: HeuristicRules,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceModel {
    #[default]
    CentralTiles,
    RobustAffine,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceModel {
    #[default]
    SingleScale,
    MultiscaleConsensus,
}

/// Detector family. `tile_v1` is the default baseline; the v2 background
/// model is experimental and opt-in until it passes the plan's gates.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DetectorKind {
    #[default]
    TileV1,
    BackgroundV2Experimental,
}

/// Uncertain-boundary handling for the v2 pixel mask (plan R3.4).
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UncertainPolicy {
    /// Valid retention first: uncertain band pixels stay in the product.
    #[default]
    RetainUncertain,
    ExcludeUncertain,
}

/// Registered starting values for the v2 detector (plan R2/R3). Thresholds
/// are in standard-error units and remain provisional until calibrated by
/// the null false-alarm procedure (plan R1.2).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct V2Rules {
    pub mesh_size_px: usize,
    pub clip_kappa: f64,
    pub clip_iterations: usize,
    pub trusted_valid_fraction: f64,
    pub min_trusted_meshes: usize,
    pub surface_max_degree: usize,
    pub level_threshold: f64,
    pub dispersion_threshold: f64,
    pub strong_level_threshold: f64,
    /// Rule N: noise excess alone (flat-amplified borders at sky level).
    pub noise_only_threshold: f64,
    /// Relative illumination at the valid boundary (decision D4, eta_V).
    /// Dispersion rules also require the effect |ln(s/sigma)| >=
    /// 0.5 ln(1/eta_valid), the variance ratio of a noise-dominated sky at
    /// that illumination (raw: ratio ~ eta; flat-divided: ~ 1/eta).
    pub eta_valid: f64,
    /// One-sided minimum-effect test: (|ln(s/sigma)| - delta) / SE >= this,
    /// with delta = 0.5 ln(1/eta_valid); the effect must lie significantly
    /// beyond the D4 boundary, not just its point estimate.
    pub effect_margin_z: f64,
    pub source_seed_snr: f64,
    pub source_min_pixels: usize,
    pub source_dilation_fwhm: f64,
    pub refit_iterations: usize,
    pub boundary_min_cv_gain: f64,
    /// Fraction of the profile contrast the fitted discontinuity must carry
    /// (a step is abrupt; hinges and smooth ramps carry almost none).
    pub boundary_min_jump_fraction: f64,
    pub hysteresis_low_fraction: f64,
    pub uncertain_policy: UncertainPolicy,
}

impl Default for V2Rules {
    fn default() -> Self {
        Self {
            mesh_size_px: 64,
            clip_kappa: 3.0,
            clip_iterations: 5,
            trusted_valid_fraction: 0.5,
            min_trusted_meshes: 9,
            surface_max_degree: 2,
            level_threshold: 5.0,
            dispersion_threshold: 4.0,
            strong_level_threshold: 8.0,
            noise_only_threshold: 8.0,
            eta_valid: 0.5,
            effect_margin_z: 2.0,
            source_seed_snr: 5.0,
            source_min_pixels: 5,
            source_dilation_fwhm: 1.5,
            refit_iterations: 3,
            boundary_min_cv_gain: 1.0,
            boundary_min_jump_fraction: 0.5,
            hysteresis_low_fraction: 0.6,
            uncertain_policy: UncertainPolicy::RetainUncertain,
        }
    }
}

impl V2Rules {
    fn validate(&self) -> anyhow::Result<()> {
        let positive = [
            self.clip_kappa,
            self.level_threshold,
            self.dispersion_threshold,
            self.strong_level_threshold,
            self.noise_only_threshold,
            self.source_seed_snr,
            self.source_dilation_fwhm,
        ];
        if positive.iter().any(|v| !v.is_finite() || *v <= 0.0)
            || !(16..=4096).contains(&self.mesh_size_px)
            || !(1..=20).contains(&self.clip_iterations)
            || !(0.0..=1.0).contains(&self.trusted_valid_fraction)
            || !(self.eta_valid > 0.0 && self.eta_valid < 1.0)
            || !self.effect_margin_z.is_finite()
            || self.effect_margin_z < 0.0
            || self.min_trusted_meshes < 4
            || self.surface_max_degree > 2
            || self.source_min_pixels == 0
            || self.refit_iterations == 0
            || self.refit_iterations > 10
            || !self.boundary_min_cv_gain.is_finite()
            || self.boundary_min_cv_gain < 0.0
            || !(0.0..=1.0).contains(&self.boundary_min_jump_fraction)
            || !(0.0..1.0).contains(&self.hysteresis_low_fraction)
            || self.hysteresis_low_fraction <= 0.0
            || self.strong_level_threshold < self.level_threshold
        {
            bail!("heuristics.v2 values are out of range");
        }
        Ok(())
    }
}

impl ReferenceModel {
    pub fn algorithm_version(self, evidence: EvidenceModel) -> &'static str {
        match (self, evidence) {
            (Self::CentralTiles, EvidenceModel::SingleScale) => "tile-detector-v1",
            (Self::RobustAffine, EvidenceModel::SingleScale) => {
                "tile-detector-v1-robust-affine-experimental"
            }
            (Self::CentralTiles, EvidenceModel::MultiscaleConsensus) => {
                "tile-detector-v1-multiscale-consensus-experimental"
            }
            (Self::RobustAffine, EvidenceModel::MultiscaleConsensus) => {
                "tile-detector-v1-robust-affine-multiscale-experimental"
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HeuristicRules {
    #[serde(default)]
    pub detector: DetectorKind,
    #[serde(default)]
    pub v2: V2Rules,
    #[serde(default)]
    pub reference_model: ReferenceModel,
    #[serde(default)]
    pub evidence_model: EvidenceModel,
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
    #[serde(default = "default_maximum_residual_artifact_fraction")]
    /// Maximum fraction of defined pixels in a crop that may retain detector flags.
    pub maximum_residual_artifact_fraction: f64,
    #[serde(default = "default_maximum_crop_sacrifice_fraction")]
    /// Legacy/fallback limit used by heuristic files without a component-count table.
    pub maximum_crop_sacrifice_fraction: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop_sacrifice_fraction_by_border_component_count: Option<CropSacrificeLimits>,
    /// Crop-mode action when no rectangle is accepted (decision D1).
    #[serde(default)]
    pub crop_fallback: CropFallback,
    /// Optional exchange rate lambda (plan R4.1): accept a crop only if
    /// sacrificed unflagged defined pixels <= lambda * flagged pixels removed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop_exchange_rate: Option<f64>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CropSacrificeLimits {
    pub one: f64,
    pub two: f64,
    pub three: f64,
    pub four_or_more: f64,
}

impl HeuristicRules {
    /// Algorithm identity recorded in reports and logs.
    pub fn algorithm_version(&self) -> &'static str {
        match self.detector {
            DetectorKind::TileV1 => self.reference_model.algorithm_version(self.evidence_model),
            DetectorKind::BackgroundV2Experimental => "border-detector-v2-background-experimental",
        }
    }

    /// Return the configured cap for accepted 4-connected artifact components
    /// that each touch at least one image border.
    pub fn crop_sacrifice_limit(&self, border_component_count: usize) -> f64 {
        let Some(limits) = self.crop_sacrifice_fraction_by_border_component_count else {
            return self.maximum_crop_sacrifice_fraction;
        };
        match border_component_count {
            0 | 1 => limits.one,
            2 => limits.two,
            3 => limits.three,
            _ => limits.four_or_more,
        }
    }
}

fn default_maximum_crop_sacrifice_fraction() -> f64 {
    0.35
}

fn default_maximum_residual_artifact_fraction() -> f64 {
    // Older custom heuristic files retain the pre-feature strict crop rule.
    0.0
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
        if !matches!(
            self.processing.mode,
            ProcessingMode::Mask | ProcessingMode::Crop | ProcessingMode::CropMasked
        ) {
            bail!("processing.mode must be 'mask', 'crop' or 'crop_masked'");
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
        if !h.maximum_crop_sacrifice_fraction.is_finite()
            || !(0.0..=1.0).contains(&h.maximum_crop_sacrifice_fraction)
        {
            bail!("heuristics.maximum_crop_sacrifice_fraction must be in [0, 1]");
        }
        if !h.maximum_residual_artifact_fraction.is_finite()
            || !(0.0..=1.0).contains(&h.maximum_residual_artifact_fraction)
        {
            bail!("heuristics.maximum_residual_artifact_fraction must be in [0, 1]");
        }
        h.v2.validate()?;
        if let Some(rate) = h.crop_exchange_rate
            && (!rate.is_finite() || rate < 0.0)
        {
            bail!("heuristics.crop_exchange_rate must be finite and non-negative");
        }
        if let Some(limits) = h.crop_sacrifice_fraction_by_border_component_count {
            let values = [limits.one, limits.two, limits.three, limits.four_or_more];
            if values
                .iter()
                .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
            {
                bail!("crop sacrifice limits by border-component count must be in [0, 1]");
            }
            if values.windows(2).any(|pair| pair[0] > pair[1]) {
                bail!(
                    "crop sacrifice limits must not decrease as border-component count increases"
                );
            }
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
threads = 4
recursive = false
memory_budget_mib = 8192
maximum_image_mib = 1536
mode = "mask"
"#;

const DEFAULT_HEURISTICS: &str = r#"
[heuristics]
reference_model = "central_tiles"
evidence_model = "single_scale"
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
maximum_residual_artifact_fraction = 0.05
maximum_crop_sacrifice_fraction = 0.35
crop_fallback = "bbox_mask"

[heuristics.crop_sacrifice_fraction_by_border_component_count]
one = 0.35
two = 0.35
three = 0.35
four_or_more = 0.47
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
    fn rejects_crop_sacrifice_fraction_outside_unit_interval() {
        let text = DEFAULT_HEURISTICS.replace(
            "maximum_crop_sacrifice_fraction = 0.35",
            "maximum_crop_sacrifice_fraction = 1.1",
        );
        let config: HeuristicsConfig = toml::from_str(&text).unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn residual_artifact_fraction_is_bounded_and_shipped_default_is_explicit() {
        for value in ["-0.01", "1.01", "nan"] {
            let text = DEFAULT_HEURISTICS.replace(
                "maximum_residual_artifact_fraction = 0.05",
                &format!("maximum_residual_artifact_fraction = {value}"),
            );
            let config: HeuristicsConfig = toml::from_str(&text).unwrap();
            assert!(
                config.validate().is_err(),
                "accepted residual fraction {value}"
            );
        }
        let config: HeuristicsConfig = toml::from_str(DEFAULT_HEURISTICS).unwrap();
        assert_eq!(config.heuristics.maximum_residual_artifact_fraction, 0.05);
    }

    #[test]
    fn crop_sacrifice_limit_uses_border_connected_area_count_table() {
        let config: HeuristicsConfig = toml::from_str(DEFAULT_HEURISTICS).unwrap();
        assert_eq!(config.heuristics.crop_sacrifice_limit(1), 0.35);
        assert_eq!(config.heuristics.crop_sacrifice_limit(2), 0.35);
        assert_eq!(config.heuristics.crop_sacrifice_limit(3), 0.35);
        assert_eq!(config.heuristics.crop_sacrifice_limit(4), 0.47);
        assert_eq!(config.heuristics.crop_sacrifice_limit(8), 0.47);
    }

    #[test]
    fn crop_fallback_defaults_to_keep_for_legacy_files_and_ships_bbox_mask() {
        let shipped: HeuristicsConfig = toml::from_str(DEFAULT_HEURISTICS).unwrap();
        assert_eq!(shipped.heuristics.crop_fallback, CropFallback::BboxMask);
        let legacy = DEFAULT_HEURISTICS.replace("crop_fallback = \"bbox_mask\"\n", "");
        let legacy: HeuristicsConfig = toml::from_str(&legacy).unwrap();
        assert_eq!(legacy.heuristics.crop_fallback, CropFallback::Keep);
        assert!(legacy.heuristics.crop_exchange_rate.is_none());
    }

    #[test]
    fn rejects_negative_exchange_rate() {
        let text = DEFAULT_HEURISTICS.replace(
            "crop_fallback = \"bbox_mask\"",
            "crop_fallback = \"bbox_mask\"\ncrop_exchange_rate = -1.0",
        );
        let config: HeuristicsConfig = toml::from_str(&text).unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn rejects_decreasing_crop_sacrifice_limits() {
        let text = DEFAULT_HEURISTICS.replace("two = 0.35", "two = 0.20");
        let config: HeuristicsConfig = toml::from_str(&text).unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn rejects_crop_setting_as_unknown_processing_option() {
        let text = DEFAULT_CONFIG.replace("threads = 4", "threads = 4\nmode = \"crop\"");
        assert!(toml::from_str::<Config>(&text).is_err());
    }

    #[test]
    fn default_processing_config_uses_four_workers() {
        let config: Config = toml::from_str(DEFAULT_CONFIG).unwrap();
        assert_eq!(config.processing.threads, 4);
    }

    #[test]
    fn missing_explicit_heuristic_file_is_an_error() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("missing.toml");
        assert!(HeuristicsConfig::load(Some(&path)).is_err());
    }
}
