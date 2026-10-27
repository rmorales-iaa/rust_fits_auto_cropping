//! Candidate mask from robust tile contrast, tile noise and border connectivity.
use super::reference::fit_robust_affine;
use super::{
    correlation::lag_one_correlation, multiscale::detect_multiscale, robust::location_scale,
};
use crate::config::{EvidenceModel, HeuristicRules, ReferenceModel};
use std::collections::VecDeque;

#[derive(Debug)]
pub struct Detection {
    pub mask: Vec<bool>,
    pub candidate_mask_pixels: usize,
    pub candidate_tiles: usize,
    pub candidate_components: usize,
    pub accepted_components: usize,
    pub candidate_fraction: f64,
    pub accepted: bool,
    pub reason_code: &'static str,
    pub reason: String,
    pub score: f64,
    pub evidence_ratio: f64,
    pub artifact_benefit: f64,
    pub valid_pixel_loss: f64,
    pub net_utility: f64,
}

#[derive(Clone, Copy, Debug)]
struct Tile {
    median: f64,
    sigma: f64,
    correlation: Option<f64>,
    candidate: bool,
    evidence_ratio: f64,
}

/// Detect low-variance or strongly shifted tiles connected to the frame boundary.
/// The method deliberately abstains on weak contrast, inadequate interior support,
/// small regions and excessive masks. It is a conservative baseline requiring empirical validation.
pub fn detect_border(data: &[f64], width: usize, height: usize, cfg: &HeuristicRules) -> Detection {
    match cfg.evidence_model {
        EvidenceModel::SingleScale => detect_single_scale(data, width, height, cfg),
        EvidenceModel::MultiscaleConsensus => detect_multiscale(data, width, height, cfg),
    }
}

