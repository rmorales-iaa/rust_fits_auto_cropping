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
    pub output_image: Option<PathBuf>,
    pub output_mask: Option<PathBuf>,
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
            output_image: None,
            output_mask: None,
            elapsed_ms: 0,
        }
    }
}
