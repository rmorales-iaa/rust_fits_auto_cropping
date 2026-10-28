//! Deterministic IRAF/Astropy-style ZScale interval estimation.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZScale {
    pub minimum: f64,
    pub maximum: f64,
}

impl ZScale {
    pub fn from_values(values: &[f64]) -> Option<Self> {
        Self::from_iter_factory(|| values.iter().copied())
    }

    /// Estimate a stretch from a replayable sample source without materializing
    /// its entire finite-pixel set. This keeps large crop previews bounded to
    /// the deterministic 1,000-sample ZScale working set.
    pub fn from_iter_factory<F, I>(values: F) -> Option<Self>
    where
        F: Fn() -> I,
        I: Iterator<Item = f64>,
    {
        const MAX_SAMPLES: usize = 1000;
        const CONTRAST: f64 = 0.25;
        const MAX_REJECT: f64 = 0.5;
        const MIN_PIXELS: usize = 5;
        const REJECTION_SIGMA: f64 = 2.5;
        const MAX_ITERATIONS: usize = 5;

        let finite_count = values().filter(|value| value.is_finite()).count();
        if finite_count == 0 {
            return None;
        }
        let stride = (finite_count / MAX_SAMPLES).max(1);
        let mut samples: Vec<f64> = values()
            .filter(|value| value.is_finite())
            .enumerate()
            .filter_map(|(index, value)| (index % stride == 0).then_some(value))
            .take(MAX_SAMPLES)
            .collect();
        samples.sort_by(f64::total_cmp);
        let n = samples.len();
        let (mut minimum, mut maximum) = (samples[0], samples[n - 1]);
        let min_good = MIN_PIXELS.max((n as f64 * MAX_REJECT) as usize);
        let grow = ((n as f64 * 0.01) as usize).max(1);
        let mut rejected = vec![false; n];
        let mut good_count = n;
        let mut previous_good_count = n + 1;
        let mut fit_slope = 0.0;

        for _ in 0..MAX_ITERATIONS {
            if good_count >= previous_good_count || good_count < min_good {
                break;
            }
            let Some((slope, intercept)) = fit_line(&samples, &rejected) else {
                break;
            };
            fit_slope = slope;
            let residuals: Vec<f64> = samples
                .iter()
                .enumerate()
                .map(|(i, sample)| sample - (intercept + slope * i as f64))
                .collect();
            let mean = residuals
                .iter()
                .zip(&rejected)
                .filter_map(|(value, bad)| (!bad).then_some(*value))
                .sum::<f64>()
                / good_count as f64;
            let sigma = (residuals
                .iter()
                .zip(&rejected)
                .filter_map(|(value, bad)| (!bad).then_some((value - mean).powi(2)))
                .sum::<f64>()
                / good_count as f64)
                .sqrt();
            let threshold = REJECTION_SIGMA * sigma;
            for i in 0..n {
                if residuals[i] < -threshold || residuals[i] > threshold {
                    rejected[i] = true;
                }
            }
            rejected = dilate(&rejected, grow);
            previous_good_count = good_count;
            good_count = rejected.iter().filter(|bad| !**bad).count();
        }

        if good_count < min_good {
            return Some(Self { minimum, maximum });
        }
        if CONTRAST > 0.0 {
            fit_slope /= CONTRAST;
        }
        let center = (n - 1) / 2;
        let median = if n.is_multiple_of(2) {
            (samples[n / 2 - 1] + samples[n / 2]) / 2.0
        } else {
            samples[n / 2]
        };
        minimum = minimum.max(median - (center as f64 - 1.0) * fit_slope);
        maximum = maximum.min(median + (n - center) as f64 * fit_slope);
        Some(Self { minimum, maximum })
    }

    pub fn to_u8(self, value: f64) -> u8 {
        if !value.is_finite() {
            return 0;
        }
        let span = self.maximum - self.minimum;
        if !span.is_finite() || span <= 0.0 {
            return 0;
        }
        (((value - self.minimum) / span).clamp(0.0, 1.0) * 255.0).round() as u8
    }
}

fn fit_line(values: &[f64], rejected: &[bool]) -> Option<(f64, f64)> {
    let mut n = 0.0;
    let (mut sx, mut sy, mut sxx, mut sxy) = (0.0, 0.0, 0.0, 0.0);
    for (index, (&value, &bad)) in values.iter().zip(rejected).enumerate() {
        if bad {
            continue;
        }
        let x = index as f64;
        n += 1.0;
        sx += x;
        sy += value;
        sxx += x * x;
        sxy += x * value;
    }
    if n < 2.0 {
        return None;
    }
    let denominator = sxx - sx * sx / n;
    if denominator == 0.0 {
        return Some((0.0, sy / n));
    }
    let slope = (sxy - sx * sy / n) / denominator;
    Some((slope, (sy - slope * sx) / n))
}

fn dilate(rejected: &[bool], width: usize) -> Vec<bool> {
    let left = (width - 1) / 2;
    let right = width - left - 1;
    (0..rejected.len())
        .map(|i| {
            let start = i.saturating_sub(right);
            let end = i.saturating_add(left + 1).min(rejected.len());
            rejected[start..end].iter().any(|bad| *bad)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_image_uses_a_degenerate_full_sample_interval() {
        let scale = ZScale::from_values(&[42.0; 64]).unwrap();
        assert_eq!(
            scale,
            ZScale {
                minimum: 42.0,
                maximum: 42.0
            }
        );
        assert_eq!(scale.to_u8(42.0), 0);
    }

    #[test]
    fn ignores_non_finite_values_and_is_deterministic() {
        let values: Vec<f64> = (0..256).map(|i| (i as f64).sin()).collect();
        let mut with_invalid = values.clone();
        with_invalid[0] = f64::NAN;
        with_invalid[1] = f64::INFINITY;
        assert_eq!(
            ZScale::from_values(&with_invalid),
            ZScale::from_values(&with_invalid)
        );
        assert!(ZScale::from_values(&with_invalid).is_some());
        assert!(ZScale::from_values(&[f64::NAN, f64::INFINITY]).is_none());
        assert_eq!(
            ZScale::from_values(&with_invalid),
            ZScale::from_iter_factory(|| with_invalid.iter().copied())
        );
    }

    #[test]
    fn limits_contain_a_linear_ramp_and_reject_a_sparse_outlier() {
        let mut values: Vec<f64> = (0..1000).map(|i| 10.0 + i as f64 * 0.01).collect();
        values[500] = 1000.0;
        let scale = ZScale::from_values(&values).unwrap();
        assert!((scale.minimum - 10.0).abs() < 1e-12);
        assert!((scale.maximum - 35.070_570_743_319_97).abs() < 1e-8);
    }
}
