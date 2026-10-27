//! Stable per-file outcomes and detector diagnostics used by logs and reports.
use serde::Serialize;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeKind {
    Clipped,
    Kept,
    Ignored,
    Error,
}

#[derive(Clone, Debug, Serialize)]
pub struct FileOutcome {
    pub input: PathBuf,
    pub status: OutcomeKind,
    pub reason: String,
    pub width: Option<usize>,
    pub height: Option<usize>,
    pub masked_pixels: usize,
    pub mask_fraction: f64,
    pub proposed_mask_fraction: f64,
    pub candidate_tiles: usize,
    pub score: f64,
    pub evidence_ratio: f64,
    pub artifact_benefit: f64,
    pub valid_pixel_loss: f64,
    pub net_utility: f64,
    pub crop_origin_x: Option<usize>,
    pub crop_origin_y: Option<usize>,
    pub crop_width: Option<usize>,
    pub crop_height: Option<usize>,
    pub crop_shape: Option<String>,
    pub crop_center_x: Option<f64>,
    pub crop_center_y: Option<f64>,
    pub crop_radius: Option<f64>,
    pub image_area_retained: usize,
    pub artifact_pixels_removed: usize,
    pub unmasked_pixels_sacrificed: usize,
    pub output_image: Option<PathBuf>,
    pub output_mask: Option<PathBuf>,
    pub output_png: Option<PathBuf>,
    pub elapsed_ms: u128,
}

impl FileOutcome {
    pub fn new(input: PathBuf, status: OutcomeKind, reason: impl Into<String>) -> Self {
        Self {
            input,
            status,
            reason: reason.into(),
            width: None,
            height: None,
            masked_pixels: 0,
            mask_fraction: 0.0,
            proposed_mask_fraction: 0.0,
            candidate_tiles: 0,
            score: 0.0,
            evidence_ratio: 0.0,
            artifact_benefit: 0.0,
            valid_pixel_loss: 0.0,
            net_utility: 0.0,
            crop_origin_x: None,
            crop_origin_y: None,
            crop_width: None,
            crop_height: None,
            crop_shape: None,
            crop_center_x: None,
            crop_center_y: None,
            crop_radius: None,
            image_area_retained: 0,
            artifact_pixels_removed: 0,
            unmasked_pixels_sacrificed: 0,
            output_image: None,
            output_mask: None,
            output_png: None,
            elapsed_ms: 0,
        }
    }
}
