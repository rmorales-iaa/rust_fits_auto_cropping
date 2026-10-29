//! Source-masked mesh background, robust surface and noise map (plan R2.1).
//!
//! Units are centred stored analysis values. Coordinates are zero-based pixel
//! centres; the surface uses coordinates normalised to [-1, 1] over the frame.
use super::bitset::Bitset;
use crate::config::V2Rules;
use crate::detection::robust::location_scale_with;
use rayon::prelude::*;

/// Robust statistics for one b x b mesh.
#[derive(Clone, Copy, Debug)]
pub struct Mesh {
    pub median: f64,
    pub sigma: f64,
    pub count: usize,
    pub trusted: bool,
}

/// Fitted background and noise description shared by later stages.
#[derive(Clone, Debug)]
pub struct Background {
    pub mesh_size: usize,
    pub cols: usize,
    pub rows: usize,
    pub meshes: Vec<Mesh>,
    pub coefficients: Vec<f64>,
    pub degree: usize,
    /// Trusted support bounding box in normalised coordinates; evaluation
    /// outside it is clamped (constant extrapolation) and penalised.
    pub support: [f64; 4],
    /// Robust scatter of trusted mesh medians about the surface, net of
    /// their sampling variance (background structure noise).
    pub structure_sigma: f64,
    /// Interpolation grid of mesh noise (trusted values, gaps filled).
    pub noise_grid: Vec<f64>,
    /// Pooled lag-one residual autocorrelation, clamped to [0, 0.9].
    pub rho: f64,
    pub trusted_count: usize,
    pub width: usize,
    pub height: usize,
    /// Mean sampling variance of a trusted mesh median (for SE_B).
    pub mean_mesh_sampling_var: f64,
}

pub fn normalized(x: f64, extent: usize) -> f64 {
    2.0 * x / extent.saturating_sub(1).max(1) as f64 - 1.0
}

fn basis(u: f64, v: f64, degree: usize, out: &mut [f64; 6]) -> usize {
    out[0] = 1.0;
    if degree == 0 {
        return 1;
    }
    out[1] = u;
    out[2] = v;
    if degree == 1 {
        return 3;
    }
    out[3] = u * u;
    out[4] = u * v;
    out[5] = v * v;
    6
}

/// Weighted least squares with partial-pivot elimination; `None` when the
/// normal matrix is singular or badly conditioned.
fn solve_weighted(
    points: &[(f64, f64)],
    values: &[f64],
    weights: &[f64],
    degree: usize,
) -> Option<Vec<f64>> {
    let mut row = [0.0; 6];
    let p = basis(0.0, 0.0, degree, &mut row);
    let mut a = vec![0.0; p * p];
    let mut b = vec![0.0; p];
    for ((&(u, v), &y), &w) in points.iter().zip(values).zip(weights) {
        basis(u, v, degree, &mut row);
        for i in 0..p {
            b[i] += w * row[i] * y;
            for j in 0..p {
                a[i * p + j] += w * row[i] * row[j];
            }
        }
    }
    let scale = (0..p).map(|i| a[i * p + i].abs()).fold(0.0_f64, f64::max);
    if scale.is_nan() || scale <= 0.0 {
        return None;
    }
    for col in 0..p {
        let pivot =
            (col..p).max_by(|&r1, &r2| a[r1 * p + col].abs().total_cmp(&a[r2 * p + col].abs()))?;
        if a[pivot * p + col].abs() <= 1e-10 * scale {
            return None;
        }
        if pivot != col {
            for j in 0..p {
                a.swap(col * p + j, pivot * p + j);
            }
            b.swap(col, pivot);
        }
        for r in col + 1..p {
            let f = a[r * p + col] / a[col * p + col];
            for j in col..p {
                a[r * p + j] -= f * a[col * p + j];
            }
            b[r] -= f * b[col];
        }
    }
    let mut x = vec![0.0; p];
    for i in (0..p).rev() {
        let mut sum = b[i];
        for j in i + 1..p {
            sum -= a[i * p + j] * x[j];
        }
        x[i] = sum / a[i * p + i];
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}

fn evaluate(coefficients: &[f64], degree: usize, u: f64, v: f64) -> f64 {
    let mut row = [0.0; 6];
    let p = basis(u, v, degree, &mut row);
    (0..p).map(|i| coefficients[i] * row[i]).sum()
}

/// Bounded Huber IRLS (c = 1.345 in robust residual units, <= 10 updates).
fn huber_fit(points: &[(f64, f64)], values: &[f64], degree: usize) -> Option<Vec<f64>> {
    let mut weights = vec![1.0; values.len()];
    let mut coefficients = solve_weighted(points, values, &weights, degree)?;
    let mut scratch = Vec::with_capacity(values.len());
    for _ in 0..10 {
        let residuals: Vec<f64> = points
            .iter()
            .zip(values)
            .map(|(&(u, v), y)| y - evaluate(&coefficients, degree, u, v))
            .collect();
        let (_, scale) = location_scale_with(residuals.iter().copied(), &mut scratch);
        if scale.is_nan() || scale <= 0.0 {
            break;
        }
        for (w, r) in weights.iter_mut().zip(&residuals) {
            let t = r.abs() / scale;
            *w = if t <= 1.345 { 1.0 } else { 1.345 / t };
        }
        let next = solve_weighted(points, values, &weights, degree)?;
        let change = next
            .iter()
            .zip(&coefficients)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f64::max);
        coefficients = next;
        if change <= 1e-9 * (1.0 + coefficients[0].abs()) {
            break;
        }
    }
    Some(coefficients)
}

