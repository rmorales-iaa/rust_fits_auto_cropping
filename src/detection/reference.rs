//! Experimental robust affine reference fit for local tile contrast.
use super::robust::location_scale;

const HUBER_CUTOFF: f64 = 1.345;
const MAX_IRLS_ITERATIONS: usize = 10;
const COEFFICIENT_TOLERANCE: f64 = 1e-10;

#[derive(Clone, Copy, Debug)]
pub(super) struct AffineReference {
    coefficients: [f64; 3],
    value_scale: f64,
    pub residual_scale: f64,
    pub typical_noise: f64,
}

impl AffineReference {
    pub fn predict(self, x: f64, y: f64) -> Option<f64> {
        let normalized = self.coefficients[0] + self.coefficients[1] * x + self.coefficients[2] * y;
        let prediction = normalized * self.value_scale;
        prediction.is_finite().then_some(prediction)
    }
}

/// Fit a plane to normalized mesh centers using finite samples and bounded Huber IRLS.
/// `points` must already be centered/scaled to [-1, 1] on each axis. Returns `None`
/// when support is insufficient, the design is rank deficient, or arithmetic is non-finite.
pub(super) fn fit_robust_affine(
    points: &[(f64, f64)],
    values: &[f64],
    dispersions: &[f64],
) -> Option<AffineReference> {
    if points.len() != values.len() || values.len() != dispersions.len() {
        return None;
    }
    let samples: Vec<_> = points
        .iter()
        .zip(values)
        .zip(dispersions)
        .filter_map(|((&(x, y), &value), &dispersion)| {
            (x.is_finite() && y.is_finite() && value.is_finite() && dispersion.is_finite())
                .then_some((x, y, value, dispersion.max(0.0)))
        })
        .collect();
    if samples.len() < 6 {
        return None;
    }
    let value_scale = samples
        .iter()
        .map(|sample| sample.2.abs())
        .fold(0.0_f64, f64::max)
        .max(1.0);
    let normalized: Vec<f64> = samples
        .iter()
        .map(|sample| sample.2 / value_scale)
        .collect();
    let coordinates: Vec<(f64, f64)> = samples.iter().map(|sample| (sample.0, sample.1)).collect();
    let noise: Vec<f64> = samples
        .iter()
        .map(|sample| sample.3 / value_scale)
        .collect();
    let mut weights = vec![1.0; samples.len()];
    let mut coefficients = weighted_plane(&coordinates, &normalized, &weights)?;

    for _ in 0..MAX_IRLS_ITERATIONS {
        let mut residuals: Vec<f64> = coordinates
            .iter()
            .zip(&normalized)
            .map(|(&(x, y), value)| value - evaluate(coefficients, x, y))
            .collect();
        if residuals.iter().any(|value| !value.is_finite()) {
            return None;
        }
        let (residual_center, residual_scale) = location_scale(&residuals);
        if !residual_center.is_finite() || !residual_scale.is_finite() {
            return None;
        }
        // Recenter the robust residual location into the intercept before weighting.
        coefficients[0] += residual_center;
        residuals = coordinates
            .iter()
            .zip(&normalized)
            .map(|(&(x, y), value)| value - evaluate(coefficients, x, y))
            .collect();
        let scale_floor = f64::EPSILON
            * normalized
                .iter()
                .map(|value| value.abs())
                .fold(1.0_f64, f64::max);
        let scale = residual_scale.max(scale_floor);
        for (weight, residual) in weights.iter_mut().zip(&residuals) {
            let standardized = residual.abs() / (HUBER_CUTOFF * scale);
            *weight = if standardized <= 1.0 {
                1.0
            } else {
                1.0 / standardized
            };
        }
        let updated = weighted_plane(&coordinates, &normalized, &weights)?;
        let delta = coefficients
            .iter()
            .zip(updated)
            .map(|(old, new)| (old - new).abs())
            .fold(0.0_f64, f64::max);
        let magnitude = coefficients
            .iter()
            .map(|value| value.abs())
            .fold(1.0_f64, f64::max);
        coefficients = updated;
        if delta <= COEFFICIENT_TOLERANCE * magnitude {
            break;
        }
    }

    let residuals: Vec<f64> = coordinates
        .iter()
        .zip(&normalized)
        .map(|(&(x, y), value)| value - evaluate(coefficients, x, y))
        .collect();
    let (_, residual_scale) = location_scale(&residuals);
    let (typical_noise, _) = location_scale(&noise);
    if !residual_scale.is_finite() || !typical_noise.is_finite() {
        return None;
    }
    let scale_floor = f64::EPSILON * value_scale;
    let residual_scale = (residual_scale.max(typical_noise) * value_scale).max(scale_floor);
    if !residual_scale.is_finite() {
        return None;
    }
    Some(AffineReference {
        coefficients,
        value_scale,
        residual_scale,
        typical_noise: typical_noise * value_scale,
    })
}

