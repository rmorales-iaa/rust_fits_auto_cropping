//! Deterministic run summary in JSON and CSV.
use crate::model::{FileOutcome, OutcomeKind};
use anyhow::Context;
use chrono::Utc;
use serde::Serialize;
use std::{fs, path::Path};

#[derive(Serialize)]
pub struct Summary<'a> {
    pub run_id: &'a str,
    pub created_utc: String,
    pub total: usize,
    pub clipped: usize,
    pub kept: usize,
    pub ignored: usize,
    pub errors: usize,
    pub files: &'a [FileOutcome],
}

pub fn write(output: &Path, run_id: &str, outcomes: &[FileOutcome]) -> anyhow::Result<()> {
    let dir = output.join("reports").join(run_id);
    fs::create_dir_all(&dir)?;
    let count = |f: fn(&OutcomeKind) -> bool| outcomes.iter().filter(|o| f(&o.status)).count();
    let summary = Summary {
        run_id,
        created_utc: Utc::now().to_rfc3339(),
        total: outcomes.len(),
        clipped: count(|s| matches!(s, OutcomeKind::Clipped)),
        kept: count(|s| matches!(s, OutcomeKind::Kept)),
        ignored: count(|s| matches!(s, OutcomeKind::Ignored)),
        errors: count(|s| matches!(s, OutcomeKind::Error)),
        files: outcomes,
    };
    fs::write(
        dir.join("summary.json"),
        serde_json::to_vec_pretty(&summary)?,
    )?;
    let mut csv = String::from(
        "input,status,reason,width,height,masked_pixels,mask_fraction,proposed_mask_fraction,candidate_tiles,score,output_image,output_mask,elapsed_ms\n",
    );
    for o in outcomes {
        let fields = [
            o.input.display().to_string(),
            format!("{:?}", o.status).to_lowercase(),
            o.reason.clone(),
            o.width.map(|v| v.to_string()).unwrap_or_default(),
            o.height.map(|v| v.to_string()).unwrap_or_default(),
            o.masked_pixels.to_string(),
            format!("{:.8}", o.mask_fraction),
            format!("{:.8}", o.proposed_mask_fraction),
            o.candidate_tiles.to_string(),
            format!("{:.6}", o.score),
            o.output_image
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            o.output_mask
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
    fs::write(dir.join("per-image.csv"), csv).context("writing per-image summary")?;
    Ok(())
}
