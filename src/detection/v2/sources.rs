//! Pixel-derived compact-source mask and FWHM estimate (plan R2.3).
//!
//! Seeds are 4-connected groups of at least `source_min_pixels` pixels with
//! (x - B)/sigma above `source_seed_snr`. The FWHM is the median, over
//! isolated seeds, of 2.3548 * sqrt((Ixx + Iyy)/2) from positive-residual
//! second moments in a 15x15 box about the peak, bounded to [1, 20] px.
//! Seeds are dilated by ceil(source_dilation_fwhm * FWHM). The mask excludes
//! sources from background fits and protects them from automatic removal; it
//! deletes nothing from the science image.
use super::background::Background;
use super::bitset::Bitset;
use super::noise::{second_differences, sigma_from_differences};
use crate::config::V2Rules;
use crate::detection::robust::median;
use rayon::prelude::*;
use std::collections::VecDeque;

pub struct SourceMask {
    pub mask: Bitset,
    pub fwhm: f64,
    pub seeds: usize,
}

/// Local noise of each 16x16 block from second differences (see `noise`),
/// independent of candidate exclusions. Second differences cancel smooth gradients (galaxy
/// halos, vignetting), so extended sources stay seeded, while amplified noise
/// raises the estimate and is not mistaken for sources.
fn local_block_noise(data: &[f64], width: usize, height: usize, quantum: f64) -> (usize, Vec<f64>) {
    const B: usize = 16;
    let cols = width.div_ceil(B);
    let rows = height.div_ceil(B);
    let out = (0..rows)
        .into_par_iter()
        .map(|by| {
            let (mut diffs, mut scratch) = (Vec::with_capacity(B * B), Vec::with_capacity(B * B));
            (0..cols)
                .map(|bx| {
                    diffs.clear();
                    let (x0, x1) = (bx * B, ((bx + 1) * B).min(width));
                    let (y0, y1) = (by * B, ((by + 1) * B).min(height));
                    second_differences(data, width, x0, x1, y0, y1, |_| false, &mut diffs);
                    sigma_from_differences(&diffs, quantum, &mut scratch).0
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>()
        .into_iter()
        .flatten()
        .collect();
    (cols, out)
}

/// Local-background validation of a seed component: its peak residual must
/// exceed `source_seed_snr` times the second-difference noise measured in a
/// 6-px margin around the component's bounding box (seed pixels excluded).
/// Genuine compact sources pass; clumps of excursions inside amplified-noise
/// regions (where block noise straddles quiet and noisy areas) do not.
fn locally_significant(
    data: &[f64],
    width: usize,
    height: usize,
    bg: &Background,
    seed: &Bitset,
    component: &[usize],
    rules: &V2Rules,
) -> bool {
    const MARGIN: usize = 6;
    let (mut x0, mut x1, mut y0, mut y1) = (usize::MAX, 0usize, usize::MAX, 0usize);
    let mut peak = f64::NEG_INFINITY;
    for &i in component {
        let (x, y) = (i % width, i / width);
        x0 = x0.min(x);
        x1 = x1.max(x);
        y0 = y0.min(y);
        y1 = y1.max(y);
        peak = peak.max(data[i] - bg.level(x as f64, y as f64));
    }
    let (bx0, bx1) = (x0.saturating_sub(MARGIN), (x1 + MARGIN + 1).min(width));
    let (by0, by1) = (y0.saturating_sub(MARGIN), (y1 + MARGIN + 1).min(height));
    let mut diffs = Vec::new();
    second_differences(data, width, bx0, bx1, by0, by1, |i| seed.get(i), &mut diffs);
    let (ring_sigma, _) = sigma_from_differences(&diffs, bg.quantum, &mut Vec::new());
    !ring_sigma.is_finite() || peak > rules.source_seed_snr * ring_sigma
}

pub fn detect(
    data: &[f64],
    width: usize,
    height: usize,
    bg: &Background,
    rules: &V2Rules,
) -> SourceMask {
    let n = width * height;
    let mut seed = Bitset::new(n);
    let (block_cols, block_noise) = local_block_noise(data, width, height, bg.quantum);
    let seeds_by_row: Vec<Vec<usize>> = (0..height)
        .into_par_iter()
        .map(|y| {
            let mut row = Vec::new();
            for x in 0..width {
                let i = y * width + x;
                let value = data[i];
                if !value.is_finite() {
                    continue;
                }
                let (fx, fy) = (x as f64, y as f64);
                let local = block_noise[(y / 16) * block_cols + x / 16];
                let trusted = bg.noise(fx, fy);
                let sigma = if local.is_finite() {
                    trusted.max(local)
                } else {
                    trusted
                };
                if sigma > 0.0 && (value - bg.level(fx, fy)) / sigma > rules.source_seed_snr {
                    row.push(i);
                }
            }
            row
        })
        .collect();
    for i in seeds_by_row.into_iter().flatten() {
        seed.set(i);
    }
    let mut visited = Bitset::new(n);
    let mut components: Vec<Vec<usize>> = Vec::new();
    let mut queue = VecDeque::new();
    for start in 0..n {
        if !seed.get(start) || visited.get(start) {
            continue;
        }
        visited.set(start);
        queue.push_back(start);
        let mut component = Vec::new();
        while let Some(i) = queue.pop_front() {
            component.push(i);
            let (x, y) = (i % width, i / width);
            for (nx, ny) in [
                (x.wrapping_sub(1), y),
                (x + 1, y),
                (x, y.wrapping_sub(1)),
                (x, y + 1),
            ] {
                if nx < width && ny < height {
                    let j = ny * width + nx;
                    if seed.get(j) && !visited.get(j) {
                        visited.set(j);
                        queue.push_back(j);
                    }
                }
            }
        }
        if component.len() >= rules.source_min_pixels
            && locally_significant(data, width, height, bg, &seed, &component, rules)
        {
            components.push(component);
        }
    }
    let mut widths = Vec::new();
    for component in components.iter().filter(|c| c.len() <= 400) {
        let &peak = component
            .iter()
            .max_by(|&&a, &&b| data[a].total_cmp(&data[b]))
            .expect("component is non-empty");
        let (px, py) = (peak % width, peak / width);
        if px < 7 || py < 7 || px + 7 >= width || py + 7 >= height {
            continue;
        }
        let (mut s, mut sx, mut sy, mut sxx, mut syy) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for y in py - 7..=py + 7 {
            for x in px - 7..=px + 7 {
                let i = y * width + x;
                if !data[i].is_finite() {
                    continue;
                }
                let r = data[i] - bg.level(x as f64, y as f64);
                if r <= 0.0 {
                    continue;
                }
                let (dx, dy) = (x as f64 - px as f64, y as f64 - py as f64);
                s += r;
                sx += r * dx;
                sy += r * dy;
                sxx += r * dx * dx;
                syy += r * dy * dy;
            }
        }
        if s > 0.0 {
            let (mx, my) = (sx / s, sy / s);
            let var = 0.5 * ((sxx / s - mx * mx) + (syy / s - my * my));
            if var > 0.0 {
                widths.push(2.3548 * var.sqrt());
            }
        }
    }
    let fwhm = if widths.is_empty() {
        3.0
    } else {
        median(&mut widths)
    }
    .clamp(1.0, 20.0);
    let radius = (rules.source_dilation_fwhm * fwhm).ceil() as isize;
    let mut mask = Bitset::new(n);
    let r2 = radius * radius;
    for component in &components {
        for &i in component {
            mask.set(i);
            let (ux, uy) = (i % width, i / width);
            // Interior seed pixels are covered by their boundary's disks.
            let interior = ux > 0
                && uy > 0
                && ux + 1 < width
                && uy + 1 < height
                && seed.get(i - 1)
                && seed.get(i + 1)
                && seed.get(i - width)
                && seed.get(i + width);
            if interior {
                continue;
            }
            let (x, y) = (ux as isize, uy as isize);
            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    if dx * dx + dy * dy > r2 {
                        continue;
                    }
                    let (nx, ny) = (x + dx, y + dy);
                    if nx >= 0 && ny >= 0 && (nx as usize) < width && (ny as usize) < height {
                        mask.set(ny as usize * width + nx as usize);
                    }
                }
            }
        }
    }
    SourceMask {
        mask,
        fwhm,
        seeds: components.len(),
    }
}