fn evaluate(coefficients: [f64; 3], x: f64, y: f64) -> f64 {
    coefficients[0] + coefficients[1] * x + coefficients[2] * y
}

fn weighted_plane(points: &[(f64, f64)], values: &[f64], weights: &[f64]) -> Option<[f64; 3]> {
    if points.len() != values.len() || values.len() != weights.len() || points.len() < 3 {
        return None;
    }
    let mut matrix = [[0.0_f64; 4]; 3];
    for ((&(x, y), &value), &weight) in points.iter().zip(values).zip(weights) {
        if !x.is_finite()
            || !y.is_finite()
            || !value.is_finite()
            || !weight.is_finite()
            || weight < 0.0
        {
            return None;
        }
        let row = [1.0, x, y];
        for i in 0..3 {
            for j in 0..3 {
                matrix[i][j] += weight * row[i] * row[j];
            }
            matrix[i][3] += weight * row[i] * value;
        }
    }
    solve_three(matrix)
}

fn solve_three(mut matrix: [[f64; 4]; 3]) -> Option<[f64; 3]> {
    let matrix_scale = matrix
        .iter()
        .flat_map(|row| row[..3].iter())
        .map(|value| value.abs())
        .fold(0.0_f64, f64::max);
    if !matrix_scale.is_finite() || matrix_scale == 0.0 {
        return None;
    }
    let threshold = 128.0 * f64::EPSILON * matrix_scale;
    for column in 0..3 {
        let pivot_row = (column..3).max_by(|&left, &right| {
            matrix[left][column]
                .abs()
                .total_cmp(&matrix[right][column].abs())
        })?;
        if matrix[pivot_row][column].abs() <= threshold {
            return None;
        }
        matrix.swap(column, pivot_row);
        let pivot = matrix[column][column];
        for value in &mut matrix[column][column..=3] {
            *value /= pivot;
        }
        let pivot_values = matrix[column];
        for (row_index, row) in matrix.iter_mut().enumerate() {
            if row_index == column {
                continue;
            }
            let factor = row[column];
            for col in column..=3 {
                row[col] -= factor * pivot_values[col];
            }
        }
    }
    let solution = [matrix[0][3], matrix[1][3], matrix[2][3]];
    solution
        .iter()
        .all(|value| value.is_finite())
        .then_some(solution)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn robust_affine_fit_recovers_plane_despite_one_interior_outlier() {
        let points: Vec<_> = (0..4)
            .flat_map(|y| (0..4).map(move |x| (x as f64 / 1.5 - 1.0, y as f64 / 1.5 - 1.0)))
            .collect();
        let mut values: Vec<_> = points
            .iter()
            .map(|&(x, y)| 100.0 + 12.0 * x - 7.0 * y)
            .collect();
        values[5] += 500.0;
        let fit = fit_robust_affine(&points, &values, &vec![0.5; values.len()]).unwrap();
        assert!((fit.predict(0.4, -0.2).unwrap() - (100.0 + 12.0 * 0.4 + 1.4)).abs() < 0.1);
        assert!(fit.residual_scale.is_finite());
        assert!(fit.typical_noise.is_finite());
    }

    #[test]
    fn robust_affine_fit_abstains_on_rank_deficient_support() {
        let points: Vec<_> = (0..8).map(|x| (x as f64 / 4.0 - 1.0, 0.0)).collect();
        assert!(fit_robust_affine(&points, &[1.0; 8], &[0.1; 8]).is_none());
    }

    #[test]
    fn robust_affine_fit_rejects_nonfinite_or_insufficient_support() {
        let points = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0), (0.5, 0.5)];
        assert!(fit_robust_affine(&points, &[1.0; 5], &[0.1; 5]).is_none());
        let mut values = [1.0; 6];
        values[0] = f64::NAN;
        let points = [
            (-1.0, -1.0),
            (0.0, -1.0),
            (1.0, -1.0),
            (-1.0, 1.0),
            (0.0, 1.0),
            (1.0, 1.0),
        ];
        assert!(fit_robust_affine(&points, &values, &[0.1; 6]).is_none());
    }
}
