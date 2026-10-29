//! Experimental v2 border detector (plan R2, R3, R4.5).
//!
//! Pipeline: (1) source-masked mesh background, robust surface and noise map;
//! (2) pixel-derived source mask; (3) two-sided standard-error tile evidence
//! with source protection; (4) edge-connected components, with deficit-only
//! components requiring a boundary change-point test; (5) refit excluding
//! accepted candidates (bounded iterations); (6) native-resolution hysteresis
//! refinement. A frame without adequate trusted reference support abstains.
//! Thresholds are registered starting values pending null false-alarm
//! calibration; nothing here is a calibrated probability.
mod background;
pub(crate) mod bitset;
mod distance;
mod evidence;
mod noise;
mod refine;
mod sources;

use super::robust::median;
use super::tile_detector::Detection;
use crate::config::HeuristicRules;
use bitset::Bitset;
use evidence::Rule;

fn empty(len: usize, reason_code: &'static str, reason: String) -> Detection {
    Detection {
        mask: vec![false; len],
        candidate_mask_pixels: 0,
        candidate_tiles: 0,
        candidate_components: 0,
        accepted_components: 0,
        candidate_fraction: 0.0,
        accepted: false,
        reason_code,
        reason,
        score: 0.0,
        evidence_ratio: 0.0,
        artifact_benefit: 0.0,
        valid_pixel_loss: 0.0,
        net_utility: 0.0,
        uncertain_boundary_pixels: 0,
        protected_source_pixels: 0,
    }
}

/// Stored-value quantum: 1 when every finite analysis value is an integer.
fn quantum(data: &[f64]) -> f64 {
    let integral = data
        .iter()
        .filter(|v| v.is_finite())
        .all(|v| v.fract() == 0.0);
    if integral { 1.0 } else { 0.0 }
}