/// Iterative kappa-sigma clipped median and 1.4826*MAD; zero-MAD quantized
/// samples fall back to 1.2533*mean absolute deviation with a q/sqrt(12) floor.
pub fn clipped_stats(
    samples: &mut Vec<f64>,
    kappa: f64,
    iterations: usize,
    quantum: f64,
    scratch: &mut Vec<f64>,
) -> (f64, f64) {
    let floor = quantum / 12.0_f64.sqrt();
    let mut median = f64::NAN;
    let mut sigma = f64::NAN;
    for _ in 0..iterations {
        let before = samples.len();
        let (m, s) = location_scale_with(samples.iter().copied(), scratch);
        median = m;
        sigma = s;
        if samples.is_empty() {
            break;
        }
        if sigma.is_nan() || sigma <= 0.0 {
            let mad =
                samples.iter().map(|v| (v - median).abs()).sum::<f64>() / samples.len() as f64;
            sigma = (1.2533 * mad).max(floor);
        }
        let limit = kappa * sigma;
        samples.retain(|v| (v - median).abs() <= limit);
        if samples.len() == before {
            break;
        }
    }
    (median, sigma.max(floor))
}

/// Estimate the background from finite samples not in `excluded`.
pub fn fit(
    data: &[f64],
    width: usize,
    height: usize,
    excluded: &Bitset,
    quantum: f64,
    rules: &V2Rules,
) -> Option<Background> {
    let b = rules.mesh_size_px.min(width.min(height) / 4).max(16);
    if width < 3 * b || height < 3 * b {
        return None;
    }
    let cols = width.div_ceil(b);
    let rows = height.div_ceil(b);
    // Mesh rows are independent; ordered collection keeps results identical
    // for any worker count.
    let mesh_row = |my: usize| -> Vec<Mesh> {
        let mut samples = Vec::with_capacity(b * b);
        let mut scratch = Vec::with_capacity(b * b);
        (0..cols)
            .map(|mx| {
                samples.clear();
                let (x0, x1) = (mx * b, ((mx + 1) * b).min(width));
                let (y0, y1) = (my * b, ((my + 1) * b).min(height));
                for y in y0..y1 {
                    for x in x0..x1 {
                        let i = y * width + x;
                        if data[i].is_finite() && !excluded.get(i) {
                            samples.push(data[i]);
                        }
                    }
                }
                let area = (x1 - x0) * (y1 - y0);
                let count_raw = samples.len();
                let (median, sigma) = if count_raw > 0 {
                    clipped_stats(
                        &mut samples,
                        rules.clip_kappa,
                        rules.clip_iterations,
                        quantum,
                        &mut scratch,
                    )
                } else {
                    (f64::NAN, f64::NAN)
                };
                let valid_fraction = count_raw as f64 / area as f64;
                Mesh {
                    median,
                    sigma,
                    count: samples.len(),
                    trusted: valid_fraction >= rules.trusted_valid_fraction
                        && samples.len() >= 64
                        && median.is_finite()
                        && sigma.is_finite(),
                }
            })
            .collect()
    };
    let mut meshes: Vec<Mesh> = (0..rows)
        .into_par_iter()
        .map(mesh_row)
        .collect::<Vec<_>>()
        .into_iter()
        .flatten()
        .collect();
    let mut scratch = Vec::with_capacity(b * b);
    // Reject trusted meshes whose level or dispersion is a gross outlier among
    // trusted meshes (5 robust sigma); they are candidates, not reference.
    let center = |i: usize| {
        let mx = i % cols;
        let my = i / cols;
        let xc = ((mx * b) as f64 + (((mx + 1) * b).min(width) - 1) as f64) * 0.5;
        let yc = ((my * b) as f64 + (((my + 1) * b).min(height) - 1) as f64) * 0.5;
        (normalized(xc, width), normalized(yc, height))
    };
    for _ in 0..3 {
        let trusted: Vec<usize> = (0..meshes.len()).filter(|&i| meshes[i].trusted).collect();
        if trusted.len() < rules.min_trusted_meshes {
            return None;
        }
        let (lc, ls) = location_scale_with(trusted.iter().map(|&i| meshes[i].median), &mut scratch);
        let (nc, ns) =
            location_scale_with(trusted.iter().map(|&i| meshes[i].sigma.ln()), &mut scratch);
        let mut changed = false;
        for &i in &trusted {
            let level_out = ls > 0.0
                && (meshes[i].median - lc).abs()
                    > 5.0 * ls.max(meshes[i].sigma / (meshes[i].count as f64).sqrt());
            let noise_out = ns > 0.0 && (meshes[i].sigma.ln() - nc).abs() > 5.0 * ns.max(0.05);
            if level_out || noise_out {
                meshes[i].trusted = false;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let trusted: Vec<usize> = (0..meshes.len()).filter(|&i| meshes[i].trusted).collect();
    if trusted.len() < rules.min_trusted_meshes {
        return None;
    }
    let tcols: Vec<usize> = trusted.iter().map(|&i| i % cols).collect();
    let trows: Vec<usize> = trusted.iter().map(|&i| i / cols).collect();
    let span_x = tcols.iter().max()? - tcols.iter().min()?;
    let span_y = trows.iter().max()? - trows.iter().min()?;
    if span_x < 2 || span_y < 2 {
        return None;
    }
    let points: Vec<(f64, f64)> = trusted.iter().map(|&i| center(i)).collect();
    let values: Vec<f64> = trusted.iter().map(|&i| meshes[i].median).collect();
    // Degree by blocked 5-fold cross-validation over 2x2 mesh blocks; the
    // lowest degree within 10% of the best median absolute CV error wins.
    let folds: Vec<usize> = trusted
        .iter()
        .map(|&i| ((i % cols) / 2 + 3 * ((i / cols) / 2)) % 5)
        .collect();
    let mut cv = Vec::new();
    for degree in 0..=rules.surface_max_degree {
        let p = [1, 3, 6][degree];
        if trusted.len() < 3 * p {
            break;
        }
        let mut errors = Vec::with_capacity(trusted.len());
        let mut ok = true;
        for fold in 0..5 {
            let train: Vec<usize> = (0..trusted.len()).filter(|&k| folds[k] != fold).collect();
            if train.len() < 2 * p {
                ok = false;
                break;
            }
            let tp: Vec<(f64, f64)> = train.iter().map(|&k| points[k]).collect();
            let tv: Vec<f64> = train.iter().map(|&k| values[k]).collect();
            let Some(c) = huber_fit(&tp, &tv, degree) else {
                ok = false;
                break;
            };
            for k in (0..trusted.len()).filter(|&k| folds[k] == fold) {
                errors.push((values[k] - evaluate(&c, degree, points[k].0, points[k].1)).abs());
            }
        }
        if !ok || errors.is_empty() {
            break;
        }
        let (median_error, _) = location_scale_with(errors.iter().copied(), &mut scratch);
        cv.push((degree, median_error));
    }
    let best = cv.iter().map(|(_, e)| *e).fold(f64::INFINITY, f64::min);
    let degree = cv
        .iter()
        .find(|(_, e)| *e <= best * 1.1 + 1e-300)
        .map(|(d, _)| *d)
        .unwrap_or(0);
    let coefficients = huber_fit(&points, &values, degree)?;
    let residuals: Vec<f64> = points
        .iter()
        .zip(&values)
        .map(|(&(u, v), y)| y - evaluate(&coefficients, degree, u, v))
        .collect();
    let (_, residual_scale) = location_scale_with(residuals.iter().copied(), &mut scratch);
    let sampling: Vec<f64> = trusted
        .iter()
        .map(|&i| {
            std::f64::consts::FRAC_PI_2 * meshes[i].sigma.powi(2) / meshes[i].count.max(1) as f64
        })
        .collect();
    let mean_mesh_sampling_var = sampling.iter().sum::<f64>() / sampling.len() as f64;
    let (median_sampling, _) = location_scale_with(sampling.iter().copied(), &mut scratch);
    let structure_sigma = (residual_scale.powi(2) - median_sampling).max(0.0).sqrt();
    let us: Vec<f64> = points.iter().map(|p| p.0).collect();
    let vs: Vec<f64> = points.iter().map(|p| p.1).collect();
    let support = [
        us.iter().copied().fold(f64::INFINITY, f64::min),
        us.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        vs.iter().copied().fold(f64::INFINITY, f64::min),
        vs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    ];
    let noise_grid = fill_noise_grid(&meshes, cols, rows);
    let mut background = Background {
        mesh_size: b,
        cols,
        rows,
        meshes,
        coefficients,
        degree,
        support,
        structure_sigma,
        noise_grid,
        rho: 0.0,
        trusted_count: trusted.len(),
        width,
        height,
        mean_mesh_sampling_var,
    };
    background.rho = pooled_rho(data, excluded, &background);
    Some(background)
}

/// Trusted mesh sigma with 3x3 median smoothing; untrusted cells take the
/// median of the nearest ring of trusted cells.
fn fill_noise_grid(meshes: &[Mesh], cols: usize, rows: usize) -> Vec<f64> {
    let mut grid = vec![f64::NAN; meshes.len()];
    let mut scratch = Vec::new();
    for my in 0..rows {
        for mx in 0..cols {
            let mut values = Vec::with_capacity(9);
            for radius in 0..cols.max(rows) {
                values.clear();
                for ny in my.saturating_sub(radius.max(1))..=(my + radius.max(1)).min(rows - 1) {
                    for nx in mx.saturating_sub(radius.max(1))..=(mx + radius.max(1)).min(cols - 1)
                    {
                        let m = &meshes[ny * cols + nx];
                        if m.trusted {
                            values.push(m.sigma);
                        }
                    }
                }
                if !values.is_empty() {
                    break;
                }
            }
            let (median, _) = location_scale_with(values.iter().copied(), &mut scratch);
            grid[my * cols + mx] = median;
        }
    }
    grid
}

impl Background {
    /// Surface value at a pixel, clamped to the trusted support box.
    pub fn level(&self, x: f64, y: f64) -> f64 {
        let u = normalized(x, self.width).clamp(self.support[0], self.support[1]);
        let v = normalized(y, self.height).clamp(self.support[2], self.support[3]);
        evaluate(&self.coefficients, self.degree, u, v)
    }

    /// Extrapolation distance outside the trusted support, in mesh widths.
    pub fn extrapolation(&self, x: f64, y: f64) -> f64 {
        let u = normalized(x, self.width);
        let v = normalized(y, self.height);
        let du = (self.support[0] - u).max(u - self.support[1]).max(0.0);
        let dv = (self.support[2] - v).max(v - self.support[3]).max(0.0);
        let mesh_u = 2.0 * self.mesh_size as f64 / self.width.max(2) as f64;
        let mesh_v = 2.0 * self.mesh_size as f64 / self.height.max(2) as f64;
        (du / mesh_u).hypot(dv / mesh_v)
    }

    /// Bilinear interpolation of the mesh noise grid at a pixel.
    pub fn noise(&self, x: f64, y: f64) -> f64 {
        let b = self.mesh_size as f64;
        let gx = ((x + 0.5) / b - 0.5).clamp(0.0, (self.cols - 1) as f64);
        let gy = ((y + 0.5) / b - 0.5).clamp(0.0, (self.rows - 1) as f64);
        let (x0, y0) = (gx.floor() as usize, gy.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(self.cols - 1), (y0 + 1).min(self.rows - 1));
        let (fx, fy) = (gx - x0 as f64, gy - y0 as f64);
        let g = |cx: usize, cy: usize| self.noise_grid[cy * self.cols + cx];
        (1.0 - fy) * ((1.0 - fx) * g(x0, y0) + fx * g(x1, y0))
            + fy * ((1.0 - fx) * g(x0, y1) + fx * g(x1, y1))
    }

    /// Variance of the background prediction at a pixel: structure scatter
    /// plus fit uncertainty, inflated by extrapolation leverage.
    pub fn prediction_var(&self, x: f64, y: f64) -> f64 {
        let p = [1.0, 3.0, 6.0][self.degree];
        let fit = (self.structure_sigma.powi(2) + self.mean_mesh_sampling_var) * p
            / self.trusted_count.max(1) as f64;
        let e = self.extrapolation(x, y);
        self.structure_sigma.powi(2) + fit * (1.0 + e * e)
    }

    /// Effective sample size of n correlated samples (AR(1) approximation).
    pub fn effective_n(&self, n: usize) -> f64 {
        (n as f64 * (1.0 - self.rho) / (1.0 + self.rho)).max(1.0)
    }
}

/// Lag-one autocorrelation of mesh-median-subtracted residuals pooled over up
/// to 64 trusted meshes (deterministic stride), excluding masked pixels.
fn pooled_rho(data: &[f64], excluded: &Bitset, bg: &Background) -> f64 {
    let trusted: Vec<usize> = (0..bg.meshes.len())
        .filter(|&i| bg.meshes[i].trusted)
        .collect();
    let stride = trusted.len().div_ceil(64).max(1);
    let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
    let b = bg.mesh_size;
    for &i in trusted.iter().step_by(stride) {
        let m = bg.meshes[i];
        let clip = 5.0 * m.sigma;
        let (mx, my) = (i % bg.cols, i / bg.cols);
        let (x0, x1) = (mx * b, ((mx + 1) * b).min(bg.width));
        let (y0, y1) = (my * b, ((my + 1) * b).min(bg.height));
        let value = |x: usize, y: usize| {
            let k = y * bg.width + x;
            let r = data[k] - m.median;
            (data[k].is_finite() && !excluded.get(k) && r.abs() <= clip).then_some(r)
        };
        for y in y0..y1 {
            for x in x0..x1 {
                let Some(a) = value(x, y) else { continue };
                for (nx, ny) in [(x + 1, y), (x, y + 1)] {
                    if nx < x1
                        && ny < y1
                        && let Some(c) = value(nx, ny)
                    {
                        sxy += a * c;
                        sxx += a * a;
                        syy += c * c;
                    }
                }
            }
        }
    }
    if sxx > 0.0 && syy > 0.0 {
        (sxy / (sxx * syy).sqrt()).clamp(0.0, 0.9)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn huber_plane_recovers_coefficients_despite_outliers() {
        let mut points = Vec::new();
        let mut values = Vec::new();
        for i in 0..7 {
            for j in 0..7 {
                let (u, v) = (i as f64 / 3.0 - 1.0, j as f64 / 3.0 - 1.0);
                points.push((u, v));
                values.push(10.0 + 2.0 * u - 3.0 * v);
            }
        }
        values[10] += 500.0;
        let c = huber_fit(&points, &values, 1).unwrap();
        assert!(
            (c[0] - 10.0).abs() < 0.2 && (c[1] - 2.0).abs() < 0.2 && (c[2] + 3.0).abs() < 0.2,
            "{c:?}"
        );
    }

    #[test]
    fn clipped_stats_handle_quantized_zero_mad_samples() {
        let mut samples = vec![5.0; 90];
        samples.extend([6.0; 10]);
        let (m, s) = clipped_stats(&mut samples, 3.0, 5, 1.0, &mut Vec::new());
        assert_eq!(m, 5.0);
        assert!(s >= 1.0 / 12.0_f64.sqrt());
    }

    #[test]
    fn flat_noise_field_gives_degree_zero_and_small_structure() {
        let (w, h) = (256, 256);
        let mut rng = crate::detection::v2::tests::Rng(1);
        let data: Vec<f64> = (0..w * h).map(|_| 10.0 * rng.gauss()).collect();
        let bg = fit(&data, w, h, &Bitset::new(w * h), 0.0, &V2Rules::default()).unwrap();
        assert!(bg.structure_sigma < 1.0, "{}", bg.structure_sigma);
        assert!(bg.rho < 0.05);
        let noise = bg.noise(100.0, 100.0);
        assert!((noise - 10.0).abs() < 1.0, "noise {noise}");
    }
}
