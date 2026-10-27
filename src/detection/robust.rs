//! Small deterministic robust statistics helpers.
pub fn median(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }
    values.sort_by(f64::total_cmp);
    let m = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[m - 1] + values[m]) * 0.5
    } else {
        values[m]
    }
}

pub fn location_scale(values: &[f64]) -> (f64, f64) {
    let mut finite: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    let center = median(&mut finite);
    let mut deviations: Vec<f64> = finite.iter().map(|v| (v - center).abs()).collect();
    let sigma = 1.4826 * median(&mut deviations);
    (center, sigma)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn median_and_mad_resist_an_outlier() {
        let (center, sigma) = location_scale(&[1.0, 1.0, 2.0, 2.0, 1000.0]);
        assert_eq!(center, 2.0);
        assert!(sigma < 2.0);
    }
}
