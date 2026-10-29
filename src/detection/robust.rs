//! Small deterministic robust statistics helpers.
//!
//! Medians use O(n) selection (`select_nth_unstable_by` with `f64::total_cmp`)
//! instead of a full sort. The selected order statistics are the same values a
//! total-order sort would place at those ranks, so results are bit-identical to
//! the former sort-based implementation (property-tested below).

/// Median of `values` (reorders the slice). Even counts use the midpoint
/// `lower * 0.5 + upper * 0.5`, which cannot overflow for finite inputs.
pub fn median(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }
    let m = values.len() / 2;
    let even = values.len().is_multiple_of(2);
    let (lower, upper, _) = values.select_nth_unstable_by(m, f64::total_cmp);
    let upper = *upper;
    if even {
        let below = lower
            .iter()
            .copied()
            .max_by(f64::total_cmp)
            .expect("an even non-empty slice has a lower half");
        below * 0.5 + upper * 0.5
    } else {
        upper
    }
}

/// Median and 1.4826-scaled MAD of the finite values.
pub fn location_scale(values: &[f64]) -> (f64, f64) {
    let mut scratch = Vec::with_capacity(values.len());
    location_scale_with(values.iter().copied(), &mut scratch)
}

/// Median and scaled MAD of the finite values produced by `values`, reusing
/// `scratch` to avoid per-call allocation. `scratch` is overwritten.
pub fn location_scale_with(
    values: impl Iterator<Item = f64>,
    scratch: &mut Vec<f64>,
) -> (f64, f64) {
    scratch.clear();
    scratch.extend(values.filter(|v| v.is_finite()));
    let center = median(scratch);
    for value in scratch.iter_mut() {
        *value = (*value - center).abs();
    }
    let sigma = 1.4826 * median(scratch);
    (center, sigma)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted_median(values: &mut [f64]) -> f64 {
        if values.is_empty() {
            return f64::NAN;
        }
        values.sort_by(f64::total_cmp);
        let m = values.len() / 2;
        if values.len().is_multiple_of(2) {
            values[m - 1] * 0.5 + values[m] * 0.5
        } else {
            values[m]
        }
    }

    fn sorted_location_scale(values: &[f64]) -> (f64, f64) {
        let mut finite: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
        let center = sorted_median(&mut finite);
        let mut deviations: Vec<f64> = finite.iter().map(|v| (v - center).abs()).collect();
        (center, 1.4826 * sorted_median(&mut deviations))
    }

    #[test]
    fn median_and_mad_resist_an_outlier() {
        let (center, sigma) = location_scale(&[1.0, 1.0, 2.0, 2.0, 1000.0]);
        assert_eq!(center, 2.0);
        assert!(sigma < 2.0);
    }

    #[test]
    fn selection_is_bit_identical_to_sorting_reference() {
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            state >> 11
        };
        let mut scratch = Vec::new();
        for trial in 0..2_000 {
            let n = (next() % 70) as usize;
            let values: Vec<f64> = (0..n)
                .map(|_| match next() % 9 {
                    0 => f64::NAN,
                    1 => -0.0,
                    2 => 0.0,
                    3 => f64::INFINITY,
                    // Heavy ties exercise equal order statistics.
                    4 | 5 => (next() % 4) as f64,
                    _ => (next() as f64 / (1u64 << 40) as f64) - 2_000.0,
                })
                .collect();
            let expected = sorted_location_scale(&values);
            let actual = location_scale_with(values.iter().copied(), &mut scratch);
            assert_eq!(
                (expected.0.to_bits(), expected.1.to_bits()),
                (actual.0.to_bits(), actual.1.to_bits()),
                "trial {trial}: {values:?}"
            );
            let mut a = values.clone();
            let mut b = values.clone();
            assert_eq!(median(&mut a).to_bits(), sorted_median(&mut b).to_bits());
        }
    }
}
