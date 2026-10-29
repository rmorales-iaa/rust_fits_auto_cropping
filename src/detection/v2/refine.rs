//! Native-resolution hysteresis boundary refinement (plan R3.2/R3.4).
//!
//! Within a band of `tile + 8` pixels on both sides of a component's tile
//! boundary, each pixel gets a level score
//!   z_pL = median_{5x5}(x - B) / (1.2533 sigma / sqrt(n_eff))
//! and a dispersion score
//!   z_pV = ln(1.4826 MAD_{7x7}(x - B) / sigma) / sqrt(1.36 / n_eff)
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
use crate::config::{UncertainPolicy, V2Rules};
use crate::detection::robust::location_scale_with;
use rayon::prelude::*;
use std::collections::VecDeque;

/// Sign pattern of a component's evidence: -1, 0 or +1 for level and dispersion.
#[derive(Clone, Copy, Debug)]
pub struct Signature {
    pub level: i8,
    pub dispersion: i8,
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
                let mut score = |radius: usize, level: bool| -> Option<f64> {
                    window.clear();
                    for wy in y.saturating_sub(radius)..=(y + radius).min(height - 1) {
                        for wx in x.saturating_sub(radius)..=(x + radius).min(width - 1) {
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
                    let (median, spread) =
                        location_scale_with(window.iter().copied(), &mut scratch);
                    Some(if level {
                        median / (1.2533 * sigma / n_eff.sqrt())
                    } else {
                        (spread.max(1e-300) / sigma).ln() / (1.36 / n_eff).sqrt()
                    })
                };
                let mut best = 0.0_f64; // in units of the high threshold
                if signature.level != 0
                    && let Some(z) = score(2, true)
                {
                    best = best.max(f64::from(signature.level) * z / high_level);
                }
                if signature.dispersion != 0
                    && let Some(z) = score(3, false)
                {
                    best = best.max(f64::from(signature.dispersion) * z / high_disp);
                }
                if best >= 1.0 {
                    3
                } else if best >= low {
                    2
                } else {
                    1
                }
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
