//! Native-resolution hysteresis boundary refinement (plan R3.2/R3.4).
//!
//! Within a band of `tile + 8` pixels on both sides of a component's tile
//! boundary, each pixel gets a level score
//!   z_pL = median_{5x5}(x - B) / (1.2533 sigma / sqrt(n_eff))
//! and a dispersion score from gradient-immune second differences in a 7x7
//! window,
//!   z_pV = ln(s_diff / sigma_diff) / sqrt(1.36 / n_eff)
//! over finite, non-source window samples. A pixel is strong when the
//! component's signature (sign of level and/or dispersion anomaly) reaches the
//! high threshold, weak when it reaches `hysteresis_low_fraction` of it.
//! Growth starts from the component core (tile pixels deeper than the band)
//! and strong in-component band pixels, and proceeds 4-connected through weak
//! band pixels; it never crosses protected sources or undefined samples.
//! Weak-but-not-strong reached pixels form the uncertain band.
use super::background::Background;
use super::bitset::Bitset;
use super::distance::banded_signed_distance;
use super::noise::{second_differences, sigma_from_differences};
use crate::config::{UncertainPolicy, V2Rules};
use crate::detection::robust::location_scale_with;
use rayon::prelude::*;
use std::collections::VecDeque;

/// Sign pattern of a component's evidence: -1, 0 or +1 for level and dispersion.
#[derive(Clone, Copy, Debug)]
pub struct Signature {
    pub level: i8,
    pub dispersion: i8,
    /// Median residual of the component core: the frame's own zero point for
    /// relative illumination when the core is unilluminated (level deficit).
    pub core_residual: Option<f64>,
}

/// Relative-illumination class of a pixel (decision D4): 3 = strong
/// (eta < eta_I, invalid), 2 = weak (eta_I <= eta < eta_V, uncertain),
/// 1 = valid side.
fn eta_class(eta: f64, eta_invalid: f64, eta_valid: f64) -> u8 {
    if eta < eta_invalid {
        3
    } else if eta < eta_valid {
        2
    } else {
        1
    }
}

/// D4 invalid boundary: eta_I = 0.4 * eta_V (0.2 for the shipped 0.5).
fn eta_invalid(eta_valid: f64) -> f64 {
    0.4 * eta_valid
}

pub struct Refined {
    pub mask: Vec<bool>,
    pub uncertain: usize,
}

