//! Lag-one spatial Pearson correlation, reported only when both axes have variance.
pub fn lag_one_correlation(
    values: &[f64],
    width: usize,
    height: usize,
    x0: usize,
    x1: usize,
    y0: usize,
    y1: usize,
) -> Option<f64> {
    if x1 > width || y1 > height {
        return None;
    }
    let mut correlations = Vec::new();
    for horizontal in [true, false] {
        let mut a = Vec::new();
        let mut b = Vec::new();
        for y in y0..y1 {
            for x in x0..x1 {
                if horizontal && x + 1 < x1 {
                    pair(
                        values[y * width + x],
                        values[y * width + x + 1],
                        &mut a,
                        &mut b,
                    );
                }
                if !horizontal && y + 1 < y1 {
                    pair(
                        values[y * width + x],
                        values[(y + 1) * width + x],
                        &mut a,
                        &mut b,
                    );
                }
            }
        }
        if a.len() < 8 {
            continue;
        }
        let ma = a.iter().sum::<f64>() / a.len() as f64;
        let mb = b.iter().sum::<f64>() / b.len() as f64;
        let (mut cov, mut va, mut vb) = (0.0, 0.0, 0.0);
        for (x, y) in a.iter().zip(&b) {
            let da = x - ma;
            let db = y - mb;
            cov += da * db;
            va += da * da;
            vb += db * db;
        }
        if va > 0.0 && vb > 0.0 {
            correlations.push((cov / (va * vb).sqrt()).clamp(-1.0, 1.0));
        }
    }
    (!correlations.is_empty()).then(|| correlations.iter().sum::<f64>() / correlations.len() as f64)
}
fn pair(x: f64, y: f64, a: &mut Vec<f64>, b: &mut Vec<f64>) {
    if x.is_finite() && y.is_finite() {
        a.push(x);
        b.push(y)
    }
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
}
