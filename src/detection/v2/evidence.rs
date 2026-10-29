//! Two-sided, standard-error-based tile evidence and components (plan R2.2,
//! R2.4, R3.1).
//!
//! For tile t with n_t finite non-source samples, residual median r_t and
//! dispersion s_t (1.4826 MAD of x - B):
//!   z_L = r_t / sqrt(Var_B + (pi/2) sigma^2 / n_eff)
//!   z_V = ln(s_t / sigma) / sqrt(1.36/n_eff + 1.36/n_ref)
//! where sigma is the interpolated trusted noise, Var_B the background
//! prediction variance, n_eff the AR(1) effective sample size, pi/2 the
//! asymptotic median-to-mean variance ratio and 1.36/n the asymptotic
//! variance of ln(MAD) for Gaussian samples. These are scores in standard
//! error units under a Gaussian working model, not calibrated p-values.
use super::background::Background;
use super::bitset::Bitset;
use super::distance::banded_signed_distance;
use crate::config::V2Rules;
use crate::detection::robust::location_scale_with;
use rayon::prelude::*;
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, Default)]
pub struct TileEvidence {
    pub z_level: f64,
    pub z_dispersion: f64,
    pub source_fraction: f64,
    pub samples: usize,
    pub constant: bool,
    pub rule: Rule,
}

/// Which registered rule made a tile a candidate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Rule {
    #[default]
    None,
    /// Level and dispersion anomaly together.
    Variance,
    /// Noise excess alone (flat-amplified border at sky level).
    Noise,
    /// Strong deficit; requires a passed boundary change-point test.
    Deficit,
    /// Exactly constant tile against a non-constant reference.
    Constant,
}

pub struct TileGrid {
    pub size: usize,
    pub cols: usize,
    pub rows: usize,
    pub tiles: Vec<TileEvidence>,
}

impl TileGrid {
    pub fn bounds(
        &self,
        index: usize,
        width: usize,
        height: usize,
    ) -> (usize, usize, usize, usize) {
        let (tx, ty) = (index % self.cols, index / self.cols);
        (
            tx * self.size,
            ((tx + 1) * self.size).min(width),
            ty * self.size,
            ((ty + 1) * self.size).min(height),
        )
    }
}

#[allow(clippy::too_many_arguments)]
pub fn evaluate_tiles(
    data: &[f64],
    width: usize,
    height: usize,
    tile_size: usize,
    bg: &Background,
    sources: &Bitset,
    quantum: f64,
    rules: &V2Rules,
) -> TileGrid {
    let cols = width.div_ceil(tile_size);
    let rows = height.div_ceil(tile_size);
    let reference_n = {
        let mut counts: Vec<f64> = bg
            .meshes
            .iter()
            .filter(|m| m.trusted)
            .map(|m| m.count as f64)
            .collect();
        crate::detection::robust::median(&mut counts)
    };
    let n_ref = bg.effective_n(reference_n as usize);
    // Tile rows are independent; ordered collection keeps results identical
    // for any worker count.
    let tile_row = |ty: usize| -> Vec<TileEvidence> {
        let mut residuals = Vec::with_capacity(tile_size * tile_size);
        let mut scratch = Vec::with_capacity(tile_size * tile_size);
        let mut row = Vec::with_capacity(cols);
        for tx in 0..cols {
            let (x0, x1, y0, y1) = (
                tx * tile_size,
                ((tx + 1) * tile_size).min(width),
                ty * tile_size,
                ((ty + 1) * tile_size).min(height),
            );
            residuals.clear();
            let (mut finite, mut source) = (0usize, 0usize);
            let (mut first, mut constant) = (f64::NAN, true);
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = y * width + x;
                    if !data[i].is_finite() {
                        continue;
                    }
                    finite += 1;
                    if sources.get(i) {
                        source += 1;
                        continue;
                    }
                    if first.is_nan() {
                        first = data[i];
                    } else if data[i] != first {
                        constant = false;
                    }
                    residuals.push(data[i] - bg.level(x as f64, y as f64));
                }
            }
            let mut tile = TileEvidence {
                samples: residuals.len(),
                source_fraction: if finite == 0 {
                    0.0
                } else {
                    source as f64 / finite as f64
                },
                constant: constant && !residuals.is_empty(),
                ..TileEvidence::default()
            };
            if residuals.len() >= 16 {
                let (xc, yc) = ((x0 + x1 - 1) as f64 * 0.5, (y0 + y1 - 1) as f64 * 0.5);
                let floor = (quantum / 12.0_f64.sqrt()).max(1e-12 * (1.0 + bg.level(xc, yc).abs()));
                let sigma = bg.noise(xc, yc).max(floor);
                let n_eff = bg.effective_n(residuals.len());
                let (median, spread) = location_scale_with(residuals.iter().copied(), &mut scratch);
                let level_se = (bg.prediction_var(xc, yc)
                    + std::f64::consts::FRAC_PI_2 * sigma * sigma / n_eff)
                    .sqrt();
                tile.z_level = median / level_se.max(floor);
                tile.z_dispersion =
                    (spread.max(floor) / sigma).ln() / (1.36 / n_eff + 1.36 / n_ref).sqrt();
                let protected = tile.z_level > 0.0 && tile.source_fraction >= 0.5;
                tile.rule = if tile.constant && sigma > floor * 1.0001 && median.abs() > floor {
                    Rule::Constant
                } else if protected {
                    Rule::None
                } else if tile.z_dispersion.abs() >= rules.dispersion_threshold
                    && tile.z_level.abs() >= rules.level_threshold
                {
                    Rule::Variance
                } else if tile.z_dispersion >= rules.noise_only_threshold
                    && tile.source_fraction < 0.5
                {
                    Rule::Noise
                } else if tile.z_level <= -rules.strong_level_threshold {
                    Rule::Deficit
                } else {
                    Rule::None
                };
            }
            row.push(tile);
        }
        row
    };
    let tiles: Vec<TileEvidence> = (0..rows)
        .into_par_iter()
        .map(tile_row)
        .collect::<Vec<_>>()
        .into_iter()
        .flatten()
        .collect();
    TileGrid {
        size: tile_size,
        cols,
        rows,
        tiles,
    }
}

