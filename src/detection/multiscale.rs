//! Experimental deterministic agreement of accepted border masks across tile widths.
use super::tile_detector::Detection;
use super::{robust::location_scale, tile_detector::detect_single_scale};
use crate::config::HeuristicRules;

const REQUIRED_SCALE_VOTES: u8 = 2;

/// Run the single-scale detector at three bounded tile widths and require per-pixel support.
pub(super) fn detect_multiscale(
    data: &[f64],
    width: usize,
    height: usize,
    cfg: &HeuristicRules,
) -> Detection {
    let mut baseline = detect_single_scale(data, width, height, cfg);
    if width.checked_mul(height) != Some(data.len()) {
        return baseline;
    }
    let largest_supported = width.min(height) / 3;
    if cfg.tile_size > largest_supported {
        baseline.accepted = false;
        baseline.mask.fill(false);
        baseline.accepted_components = 0;
        baseline.reason_code = "multiscale_scale_unavailable";
        baseline.reason = "multiscale consensus requires at least two supported tile widths".into();
        return baseline;
    }

    let mut scales = vec![
        (cfg.tile_size / 2).max(4),
        cfg.tile_size,
        cfg.tile_size.saturating_mul(2).min(largest_supported),
    ];
    scales.sort_unstable();
    scales.dedup();
    if scales.len() < 2 {
        baseline.accepted = false;
        baseline.mask.fill(false);
        baseline.accepted_components = 0;
        baseline.reason_code = "multiscale_scale_unavailable";
        baseline.reason = "multiscale consensus requires two distinct tile widths".into();
        return baseline;
    }

    let mut base_report = None;
    let mut votes = vec![0_u8; data.len()];
    let mut evidence = Vec::with_capacity(scales.len());
    let mut component_counts = Vec::with_capacity(scales.len());
    let mut accepted_scales = Vec::with_capacity(scales.len());
    let mut maximum_score = 0.0_f64;
    let mut maximum_candidate_tiles = 0;
    let mut maximum_candidate_components = 0;
    let mut baseline = Some(baseline);
    for tile_size in scales.iter().copied() {
        let mut cfg_at_scale = cfg.clone();
        cfg_at_scale.tile_size = tile_size;
        let mut result = if tile_size == cfg.tile_size {
            baseline.take().expect("base scale was evaluated once")
        } else {
            detect_single_scale(data, width, height, &cfg_at_scale)
        };
        let is_base_scale = tile_size == cfg.tile_size;
        maximum_score = maximum_score.max(result.score);
        maximum_candidate_tiles = maximum_candidate_tiles.max(result.candidate_tiles);
        maximum_candidate_components =
            maximum_candidate_components.max(result.candidate_components);
        if result.accepted {
            accepted_scales.push(tile_size);
            evidence.push(result.evidence_ratio);
            component_counts.push(result.accepted_components);
            for (vote, selected) in votes.iter_mut().zip(&result.mask) {
                if *selected {
                    *vote = vote.saturating_add(1);
                }
            }
        }
        if is_base_scale {
            result.mask.clear();
            base_report = Some(result);
        }
    }
    let mut report = base_report.expect("base scale is included in the scale family");
    let accepted_scale_count = accepted_scales.len();
    let union_pixels = votes.iter().filter(|&&vote| vote > 0).count();
    let consensus_pixels = votes
        .iter()
        .zip(data)
        .filter(|(vote, value)| **vote >= REQUIRED_SCALE_VOTES && value.is_finite())
        .count();
    let defined = data.iter().filter(|value| value.is_finite()).count();
    let candidate_fraction = union_pixels as f64 / defined.max(1) as f64;
    let consensus_fraction = consensus_pixels as f64 / defined.max(1) as f64;
    let evidence_ratio = if evidence.is_empty() {
        report.evidence_ratio
    } else {
        location_scale(&evidence).0
    }
    .min(cfg.maximum_evidence_ratio)
    .max(0.0);
    let artifact_benefit = consensus_fraction * evidence_ratio;
    let valid_pixel_loss = cfg.valid_pixel_loss_cost * consensus_fraction
        / (1.0 - consensus_fraction).max(f64::EPSILON);
    let net_utility = artifact_benefit - valid_pixel_loss;
    report.mask = vec![false; data.len()];
    report.candidate_mask_pixels = union_pixels;
    report.candidate_tiles = maximum_candidate_tiles;
    report.candidate_components = maximum_candidate_components;
    report.candidate_fraction = candidate_fraction;
    report.score = maximum_score;
    report.evidence_ratio = evidence_ratio;
    report.artifact_benefit = artifact_benefit;
    report.valid_pixel_loss = valid_pixel_loss;
    report.net_utility = net_utility;
    report.accepted_components = 0;
    report.accepted = false;

    if accepted_scale_count < usize::from(REQUIRED_SCALE_VOTES) || consensus_pixels == 0 {
        report.reason_code = "multiscale_no_consensus";
        report.reason = format!(
            "multiscale abstention: {accepted_scale_count}/{} scales passed; {union_pixels} candidate pixels, {consensus_pixels} pixels supported by at least two scales",
            scales.len()
        );
        return report;
    }
    if consensus_fraction < cfg.minimum_mask_fraction
        || consensus_fraction > cfg.maximum_mask_fraction
        || evidence_ratio < cfg.minimum_evidence_ratio
        || net_utility < cfg.minimum_net_utility
    {
        report.reason_code = "multiscale_consensus_rejected";
        report.reason = format!(
            "multiscale consensus retained: {consensus_pixels} pixels at {consensus_fraction:.4} of defined samples; evidence {evidence_ratio:.3}, utility {net_utility:.4}"
        );
        return report;
    }

    for ((selected, vote), value) in report.mask.iter_mut().zip(&votes).zip(data) {
        *selected = *vote >= REQUIRED_SCALE_VOTES && value.is_finite();
    }
    report.accepted = true;
    report.accepted_components = component_counts.into_iter().min().unwrap_or(0);
    report.reason_code = "candidate_accepted";
    report.reason = format!(
        "multiscale consensus accepted: {consensus_pixels} pixels supported at two or more of {} scales {:?}; evidence {evidence_ratio:.3}, net utility {net_utility:.4}",
        scales.len(),
        accepted_scales
    );
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{EvidenceModel, ReferenceModel};

    fn config(tile_size: usize) -> HeuristicRules {
        HeuristicRules {
            reference_model: ReferenceModel::CentralTiles,
            evidence_model: EvidenceModel::MultiscaleConsensus,
            tile_size,
            minimum_border_tiles: 2,
            minimum_abs_z_score: 6.0,
            maximum_tile_noise_ratio: 0.1,
            minimum_spatial_correlation: 0.95,
            minimum_mask_fraction: 0.01,
            maximum_mask_fraction: 0.35,
            minimum_evidence_ratio: 1.25,
            maximum_evidence_ratio: 5.0,
            valid_pixel_loss_cost: 1.5,
            minimum_net_utility: 0.001,
            maximum_residual_artifact_fraction: 0.05,
            maximum_crop_sacrifice_fraction: 0.25,
            crop_sacrifice_fraction_by_border_component_count: None,
        }
    }

    #[test]
    fn accepted_corner_pixels_require_agreement_from_at_least_two_scales() {
        let (width, height) = (96, 96);
        let mut data = vec![100.0; width * height];
        for y in 0..32 {
            for x in 0..32 {
                data[y * width + x] = 0.0;
            }
        }
        let found = detect_multiscale(&data, width, height, &config(8));
        assert!(found.accepted, "{}", found.reason);
        assert_eq!(found.reason_code, "candidate_accepted");
        assert!(found.reason.contains("two or more"));
        assert!(found.mask[0]);
        assert!(!found.mask[48 * width + 48]);
        assert!(
            found
                .mask
                .iter()
                .zip(&data)
                .all(|(masked, value)| !masked || value.is_finite())
        );
        let repeated = detect_multiscale(&data, width, height, &config(8));
        assert_eq!(found.mask, repeated.mask);
        assert_eq!(found.evidence_ratio, repeated.evidence_ratio);
        let mut votes = vec![0_u8; data.len()];
        for tile_size in [4, 8, 16] {
            let mut scale_cfg = config(8);
            scale_cfg.tile_size = tile_size;
            let scale = detect_single_scale(&data, width, height, &scale_cfg);
            if scale.accepted {
                for (vote, selected) in votes.iter_mut().zip(scale.mask) {
                    if selected {
                        *vote += 1;
                    }
                }
            }
        }
        assert!(
            found
                .mask
                .iter()
                .zip(votes)
                .all(|(selected, vote)| *selected == (vote >= REQUIRED_SCALE_VOTES))
        );
    }

    #[test]
    fn offset_corner_boundary_is_detected_without_tile_alignment_assumption() {
        let (width, height) = (96, 96);
        let mut data = vec![100.0; width * height];
        for y in 0..29 {
            for x in 0..35 {
                data[y * width + x] = 0.0;
            }
        }
        let found = detect_multiscale(&data, width, height, &config(8));
        assert!(found.accepted, "{}", found.reason);
        assert!(found.mask[0]);
        assert!(!found.mask[48 * width + 48]);
    }

    #[test]
    fn separate_corner_components_are_retained_across_scales() {
        let (width, height) = (96, 96);
        let mut data = vec![100.0; width * height];
        for y in 0..32 {
            for x in 0..32 {
                data[y * width + x] = 0.0;
                data[(height - 1 - y) * width + (width - 1 - x)] = 0.0;
            }
        }
        let found = detect_multiscale(&data, width, height, &config(8));
        assert!(found.accepted, "{}", found.reason);
        assert!(found.mask[0]);
        assert!(found.mask[(height - 1) * width + width - 1]);
        assert!(!found.mask[48 * width + 48]);
    }

    #[test]
    fn exact_affine_gradient_null_is_kept_at_every_scale() {
        let (width, height) = (96, 96);
        let data: Vec<_> = (0..height)
            .flat_map(|y| (0..width).map(move |x| 100.0 + 0.1 * x as f64 + 0.05 * y as f64))
            .collect();
        let mut cfg = config(8);
        cfg.reference_model = ReferenceModel::RobustAffine;
        let found = detect_multiscale(&data, width, height, &cfg);
        assert!(!found.accepted);
        assert_eq!(found.reason_code, "multiscale_no_consensus");
        assert!(found.mask.iter().all(|selected| !selected));
    }

    #[test]
    fn one_scale_only_proposal_abstains() {
        let (width, height) = (24, 24);
        let mut data = vec![100.0; width * height];
        for y in 0..8 {
            for x in 0..8 {
                data[y * width + x] = 0.0;
            }
        }
        let found = detect_multiscale(&data, width, height, &config(8));
        assert!(!found.accepted);
        assert_eq!(found.reason_code, "multiscale_no_consensus");
        assert!(found.mask.iter().all(|selected| !selected));
    }
}