#[allow(clippy::too_many_arguments)]
pub fn refine(
    data: &[f64],
    width: usize,
    height: usize,
    tile_mask: &[bool],
    tile_size: usize,
    signature: Signature,
    bg: &Background,
    sources: &Bitset,
    rules: &V2Rules,
) -> Refined {
    let band_px = tile_size + 8;
    let band = band_px as f64;
    let banded = banded_signed_distance(tile_mask, width, height, band_px);
    // Signed distance within the band; beyond it, +/- infinity by side.
    let signed = |i: usize| {
        banded.get(i, tile_mask[i]).unwrap_or(if tile_mask[i] {
            f64::INFINITY
        } else {
            f64::NEG_INFINITY
        })
    };
    let n = width * height;
    let high_level = rules.level_threshold;
    let high_disp = rules.dispersion_threshold;
    let low = rules.hysteresis_low_fraction;
    // 0 = not evaluated, 1 = below low, 2 = weak, 3 = strong. Rows are
    // independent and collected in order (deterministic for any worker count).
    let in_band = |i: usize| signed(i).abs() <= band;
    let classify_row = |y: usize| -> Vec<u8> {
        let mut window = Vec::with_capacity(49);
        let mut scratch = Vec::with_capacity(49);
        (0..width)
            .map(|x| {
                let i = y * width + x;
                if !in_band(i) || !data[i].is_finite() || sources.get(i) {
                    return 0;
                }
                let sigma = bg.noise(x as f64, y as f64).max(1e-300);
                let mut level_score = || -> Option<(f64, f64)> {
                    window.clear();
                    for wy in y.saturating_sub(2)..=(y + 2).min(height - 1) {
                        for wx in x.saturating_sub(2)..=(x + 2).min(width - 1) {
                            let j = wy * width + wx;
                            if data[j].is_finite() && !sources.get(j) {
                                window.push(data[j] - bg.level(wx as f64, wy as f64));
                            }
                        }
                    }
                    if window.len() < 9 {
                        return None;
                    }
                    let n_eff = bg.effective_n(window.len());
                    let (median, _) = location_scale_with(window.iter().copied(), &mut scratch);
                    Some((median / (1.2533 * sigma / n_eff.sqrt()), median))
                };
                let level_z = if signature.level != 0 {
                    level_score()
                } else {
                    None
                };
                let dispersion_z = if signature.dispersion != 0 {
                    window.clear();
                    let (x0, x1) = (x.saturating_sub(3), (x + 4).min(width));
                    let (y0, y1) = (y.saturating_sub(3), (y + 4).min(height));
                    second_differences(
                        data,
                        width,
                        x0,
                        x1,
                        y0,
                        y1,
                        |j| sources.get(j),
                        &mut window,
                    );
                    let (s_diff, n_diff) =
                        sigma_from_differences(&window, bg.quantum, &mut scratch);
                    let reference = bg.diff_noise(x as f64, y as f64).max(1e-300);
                    (s_diff.is_finite() && n_diff > 0.0).then(|| {
                        let n_eff = bg.effective_n(n_diff as usize);
                        let ratio = s_diff.max(1e-300) / reference;
                        (ratio.ln() / (1.36 / n_eff).sqrt(), ratio)
                    })
                } else {
                    None
                };
                // Significance class (units of the high threshold) and the
                // D4 relative-illumination class; a pixel is strong/weak only
                // when it is both significant and on the invalid side of eta.
                let significance = |z: f64, sign: i8, high: f64| -> u8 {
                    let t = f64::from(sign) * z / high;
                    if t >= 1.0 {
                        3
                    } else if t >= low {
                        2
                    } else {
                        1
                    }
                };
                let eta_i = eta_invalid(rules.eta_valid);
                let mut class = 1u8;
                if let Some((z, residual)) = level_z {
                    let sig = significance(z, signature.level, high_level);
                    let eta_level = match signature.core_residual {
                        // eta = 1 - r / r0, with r0 the unilluminated core.
                        Some(r0) if signature.level < 0 && r0 < 0.0 => {
                            eta_class(1.0 - residual / r0, eta_i, rules.eta_valid)
                        }
                        _ => 3,
                    };
                    class = class.max(sig.min(eta_level));
                }
                if let Some((z, ratio)) = dispersion_z {
                    let sig = significance(z, signature.dispersion, high_disp);
                    // Noise-dominated sky: raw ratio^2 ~ eta; flat-divided ~ 1/eta.
                    let eta = if signature.dispersion < 0 {
                        ratio * ratio
                    } else {
                        1.0 / (ratio * ratio)
                    };
                    class = class.max(sig.min(eta_class(eta, eta_i, rules.eta_valid)));
                }
                class
            })
            .collect()
    };
    let class: Vec<u8> = (0..height)
        .into_par_iter()
        .map(classify_row)
        .collect::<Vec<_>>()
        .into_iter()
        .flatten()
        .collect();
    let mut reached = vec![0u8; n];
    let mut queue = VecDeque::new();
    for i in 0..n {
        let core = tile_mask[i] && signed(i) > band && data[i].is_finite();
        let strong_seed = tile_mask[i] && class[i] == 3;
        if core || strong_seed {
            reached[i] = if core { 3 } else { class[i] };
            queue.push_back(i);
        }
    }
    while let Some(i) = queue.pop_front() {
        let (x, y) = (i % width, i / width);
        for (nx, ny) in [
            (x.wrapping_sub(1), y),
            (x + 1, y),
            (x, y.wrapping_sub(1)),
            (x, y + 1),
        ] {
            if nx >= width || ny >= height {
                continue;
            }
            let j = ny * width + nx;
            if reached[j] != 0 {
                continue;
            }
            let interior_core = tile_mask[j] && signed(j) > band && data[j].is_finite();
            if interior_core || class[j] >= 2 {
                reached[j] = if interior_core { 3 } else { class[j] };
                queue.push_back(j);
            }
        }
    }
    let mut uncertain = 0;
    let mask: Vec<bool> = reached
        .iter()
        .map(|&r| match r {
            3 => true,
            2 => {
                uncertain += 1;
                rules.uncertain_policy == UncertainPolicy::ExcludeUncertain
            }
            _ => false,
        })
        .collect();
    Refined { mask, uncertain }
}
