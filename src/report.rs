//! Deterministic run summary in JSON and CSV.
use crate::model::{FileOutcome, OutcomeKind};
use anyhow::Context;
use chrono::Utc;
use serde::Serialize;
use std::{fs, path::Path};

#[derive(Serialize)]
pub struct Summary<'a> {
    pub schema_version: u32,
    pub run_id: &'a str,
    pub created_utc: String,
    pub program: &'static str,
    pub version: &'static str,
    pub total: usize,
    pub output_images: usize,
    pub clipped: usize,
    pub proposed: usize,
    pub kept: usize,
    pub ignored: usize,
    pub errors: usize,
    pub dry_run: bool,
    pub algorithm_version: &'a str,
    pub policy_version: &'static str,
    pub git_revision: &'static str,
    pub git_dirty: bool,
    pub executable_sha256: &'a str,
    pub source_config_sha256: &'a str,
    pub effective_config_sha256: &'a str,
    pub source_heuristics_sha256: &'a str,
    pub effective_heuristics_sha256: &'a str,
    pub files: &'a [FileOutcome],
}

pub struct ReportIdentity<'a> {
    pub algorithm_version: &'a str,
    pub executable_sha256: &'a str,
    pub source_config_sha256: &'a str,
    pub effective_config_sha256: &'a str,
    pub source_heuristics_sha256: &'a str,
    pub effective_heuristics_sha256: &'a str,
}

pub fn write(
    run_dir: &Path,
    run_id: &str,
    outcomes: &[FileOutcome],
    dry_run: bool,
    identity: ReportIdentity<'_>,
) -> anyhow::Result<()> {
    let count = |f: fn(&OutcomeKind) -> bool| outcomes.iter().filter(|o| f(&o.status)).count();
    let summary = Summary {
        schema_version: 8,
        run_id,
        created_utc: Utc::now().to_rfc3339(),
        program: env!("CARGO_PKG_NAME"),
        version: env!("RUST_FITS_AUTO_CROPPING_VERSION"),
        total: outcomes.len(),
        output_images: outcomes
            .iter()
            .filter(|outcome| outcome.output_image.is_some())
            .count(),
        clipped: count(|s| matches!(s, OutcomeKind::Clipped)),
        proposed: count(|s| matches!(s, OutcomeKind::Proposed)),
        kept: count(|s| matches!(s, OutcomeKind::Kept)),
        ignored: count(|s| matches!(s, OutcomeKind::Ignored)),
        errors: count(|s| matches!(s, OutcomeKind::Error)),
        dry_run,
        algorithm_version: identity.algorithm_version,
        policy_version: "heuristics-v1",
        git_revision: env!("RUST_FITS_GIT_REVISION"),
        git_dirty: env!("RUST_FITS_GIT_DIRTY") == "true",
        executable_sha256: identity.executable_sha256,
        source_config_sha256: identity.source_config_sha256,
        effective_config_sha256: identity.effective_config_sha256,
        source_heuristics_sha256: identity.source_heuristics_sha256,
        effective_heuristics_sha256: identity.effective_heuristics_sha256,
        files: outcomes,
    };
    fs::write(
        run_dir.join("summary.json"),
        serde_json::to_vec_pretty(&summary)?,
    )?;
    let mut csv = String::from(
        "input,status,reason_code,reason,width,height,source_sha256,original_undefined_pixels,candidate_mask_pixels,candidate_undefined_pixels,candidate_proposed_overlap_pixels,proposed_mask_pixels,proposed_undefined_pixels,applied_artifact_pixels,applied_artifact_undefined_pixels,masked_pixels,crop_excluded_undefined_pixels,crop_applied_undefined_pixels,crop_artifact_pixels_retained,mask_fraction,proposed_mask_fraction,candidate_tiles,candidate_components,accepted_components,score,evidence_ratio,artifact_benefit,valid_pixel_loss,net_utility,crop_shape,crop_origin_x,crop_origin_y,crop_width,crop_height,crop_center_x,crop_center_y,crop_radius,image_area_retained,artifact_pixels_removed,unmasked_pixels_sacrificed,output_image,output_mask,output_reason_mask,output_png,output_manifest,elapsed_ms\n",
    );
    for o in outcomes {
        let fields = [
            o.input.display().to_string(),
            format!("{:?}", o.status).to_lowercase(),
            o.reason_code.to_string(),
            o.reason.clone(),
            o.width.map(|v| v.to_string()).unwrap_or_default(),
            o.height.map(|v| v.to_string()).unwrap_or_default(),
            o.source_sha256.clone().unwrap_or_default(),
            o.original_undefined_pixels.to_string(),
            o.candidate_mask_pixels.to_string(),
            o.candidate_undefined_pixels.to_string(),
            o.candidate_proposed_overlap_pixels.to_string(),
            o.proposed_mask_pixels.to_string(),
            o.proposed_undefined_pixels.to_string(),
            o.applied_artifact_pixels.to_string(),
            o.applied_artifact_undefined_pixels.to_string(),
            o.masked_pixels.to_string(),
            o.crop_excluded_undefined_pixels.to_string(),
            o.crop_applied_undefined_pixels.to_string(),
            o.crop_artifact_pixels_retained.to_string(),
            format!("{:.8}", o.mask_fraction),
            format!("{:.8}", o.proposed_mask_fraction),
            o.candidate_tiles.to_string(),
            o.candidate_components.to_string(),
            o.accepted_components.to_string(),
            format!("{:.6}", o.score),
            format!("{:.6}", o.evidence_ratio),
            format!("{:.6}", o.artifact_benefit),
            format!("{:.6}", o.valid_pixel_loss),
            format!("{:.6}", o.net_utility),
            o.crop_shape.clone().unwrap_or_default(),
            o.crop_origin_x.map_or_else(String::new, |v| v.to_string()),
            o.crop_origin_y.map_or_else(String::new, |v| v.to_string()),
            o.crop_width.map_or_else(String::new, |v| v.to_string()),
            o.crop_height.map_or_else(String::new, |v| v.to_string()),
            o.crop_center_x
                .map_or_else(String::new, |v| format!("{v:.6}")),
            o.crop_center_y
                .map_or_else(String::new, |v| format!("{v:.6}")),
            o.crop_radius
                .map_or_else(String::new, |v| format!("{v:.6}")),
            o.image_area_retained.to_string(),
            o.artifact_pixels_removed.to_string(),
            o.unmasked_pixels_sacrificed.to_string(),
            o.output_image
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            o.output_mask
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            o.output_reason_mask
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            o.output_png
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            o.output_manifest
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            o.elapsed_ms.to_string(),
        ];
        csv.push_str(
            &fields
                .iter()
                .map(|s| format!("\"{}\"", s.replace('"', "\"\"")))
                .collect::<Vec<_>>()
                .join(","),
        );
        csv.push('\n');
    }
    fs::write(run_dir.join("per-image.csv"), csv).context("writing per-image summary")?;
    Ok(())
}