/// 4-connected components of candidate tiles that touch the frame edge.
pub fn edge_components(grid: &TileGrid, candidate: &[bool]) -> Vec<Vec<usize>> {
    let (cols, rows) = (grid.cols, grid.rows);
    let mut seen = vec![false; candidate.len()];
    let mut out = Vec::new();
    let mut queue = VecDeque::new();
    for y in 0..rows {
        for x in 0..cols {
            let start = y * cols + x;
            let edge = x == 0 || y == 0 || x + 1 == cols || y + 1 == rows;
            if !edge || !candidate[start] || seen[start] {
                continue;
            }
            seen[start] = true;
            queue.push_back(start);
            let mut component = Vec::new();
            while let Some(i) = queue.pop_front() {
                component.push(i);
                let (cx, cy) = (i % cols, i / cols);
                for (nx, ny) in [
                    (cx.wrapping_sub(1), cy),
                    (cx + 1, cy),
                    (cx, cy.wrapping_sub(1)),
                    (cx, cy + 1),
                ] {
                    if nx < cols && ny < rows {
                        let j = ny * cols + nx;
                        if candidate[j] && !seen[j] {
                            seen[j] = true;
                            queue.push_back(j);
                        }
                    }
                }
            }
            out.push(component);
        }
    }
    out
}

/// Pixel mask of a set of tiles.
pub fn tile_pixels(grid: &TileGrid, tiles: &[usize], width: usize, height: usize) -> Vec<bool> {
    let mut mask = vec![false; width * height];
    for &t in tiles {
        let (x0, x1, y0, y1) = grid.bounds(t, width, height);
        for y in y0..y1 {
            mask[y * width + x0..y * width + x1].fill(true);
        }
    }
    mask
}

fn weighted_poly(points: &[(f64, f64, f64)], degree: usize) -> Option<Vec<f64>> {
    let p = degree + 1;
    let mut a = vec![0.0; p * p];
    let mut b = vec![0.0; p];
    for &(x, y, w) in points {
        let powers: Vec<f64> = (0..p).map(|k| x.powi(k as i32)).collect();
        for i in 0..p {
            b[i] += w * powers[i] * y;
            for j in 0..p {
                a[i * p + j] += w * powers[i] * powers[j];
            }
        }
    }
    for col in 0..p {
        let pivot =
            (col..p).max_by(|&r1, &r2| a[r1 * p + col].abs().total_cmp(&a[r2 * p + col].abs()))?;
        if a[pivot * p + col].abs() < 1e-12 {
            return None;
        }
        for j in 0..p {
            a.swap(col * p + j, pivot * p + j);
        }
        b.swap(col, pivot);
        for r in col + 1..p {
            let f = a[r * p + col] / a[col * p + col];
            for j in col..p {
                a[r * p + j] -= f * a[col * p + j];
            }
            b[r] -= f * b[col];
        }
    }
    let mut c = vec![0.0; p];
    for i in (0..p).rev() {
        let s: f64 = (i + 1..p).map(|j| a[i * p + j] * c[j]).sum();
        c[i] = (b[i] - s) / a[i * p + i];
    }
    c.iter().all(|v| v.is_finite()).then_some(c)
}

