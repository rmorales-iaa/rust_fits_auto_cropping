//! Gradient-immune pixel noise from second differences (plan R2.2).
//!
//! For a window, horizontal and vertical second differences
//! d = x[i-1] - 2 x[i] + x[i+1] use only triplets whose three samples are
//! finite and not skipped. For white Gaussian noise Var(d) = 6 sigma^2, so
//! sigma = 1.4826 MAD(d) / sqrt(6). Linear gradients cancel exactly and smooth
//! curvature contributes only its second derivative, so unmodelled vignetting
//! ramps or halos do not masquerade as noise. Overlapping differences are
//! correlated (lag-one correlation -2/3), so the effective sample size used
//! for ln-sigma uncertainty is conservatively half the number of differences.
use crate::detection::robust::location_scale_with;

/// Append second differences inside `[x0, x1) x [y0, y1)` to `out`.
#[allow(clippy::too_many_arguments)]
pub fn second_differences(
    data: &[f64],
    width: usize,
    x0: usize,
    x1: usize,
    y0: usize,
    y1: usize,
    skip: impl Fn(usize) -> bool,
    out: &mut Vec<f64>,
) {
    let usable = |i: usize| data[i].is_finite() && !skip(i);
    for y in y0..y1 {
        for x in x0..x1 {
            let i = y * width + x;
            if !usable(i) {
                continue;
            }
            if x > x0 && x + 1 < x1 && usable(i - 1) && usable(i + 1) {
                out.push(data[i - 1] - 2.0 * data[i] + data[i + 1]);
            }
            if y > y0 && y + 1 < y1 && usable(i - width) && usable(i + width) {
                out.push(data[i - width] - 2.0 * data[i] + data[i + width]);
            }
        }
    }
}

/// (sigma, effective sample size) from second differences; NaN sigma when
/// fewer than 8 differences are available.
///
/// For quantized data (`quantum > 0`) the MAD of integer differences moves in
/// whole quanta, which is too coarse for ln-sigma comparisons at low counts.
/// There the smooth clipped mean-absolute estimator
/// sigma = 1.2533 * mean|d| / sqrt(6) (Gaussian: E|d| = sigma_d sqrt(2/pi))
/// is used over |d| <= max(5 MAD-sigma_d, 3 quanta).
pub fn sigma_from_differences(
    differences: &[f64],
    quantum: f64,
    scratch: &mut Vec<f64>,
) -> (f64, f64) {
    if differences.len() < 8 {
        return (f64::NAN, 0.0);
    }
    let (_, mad_sigma) = location_scale_with(differences.iter().copied(), scratch);
    let n_eff = differences.len() as f64 / 2.0;
    if quantum > 0.0 {
        let clip = (5.0 * mad_sigma).max(3.0 * quantum);
        let (sum, count) = differences
            .iter()
            .filter(|d| d.abs() <= clip)
            .fold((0.0, 0usize), |(s, c), d| (s + d.abs(), c + 1));
        if count >= 8 {
            return (1.2533 * (sum / count as f64) / 6.0_f64.sqrt(), n_eff);
        }
    }
    (mad_sigma / 6.0_f64.sqrt(), n_eff)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_gradients_cancel_and_white_noise_scale_is_recovered() {
        let (w, h) = (64, 64);
        let mut rng = crate::detection::v2::tests::Rng(3);
        let data: Vec<f64> = (0..w * h)
            .map(|i| 7.0 * (i % w) as f64 - 3.0 * (i / w) as f64 + 10.0 * rng.gauss())
            .collect();
        let mut diffs = Vec::new();
        second_differences(&data, w, 0, w, 0, h, |_| false, &mut diffs);
        let (sigma, n_eff) = sigma_from_differences(&diffs, 0.0, &mut Vec::new());
        assert!((sigma - 10.0).abs() < 0.5, "{sigma}");
        assert_eq!(n_eff, diffs.len() as f64 / 2.0);
    }
}
