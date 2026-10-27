//! Allocation-free, scale-conditioned lag-one Pearson correlation.

/// Average valid horizontal and vertical lag-one correlations within a tile.
/// Each direction is omitted when it has fewer than eight finite pairs or no
/// representable variance. Values are rescaled before the Welford covariance
/// pass, avoiding overflow from squaring large but finite pixel values.
pub fn lag_one_correlation(
    values: &[f64],
    width: usize,
    height: usize,
    x0: usize,
    x1: usize,
    y0: usize,
    y1: usize,
) -> Option<f64> {
    if width.checked_mul(height) != Some(values.len()) || x1 > width || y1 > height {
        return None;
    }
    let horizontal = axis_correlation(values, width, x0, x1, y0, y1, true);
    let vertical = axis_correlation(values, width, x0, x1, y0, y1, false);
    match (horizontal, vertical) {
        (Some(a), Some(b)) => Some((a * 0.5 + b * 0.5).clamp(-1.0, 1.0)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn axis_correlation(
    values: &[f64],
    width: usize,
    x0: usize,
    x1: usize,
    y0: usize,
    y1: usize,
    horizontal: bool,
) -> Option<f64> {
    let mut count = 0usize;
    let mut scale = 0.0_f64;
    for y in y0..y1 {
        for x in x0..x1 {
            let next = if horizontal {
                (x + 1 < x1).then_some((y * width + x, y * width + x + 1))
            } else {
                (y + 1 < y1).then_some((y * width + x, (y + 1) * width + x))
            };
            if let Some((a, b)) = next {
                let (a, b) = (values[a], values[b]);
                if a.is_finite() && b.is_finite() {
                    count += 1;
                    scale = scale.max(a.abs()).max(b.abs());
                }
            }
        }
    }
    if count < 8 || scale == 0.0 {
        return None;
    }

    let (mut n, mut mean_a, mut mean_b) = (0.0, 0.0, 0.0);
    let (mut covariance, mut variance_a, mut variance_b) = (0.0, 0.0, 0.0);
    for y in y0..y1 {
        for x in x0..x1 {
            let next = if horizontal {
                (x + 1 < x1).then_some((y * width + x, y * width + x + 1))
            } else {
                (y + 1 < y1).then_some((y * width + x, (y + 1) * width + x))
            };
            if let Some((a, b)) = next {
                let (a, b) = (values[a], values[b]);
                if a.is_finite() && b.is_finite() {
                    let (a, b) = (a / scale, b / scale);
                    n += 1.0;
                    let delta_a = a - mean_a;
                    let delta_b = b - mean_b;
                    mean_a += delta_a / n;
                    mean_b += delta_b / n;
                    covariance += delta_a * (b - mean_b);
                    variance_a += delta_a * (a - mean_a);
                    variance_b += delta_b * (b - mean_b);
                }
            }
        }
    }
    if variance_a <= 0.0 || variance_b <= 0.0 {
        return None;
    }
    let value = covariance / (variance_a * variance_b).sqrt();
    value.is_finite().then(|| value.clamp(-1.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smooth_ramp_is_correlated_and_constant_tile_is_undefined() {
        let ramp: Vec<f64> = (0..100).map(|i| i as f64).collect();
        assert!(lag_one_correlation(&ramp, 10, 10, 0, 10, 0, 10).unwrap() > 0.99);
        assert_eq!(
            lag_one_correlation(&vec![1.0; 100], 10, 10, 0, 10, 0, 10),
            None
        );
    }

    #[test]
    fn large_finite_samples_do_not_overflow_covariance() {
        let values: Vec<f64> = (0..100)
            .map(|index| 1.0e200 + index as f64 * 1.0e185)
            .collect();
        let correlation = lag_one_correlation(&values, 10, 10, 0, 10, 0, 10).unwrap();
        assert!(correlation.is_finite());
        assert!(correlation > 0.99);
    }

    #[test]
    fn one_direction_can_supply_the_minimum_pair_count() {
        let values: Vec<f64> = (0..20).map(|index| index as f64).collect();
        assert!(lag_one_correlation(&values, 20, 1, 0, 20, 0, 1).unwrap() > 0.99);
    }
}