fn poly_at(c: &[f64], x: f64) -> f64 {
    c.iter()
        .enumerate()
        .map(|(k, v)| v * x.powi(k as i32))
        .sum()
}

/// Step model: separate lines on each side of the best split.
fn fit_step(points: &[(f64, f64, f64)]) -> Option<(f64, Vec<f64>, Vec<f64>)> {
    let mut best: Option<(f64, f64, Vec<f64>, Vec<f64>)> = None;
    for k in 2..points.len().saturating_sub(1) {
        let split = 0.5 * (points[k - 1].0 + points[k].0);
        let (left, right) = points.split_at(k);
        let (Some(cl), Some(cr)) = (weighted_poly(left, 1), weighted_poly(right, 1)) else {
            continue;
        };
        let sse: f64 = left
            .iter()
            .map(|&(x, y, w)| w * (y - poly_at(&cl, x)).powi(2))
            .sum::<f64>()
            + right
                .iter()
                .map(|&(x, y, w)| w * (y - poly_at(&cr, x)).powi(2))
                .sum::<f64>();
        if best.as_ref().is_none_or(|b| sse < b.0) {
            best = Some((sse, split, cl, cr));
        }
    }
    best.map(|(_, split, cl, cr)| (split, cl, cr))
}

/// Blocked change-point test across a component boundary (plan R3.1).
/// Profiles median residual by signed distance d in [-L, L] (inside > 0).
/// Returns the ratio CV_smooth / CV_step of 5-fold interleaved
/// cross-validated weighted squared errors (quadratic versus two-line step).
pub fn boundary_step_ratio(
    data: &[f64],
    width: usize,
    height: usize,
    component_pixels: &[bool],
    bg: &Background,
    sources: &Bitset,
    half_width: usize,
) -> Option<f64> {
    let banded = banded_signed_distance(component_pixels, width, height, half_width);
    let bins = 2 * half_width + 1;
    let mut values: Vec<Vec<f64>> = vec![Vec::new(); bins];
    for i in 0..component_pixels.len() {
        let Some(d) = banded.get(i, component_pixels[i]) else {
            continue;
        };
        let k = d.round();
        if k.abs() > half_width as f64 || !data[i].is_finite() || sources.get(i) {
            continue;
        }
        let (x, y) = ((i % width) as f64, (i / width) as f64);
        values[(k + half_width as f64) as usize].push(data[i] - bg.level(x, y));
    }
    let mut scratch = Vec::new();
    let points: Vec<(f64, f64, f64)> = values
        .iter()
        .enumerate()
        .filter(|(_, v)| v.len() >= 8)
        .map(|(k, v)| {
            let (m, _) = location_scale_with(v.iter().copied(), &mut scratch);
            (k as f64 - half_width as f64, m, v.len() as f64)
        })
        .collect();
    if points.len() < 10 || !points.iter().any(|p| p.0 > 0.0) || !points.iter().any(|p| p.0 < 0.0) {
        return None;
    }
    let (mut cv_smooth, mut cv_step) = (0.0, 0.0);
    for fold in 0..5 {
        let train: Vec<_> = points
            .iter()
            .enumerate()
            .filter(|(i, _)| i % 5 != fold)
            .map(|(_, p)| *p)
            .collect();
        let test: Vec<_> = points
            .iter()
            .enumerate()
            .filter(|(i, _)| i % 5 == fold)
            .map(|(_, p)| *p)
            .collect();
        let smooth = weighted_poly(&train, 2)?;
        let (split, left, right) = fit_step(&train)?;
        for &(x, y, w) in &test {
            cv_smooth += w * (y - poly_at(&smooth, x)).powi(2);
            let step = if x < split {
                poly_at(&left, x)
            } else {
                poly_at(&right, x)
            };
            cv_step += w * (y - step).powi(2);
        }
    }
    Some(if cv_step > 0.0 {
        cv_smooth / cv_step
    } else if cv_smooth > 0.0 {
        f64::INFINITY
    } else {
        1.0
    })
}