pub fn detect_v2(data: &[f64], width: usize, height: usize, cfg: &HeuristicRules) -> Detection {
    let rules = &cfg.v2;
    let n = data.len();
    if width.checked_mul(height) != Some(n) || width < 48 || height < 48 {
        return empty(
            n,
            "insufficient_data",
            "image too small for the v2 mesh model".into(),
        );
    }
    let defined = data.iter().filter(|v| v.is_finite()).count();
    if defined == 0 {
        return empty(n, "insufficient_data", "no defined samples".into());
    }
    let q = quantum(data);
    let tile = cfg.tile_size;
    let mut excluded = Bitset::new(n);
    let mut accepted_tiles: Vec<usize> = Vec::new();
    let mut state = None;
    for iteration in 0..rules.refit_iterations {
        let Some(bg) = background::fit(data, width, height, &excluded, q, rules) else {
            return empty(
                n,
                "reference_support_abstained",
                format!(
                    "v2 reference support insufficient (iteration {iteration}): fewer than {} trusted meshes spanning two mesh widths per axis",
                    rules.min_trusted_meshes
                ),
            );
        };
        let source_mask = sources::detect(data, width, height, &bg, rules);
        let grid =
            evidence::evaluate_tiles(data, width, height, tile, &bg, &source_mask.mask, q, rules);
        let candidate: Vec<bool> = grid.tiles.iter().map(|t| t.rule != Rule::None).collect();
        let mut components = evidence::edge_components(&grid, &candidate);
        // Deficit-only evidence must show a step, not a smooth gradient.
        let mut kept = Vec::new();
        for component in components.drain(..) {
            let deficit_only = component
                .iter()
                .filter(|&&t| grid.tiles[t].rule == Rule::Deficit)
                .count();
            if deficit_only * 2 < component.len() {
                kept.push(component);
                continue;
            }
            let pixels = evidence::tile_pixels(&grid, &component, width, height);
            let ratio = evidence::boundary_step_ratio(
                data,
                width,
                height,
                &pixels,
                &bg,
                &source_mask.mask,
                2 * tile,
            );
            if ratio.is_some_and(|(r, jump)| {
                r >= 1.0 + rules.boundary_min_cv_gain && jump >= rules.boundary_min_jump_fraction
            }) {
                kept.push(component);
            } else {
                let rest: Vec<usize> = component
                    .into_iter()
                    .filter(|&t| grid.tiles[t].rule != Rule::Deficit)
                    .collect();
                if !rest.is_empty() {
                    kept.push(rest);
                }
            }
        }
        // Size gates per independent component.
        let min_pixels = (cfg.minimum_mask_fraction * defined as f64).ceil() as usize;
        let accepted: Vec<Vec<usize>> = kept
            .into_iter()
            .filter(|c| {
                let pixels: usize = c.iter().map(|&t| grid.tiles[t].samples).sum();
                c.len() >= cfg.minimum_border_tiles && pixels >= min_pixels
            })
            .collect();
        let mut tiles: Vec<usize> = accepted.iter().flatten().copied().collect();
        tiles.sort_unstable();
        let stable = tiles == accepted_tiles;
        accepted_tiles = tiles;
        let candidate_count = candidate.iter().filter(|c| **c).count();
        state = Some((bg, source_mask, grid, accepted, candidate_count));
        if stable || iteration + 1 == rules.refit_iterations {
            break;
        }
        // Refit without the accepted candidate pixels (plan R2.1 step 6).
        excluded = Bitset::new(n);
        let (_, _, grid, _, _) = state.as_ref().expect("state set above");
        for &t in &accepted_tiles {
            let (x0, x1, y0, y1) = grid.bounds(t, width, height);
            for y in y0..y1 {
                for x in x0..x1 {
                    excluded.set(y * width + x);
                }
            }
        }
    }
    let (bg, source_mask, grid, accepted, candidate_count) =
        state.expect("at least one iteration ran");
    let protected_source_pixels = source_mask.mask.count();
    let score = grid
        .tiles
        .iter()
        .map(|t| t.z_level.abs())
        .fold(0.0, f64::max)
        .min(1e6);
    if accepted.is_empty() {
        let mut result = empty(
            n,
            if candidate_count == 0 {
                "no_border_candidate"
            } else {
                "candidate_rejected"
            },
            format!(
                "v2: {candidate_count} candidate tiles; no edge component passed the evidence, boundary and size gates (surface degree {}, {} trusted meshes, rho {:.2}, FWHM {:.1} px)",
                bg.degree, bg.trusted_count, bg.rho, source_mask.fwhm
            ),
        );
        result.candidate_tiles = candidate_count;
        result.score = score;
        result.protected_source_pixels = protected_source_pixels;
        return result;
    }
    let mut rule_counts = [0usize; 4];
    for &t in accepted.iter().flatten() {
        match grid.tiles[t].rule {
            Rule::Variance => rule_counts[0] += 1,
            Rule::Noise => rule_counts[1] += 1,
            Rule::Deficit => rule_counts[2] += 1,
            Rule::Constant => rule_counts[3] += 1,
            Rule::None => {}
        }
    }
    let mut mask = vec![false; n];
    let mut uncertain = 0usize;
    let mut evidence = Vec::new();
    for component in &accepted {
        let pixels = evidence::tile_pixels(&grid, component, width, height);
        let mut levels: Vec<f64> = component.iter().map(|&t| grid.tiles[t].z_level).collect();
        let mut dispersions: Vec<f64> = component
            .iter()
            .map(|&t| grid.tiles[t].z_dispersion)
            .collect();
        let level = median(&mut levels);
        let dispersion = median(&mut dispersions);
        let sign = |z: f64, threshold: f64| -> i8 {
            if z >= threshold {
                1
            } else if z <= -threshold {
                -1
            } else {
                0
            }
        };
        // Zero point for relative illumination: median residual of the
        // component's tile pixels deeper than one tile from its boundary
        // (or all its tile pixels if none), excluding sources.
        let core_residual = {
            let mut in_component = vec![false; grid.tiles.len()];
            for &t in component {
                in_component[t] = true;
            }
            // Neighbours beyond the frame edge count as inside: edge
            // components extend to the edge.
            let inside = |tx: isize, ty: isize| {
                tx < 0
                    || ty < 0
                    || tx >= grid.cols as isize
                    || ty >= grid.rows as isize
                    || in_component[ty as usize * grid.cols + tx as usize]
            };
            let mut values: Vec<f64> = Vec::new();
            for &t in component {
                let (tx, ty) = ((t % grid.cols) as isize, (t / grid.cols) as isize);
                let interior = inside(tx - 1, ty)
                    && inside(tx + 1, ty)
                    && inside(tx, ty - 1)
                    && inside(tx, ty + 1);
                if !interior && component.len() > 8 {
                    continue;
                }
                let (x0, x1, y0, y1) = grid.bounds(t, width, height);
                for y in y0..y1 {
                    for x in x0..x1 {
                        let i = y * width + x;
                        if data[i].is_finite() && !source_mask.mask.get(i) {
                            values.push(data[i] - bg.level(x as f64, y as f64));
                        }
                    }
                }
            }
            (!values.is_empty()).then(|| median(&mut values))
        };
        let signature = refine::Signature {
            level: sign(level, cfg.v2.level_threshold),
            dispersion: sign(dispersion, cfg.v2.dispersion_threshold),
            core_residual,
        };
        let refined = if signature.level == 0 && signature.dispersion == 0 {
            // Constant-plateau components: keep the tile mask on defined samples.
            refine::Refined {
                mask: pixels.clone(),
                uncertain: 0,
            }
        } else {
            refine::refine(
                data,
                width,
                height,
                &pixels,
                tile,
                signature,
                &bg,
                &source_mask.mask,
                rules,
            )
        };
        uncertain += refined.uncertain;
        for (m, r) in mask.iter_mut().zip(&refined.mask) {
            *m |= *r;
        }
        for &t in component {
            let e = &grid.tiles[t];
            evidence.push(
                (e.z_level.abs() / rules.level_threshold)
                    .max(e.z_dispersion.abs() / rules.dispersion_threshold),
            );
        }
    }
    for (m, value) in mask.iter_mut().zip(data) {
        *m &= value.is_finite();
    }
    let selected = mask.iter().filter(|m| **m).count();
    let fraction = selected as f64 / defined as f64;
    let evidence_ratio = median(&mut evidence)
        .min(cfg.maximum_evidence_ratio)
        .max(0.0);
    let artifact_benefit = fraction * evidence_ratio;
    let valid_pixel_loss =
        cfg.valid_pixel_loss_cost * fraction / (1.0 - fraction).max(f64::EPSILON);
    let candidate_pixels: usize = accepted
        .iter()
        .flatten()
        .map(|&t| grid.tiles[t].samples)
        .sum();
    let mut result = Detection {
        candidate_mask_pixels: candidate_pixels.max(selected),
        mask,
        candidate_tiles: candidate_count,
        candidate_components: accepted.len(),
        accepted_components: accepted.len(),
        candidate_fraction: candidate_pixels as f64 / defined as f64,
        accepted: selected > 0 && fraction >= cfg.minimum_mask_fraction,
        reason_code: "candidate_accepted",
        reason: format!(
            "v2 accepted {} edge component(s) [tiles by rule: variance {}, noise {}, deficit {}, constant {}]: {selected} pixels after native-resolution refinement ({uncertain} uncertain boundary pixels, policy {:?}); surface degree {}, {} trusted meshes, structure sigma {:.3}, rho {:.2}, FWHM {:.1} px, {} source seeds protected",
            accepted.len(),
            rule_counts[0],
            rule_counts[1],
            rule_counts[2],
            rule_counts[3],
            rules.uncertain_policy,
            bg.degree,
            bg.trusted_count,
            bg.structure_sigma,
            bg.rho,
            source_mask.fwhm,
            source_mask.seeds,
        ),
        score,
        evidence_ratio,
        artifact_benefit,
        valid_pixel_loss,
        net_utility: artifact_benefit - valid_pixel_loss,
        uncertain_boundary_pixels: uncertain,
        protected_source_pixels,
    };
    if !result.accepted {
        result.mask.fill(false);
        result.reason_code = "candidate_rejected";
        result
            .reason
            .push_str("; refined mask below minimum mask fraction");
    }
    result
}

#[cfg(test)]
mod calibration_tests;
#[cfg(test)]
mod tests;