pub(super) fn detect_single_scale(
    data: &[f64],
    width: usize,
    height: usize,
    cfg: &HeuristicRules,
) -> Detection {
    let empty = || Detection {
        mask: vec![false; data.len()],
        candidate_mask_pixels: 0,
        candidate_tiles: 0,
        candidate_components: 0,
        accepted_components: 0,
        candidate_fraction: 0.0,
        accepted: false,
        reason_code: "insufficient_data",
        reason: "insufficient valid data".into(),
        score: 0.0,
        evidence_ratio: 0.0,
        artifact_benefit: 0.0,
        valid_pixel_loss: 0.0,
        net_utility: 0.0,
    };
    if width < cfg.tile_size * 3
        || height < cfg.tile_size * 3
        || width.checked_mul(height) != Some(data.len())
    {
        return empty();
    }
    let defined_pixels = data.iter().filter(|value| value.is_finite()).count();
    if defined_pixels == 0 {
        return empty();
    }
    let cols = width.div_ceil(cfg.tile_size);
    let rows = height.div_ceil(cfg.tile_size);
    let mut tiles = Vec::with_capacity(rows * cols);
    for ty in 0..rows {
        for tx in 0..cols {
            let mut samples = Vec::new();
            for y in ty * cfg.tile_size..((ty + 1) * cfg.tile_size).min(height) {
                samples.extend(
                    data[y * width + tx * cfg.tile_size
                        ..y * width + ((tx + 1) * cfg.tile_size).min(width)]
                        .iter()
                        .copied()
                        .filter(|v| v.is_finite()),
                );
            }
            let (median, sigma) = location_scale(&samples);
            let x0 = tx * cfg.tile_size;
            let x1 = ((tx + 1) * cfg.tile_size).min(width);
            let y0 = ty * cfg.tile_size;
            let y1 = ((ty + 1) * cfg.tile_size).min(height);
            let correlation = lag_one_correlation(data, width, height, x0, x1, y0, y1);
            tiles.push(Tile {
                median,
                sigma,
                correlation,
                candidate: false,
                evidence_ratio: 0.0,
            });
        }
    }
    let interior_indices: Vec<usize> = (0..rows)
        .flat_map(|y| {
            (0..cols).filter_map(move |x| {
                (x >= cols / 4 && x < cols - cols / 4 && y >= rows / 4 && y < rows - rows / 4)
                    .then_some(y * cols + x)
            })
        })
        .filter(|&i| tiles[i].median.is_finite())
        .collect();
    let interior: Vec<f64> = interior_indices.iter().map(|&i| tiles[i].median).collect();
    if interior.len() < 4 {
        return empty();
    }
    let (center, center_scale) = location_scale(&interior);
    if !center.is_finite() {
        return empty();
    }
    let interior_noise: Vec<f64> = interior_indices
        .iter()
        .map(|&i| tiles[i].sigma)
        .filter(|v| v.is_finite())
        .collect();
    let (_, noise_scale) = location_scale(&interior_noise);
    let (reference_values, scale, typical_noise) = match cfg.reference_model {
        ReferenceModel::CentralTiles => {
            let typical_noise = interior_noise.iter().copied().fold(0.0_f64, f64::max);
            let scale = center_scale
                .max(noise_scale)
                .max(f64::EPSILON * center.abs().max(1.0));
            (vec![center; tiles.len()], scale, typical_noise)
        }
        ReferenceModel::RobustAffine => {
            let mut points = Vec::with_capacity(interior_indices.len());
            let mut medians = Vec::with_capacity(interior_indices.len());
            let mut dispersions = Vec::with_capacity(interior_indices.len());
            for &index in &interior_indices {
                let x = index % cols;
                let y = index / cols;
                let x0 = x * cfg.tile_size;
                let x1 = ((x + 1) * cfg.tile_size).min(width);
                let y0 = y * cfg.tile_size;
                let y1 = ((y + 1) * cfg.tile_size).min(height);
                let x_center = (x0 as f64 + (x1 - 1) as f64) * 0.5;
                let y_center = (y0 as f64 + (y1 - 1) as f64) * 0.5;
                points.push((
                    2.0 * x_center / width.saturating_sub(1).max(1) as f64 - 1.0,
                    2.0 * y_center / height.saturating_sub(1).max(1) as f64 - 1.0,
                ));
                medians.push(tiles[index].median);
                dispersions.push(tiles[index].sigma);
            }
            let Some(model) = fit_robust_affine(&points, &medians, &dispersions) else {
                let mut result = empty();
                result.reason_code = "reference_fit_abstained";
                result.reason =
                    "robust affine reference had insufficient or rank-deficient interior support"
                        .into();
                return result;
            };
            let mut predictions = Vec::with_capacity(tiles.len());
            for index in 0..tiles.len() {
                let x = index % cols;
                let y = index / cols;
                let x0 = x * cfg.tile_size;
                let x1 = ((x + 1) * cfg.tile_size).min(width);
                let y0 = y * cfg.tile_size;
                let y1 = ((y + 1) * cfg.tile_size).min(height);
                let x_center = (x0 as f64 + (x1 - 1) as f64) * 0.5;
                let y_center = (y0 as f64 + (y1 - 1) as f64) * 0.5;
                let normalized_x = 2.0 * x_center / width.saturating_sub(1).max(1) as f64 - 1.0;
                let normalized_y = 2.0 * y_center / height.saturating_sub(1).max(1) as f64 - 1.0;
                let Some(prediction) = model.predict(normalized_x, normalized_y) else {
                    let mut result = empty();
                    result.reason_code = "reference_fit_abstained";
                    result.reason =
                        "robust affine reference predicted a non-finite tile level".into();
                    return result;
                };
                predictions.push(prediction);
            }
            (predictions, model.residual_scale, model.typical_noise)
        }
    };
    if !scale.is_finite() || scale <= 0.0 || !typical_noise.is_finite() {
        return empty();
    }
    let mut best_score: f64 = 0.0;
    for (tile, reference) in tiles.iter_mut().zip(reference_values) {
        let delta = (tile.median - reference).abs() / scale;
        let quiet = typical_noise > 0.0
            && tile.sigma.is_finite()
            && tile.sigma <= typical_noise * cfg.maximum_tile_noise_ratio;
        let correlated = tile
            .correlation
            .is_some_and(|r| r >= cfg.minimum_spatial_correlation);
        let uniform_reference = match cfg.reference_model {
            ReferenceModel::CentralTiles => noise_scale <= f64::EPSILON,
            ReferenceModel::RobustAffine => scale <= f64::EPSILON,
        };
        let uniform_contrast = tile.sigma <= f64::EPSILON && uniform_reference;
        tile.evidence_ratio = if delta.is_finite() {
            delta / cfg.minimum_abs_z_score
        } else {
            0.0
        };
        tile.candidate =
            delta >= cfg.minimum_abs_z_score && (quiet || correlated || uniform_contrast);
        if delta.is_finite() {
            best_score = best_score.max(delta.min(1.0e6));
        }
    }
    // Label edge-connected components independently so one region cannot
    // subsidize another region's size, support, evidence or utility gates.
    let mut seen = vec![false; tiles.len()];
    let mut accepted_tiles = vec![false; tiles.len()];
    let mut queue = VecDeque::new();
    let mut components = Vec::new();
    for y in 0..rows {
        for x in 0..cols {
            if x == 0 || y == 0 || x + 1 == cols || y + 1 == rows {
                let i = y * cols + x;
                if tiles[i].candidate && !seen[i] {
                    seen[i] = true;
                    queue.push_back(i);
                    let mut component = Vec::new();
                    while let Some(tile_index) = queue.pop_front() {
                        component.push(tile_index);
                        let tile_x = tile_index % cols;
                        let tile_y = tile_index / cols;
                        for (nx, ny) in [
                            (tile_x.wrapping_sub(1), tile_y),
                            (tile_x + 1, tile_y),
                            (tile_x, tile_y.wrapping_sub(1)),
                            (tile_x, tile_y + 1),
                        ] {
                            if nx < cols && ny < rows {
                                let neighbor = ny * cols + nx;
                                if tiles[neighbor].candidate && !seen[neighbor] {
                                    seen[neighbor] = true;
                                    queue.push_back(neighbor);
                                }
                            }
                        }
                    }
                    components.push(component);
                }
            }
        }
    }
    let candidate_tile_count: usize = components.iter().map(Vec::len).sum();
    let candidate_pixels: usize = components
        .iter()
        .flatten()
        .map(|&tile_index| tile_finite_pixels(data, width, height, cols, cfg.tile_size, tile_index))
        .sum();
    let candidate_fraction = candidate_pixels as f64 / defined_pixels as f64;
    let mut accepted_component_count = 0;
    for component in &components {
        let component_pixels: usize = component
            .iter()
            .map(|&tile_index| {
                tile_finite_pixels(data, width, height, cols, cfg.tile_size, tile_index)
            })
            .sum();
        let component_fraction = component_pixels as f64 / defined_pixels as f64;
        let evidence: Vec<f64> = component
            .iter()
            .map(|&tile_index| tiles[tile_index].evidence_ratio)
            .collect();
        let (component_evidence, _) = location_scale(&evidence);
        let component_evidence = component_evidence.min(cfg.maximum_evidence_ratio).max(0.0);
        let component_benefit = component_fraction * component_evidence;
        let component_loss = cfg.valid_pixel_loss_cost * component_fraction
            / (1.0 - component_fraction).max(f64::EPSILON);
        let component_utility = component_benefit - component_loss;
        let passes = component.len() >= cfg.minimum_border_tiles
            && component_fraction >= cfg.minimum_mask_fraction
            && component_fraction <= cfg.maximum_mask_fraction
            && component_evidence >= cfg.minimum_evidence_ratio
            && component_utility >= cfg.minimum_net_utility;
        if passes {
            accepted_component_count += 1;
            for &tile_index in component {
                accepted_tiles[tile_index] = true;
            }
        }
    }
    let count = candidate_tile_count;
    let selected_evidence: Vec<f64> = if accepted_component_count == 0 {
        components
            .iter()
            .flatten()
            .map(|&tile_index| tiles[tile_index].evidence_ratio)
            .collect()
    } else {
        tiles
            .iter()
            .zip(&accepted_tiles)
            .filter_map(|(tile, selected)| selected.then_some(tile.evidence_ratio))
            .collect()
    };
    let evidence_ratio = if selected_evidence.is_empty() {
        0.0
    } else {
        let (center, _) = location_scale(&selected_evidence);
        center.min(cfg.maximum_evidence_ratio).max(0.0)
    };
    let mut selected_pixels = 0usize;
    for (ti, selected) in accepted_tiles.iter().enumerate() {
        if !selected {
            continue;
        }
        let tx = ti % cols;
        let ty = ti / cols;
        for y in ty * cfg.tile_size..((ty + 1) * cfg.tile_size).min(height) {
            for x in tx * cfg.tile_size..((tx + 1) * cfg.tile_size).min(width) {
                if data[y * width + x].is_finite() {
                    selected_pixels += 1;
                }
            }
        }
    }
    let fraction = selected_pixels as f64 / defined_pixels as f64;
    let decision_fraction = if accepted_component_count == 0 {
        candidate_fraction
    } else {
        fraction
    };
    // Report the same clip-versus-keep evidence for every non-empty candidate,
    // including candidates rejected by area or support gates. This makes an
    // abstention auditable without implying that the score is calibrated.
    let artifact_benefit = decision_fraction * evidence_ratio;
    let valid_pixel_loss =
        cfg.valid_pixel_loss_cost * decision_fraction / (1.0 - decision_fraction).max(f64::EPSILON);
    let net_utility = artifact_benefit - valid_pixel_loss;
    if accepted_component_count == 0 {
        return Detection {
            mask: vec![false; data.len()],
            candidate_mask_pixels: candidate_pixels,
            candidate_tiles: count,
            candidate_components: components.len(),
            accepted_components: 0,
            candidate_fraction,
            accepted: false,
            reason_code: if components.is_empty() {
                "no_border_candidate"
            } else {
                "candidate_rejected"
            },
            reason: if candidate_fraction > cfg.maximum_mask_fraction {
                "candidate exceeds maximum mask fraction".into()
            } else if candidate_fraction < cfg.minimum_mask_fraction {
                "candidate below minimum mask fraction".into()
            } else if evidence_ratio < cfg.minimum_evidence_ratio
                || net_utility < cfg.minimum_net_utility
            {
                format!(
                    "candidate retained: artifact benefit {artifact_benefit:.4} minus valid-pixel loss {valid_pixel_loss:.4} gives utility {net_utility:.4}, below configured clip rule"
                )
            } else {
                "no edge component passed its independent size, area, evidence and utility gates"
                    .into()
            },
            score: best_score,
            evidence_ratio,
            artifact_benefit,
            valid_pixel_loss,
            net_utility,
        };
    }
    if fraction < cfg.minimum_mask_fraction {
        return Detection {
            mask: vec![false; data.len()],
            candidate_mask_pixels: candidate_pixels,
            candidate_tiles: count,
            candidate_components: components.len(),
            accepted_components: accepted_component_count,
            candidate_fraction,
            accepted: false,
            reason_code: "candidate_rejected",
            reason: "candidate below minimum mask fraction".into(),
            score: best_score,
            evidence_ratio,
            artifact_benefit,
            valid_pixel_loss,
            net_utility,
        };
    }
    if fraction > cfg.maximum_mask_fraction {
        return Detection {
            mask: vec![false; data.len()],
            candidate_mask_pixels: candidate_pixels,
            candidate_tiles: count,
            candidate_components: components.len(),
            accepted_components: accepted_component_count,
            candidate_fraction,
            accepted: false,
            reason_code: "candidate_rejected",
            reason: "candidate exceeds maximum mask fraction".into(),
            score: best_score,
            evidence_ratio,
            artifact_benefit,
            valid_pixel_loss,
            net_utility,
        };
    }
    if evidence_ratio < cfg.minimum_evidence_ratio || net_utility < cfg.minimum_net_utility {
        return Detection {
            mask: vec![false; data.len()],
            candidate_mask_pixels: candidate_pixels,
            candidate_tiles: count,
            candidate_components: components.len(),
            accepted_components: accepted_component_count,
            candidate_fraction,
            accepted: false,
            reason_code: "candidate_rejected",
            reason: format!(
                "candidate retained: artifact benefit {artifact_benefit:.4} minus valid-pixel loss {valid_pixel_loss:.4} gives utility {net_utility:.4}, below configured clip rule"
            ),
            score: best_score,
            evidence_ratio,
            artifact_benefit,
            valid_pixel_loss,
            net_utility,
        };
    }
    let mut mask = vec![false; data.len()];
    for (ti, selected) in accepted_tiles.iter().enumerate() {
        if *selected {
            let tx = ti % cols;
            let ty = ti / cols;
            for y in ty * cfg.tile_size..((ty + 1) * cfg.tile_size).min(height) {
                for x in tx * cfg.tile_size..((tx + 1) * cfg.tile_size).min(width) {
                    if data[y * width + x].is_finite() {
                        mask[y * width + x] = true;
                    }
                }
            }
        }
    }
    Detection {
        mask,
        candidate_mask_pixels: candidate_pixels,
        candidate_tiles: count,
        candidate_components: components.len(),
        accepted_components: accepted_component_count,
        candidate_fraction,
        accepted: true,
        reason_code: "candidate_accepted",
        reason: format!(
            "clip accepted: evidence ratio {evidence_ratio:.3}; artifact benefit {artifact_benefit:.4}; valid-pixel loss {valid_pixel_loss:.4}; net utility {net_utility:.4}"
        ),
        score: best_score,
        evidence_ratio,
        artifact_benefit,
        valid_pixel_loss,
        net_utility,
    }
}

fn tile_finite_pixels(
    data: &[f64],
    width: usize,
    height: usize,
    columns: usize,
    tile_size: usize,
    tile_index: usize,
) -> usize {
    let tx = tile_index % columns;
    let ty = tile_index / columns;
    (ty * tile_size..((ty + 1) * tile_size).min(height))
        .flat_map(|y| {
            (tx * tile_size..((tx + 1) * tile_size).min(width)).map(move |x| y * width + x)
        })
        .filter(|&index| data[index].is_finite())
        .count()
}

#[cfg(test)]
#[path = "tile_detector_tests.rs"]
mod tests;
