//! Candidate mask from robust tile contrast, tile noise and border connectivity.
use super::{correlation::lag_one_correlation, robust::location_scale};
use crate::config::HeuristicRules;
use std::collections::VecDeque;

#[derive(Debug)]
pub struct Detection {
    pub mask: Vec<bool>,
    pub candidate_tiles: usize,
    pub candidate_fraction: f64,
    pub accepted: bool,
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
    let empty = || Detection {
        mask: vec![false; data.len()],
        candidate_tiles: 0,
        candidate_fraction: 0.0,
        accepted: false,
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
    let interior: Vec<f64> = (0..rows)
        .flat_map(|y| {
            (0..cols).filter_map(move |x| {
                (x >= cols / 4 && x < cols - cols / 4 && y >= rows / 4 && y < rows - rows / 4)
                    .then_some(y * cols + x)
            })
        })
        .map(|i| tiles[i].median)
        .filter(|v| v.is_finite())
        .collect();
    if interior.len() < 4 {
        return empty();
    }
    let (center, mut scale) = location_scale(&interior);
    if !center.is_finite() {
        return empty();
    }
    let interior_noise: Vec<f64> = tiles
        .iter()
        .enumerate()
        .filter(|(i, _)| {
            interior.iter().any(|v| v.is_finite()) && {
                let x = i % cols;
                let y = i / cols;
                x >= cols / 4 && x < cols - cols / 4 && y >= rows / 4 && y < rows - rows / 4
            }
        })
        .map(|(_, t)| t.sigma)
        .filter(|v| v.is_finite())
        .collect();
    let (_, noise_scale) = location_scale(&interior_noise);
    let typical_noise = interior_noise
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .fold(0.0_f64, f64::max);
    scale = scale
        .max(noise_scale)
        .max(f64::EPSILON * center.abs().max(1.0));
    let mut best_score: f64 = 0.0;
    for tile in &mut tiles {
        let delta = (tile.median - center).abs() / scale;
        let quiet = typical_noise > 0.0
            && tile.sigma.is_finite()
            && tile.sigma <= typical_noise * cfg.maximum_tile_noise_ratio;
        let correlated = tile
            .correlation
            .is_some_and(|r| r >= cfg.minimum_spatial_correlation);
        let uniform_contrast = tile.sigma <= f64::EPSILON && noise_scale <= f64::EPSILON;
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
    // Flood-fill only candidates connected to an edge; isolated sources/features remain intact.
    let mut seen = vec![false; tiles.len()];
    let mut accepted_tiles = vec![false; tiles.len()];
    let mut queue = VecDeque::new();
    for y in 0..rows {
        for x in 0..cols {
            if x == 0 || y == 0 || x + 1 == cols || y + 1 == rows {
                let i = y * cols + x;
                if tiles[i].candidate && !seen[i] {
                    seen[i] = true;
                    queue.push_back(i);
                }
            }
        }
    }
    while let Some(i) = queue.pop_front() {
        accepted_tiles[i] = true;
        let x = i % cols;
        let y = i / cols;
        for (nx, ny) in [
            (x.wrapping_sub(1), y),
            (x + 1, y),
            (x, y.wrapping_sub(1)),
            (x, y + 1),
        ] {
            if nx < cols && ny < rows {
                let ni = ny * cols + nx;
                if tiles[ni].candidate && !seen[ni] {
                    seen[ni] = true;
                    queue.push_back(ni);
                }
            }
        }
    }
    let count = accepted_tiles.iter().filter(|v| **v).count();
    let selected_evidence: Vec<f64> = tiles
        .iter()
        .zip(&accepted_tiles)
        .filter_map(|(tile, selected)| selected.then_some(tile.evidence_ratio))
        .collect();
    let (evidence_ratio, _) = location_scale(&selected_evidence);
    let evidence_ratio = evidence_ratio.min(cfg.maximum_evidence_ratio).max(0.0);
    let selected_pixels: usize = accepted_tiles
        .iter()
        .enumerate()
        .filter(|(_, selected)| **selected)
        .map(|(ti, _)| {
            let tx = ti % cols;
            let ty = ti / cols;
            (((tx + 1) * cfg.tile_size).min(width) - tx * cfg.tile_size)
                * (((ty + 1) * cfg.tile_size).min(height) - ty * cfg.tile_size)
        })
        .sum();
    let fraction = selected_pixels as f64 / data.len() as f64;
    // Report the same clip-versus-keep evidence for every non-empty candidate,
    // including candidates rejected by area or support gates. This makes an
    // abstention auditable without implying that the score is calibrated.
    let artifact_benefit = fraction * evidence_ratio;
    let valid_pixel_loss =
        cfg.valid_pixel_loss_cost * fraction / (1.0 - fraction).max(f64::EPSILON);
    let net_utility = artifact_benefit - valid_pixel_loss;
    if count < cfg.minimum_border_tiles {
        return Detection {
            mask: vec![false; data.len()],
            candidate_tiles: count,
            candidate_fraction: fraction,
            accepted: false,
            reason: "border component below minimum tile count".into(),
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
            candidate_tiles: count,
            candidate_fraction: fraction,
            accepted: false,
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
            candidate_tiles: count,
            candidate_fraction: fraction,
            accepted: false,
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
            candidate_tiles: count,
            candidate_fraction: fraction,
            accepted: false,
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
                    mask[y * width + x] = true;
                }
            }
        }
    }
    Detection {
        mask,
        candidate_tiles: count,
        candidate_fraction: fraction,
        accepted: true,
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

#[cfg(test)]
#[path = "tile_detector_tests.rs"]
mod tests;
