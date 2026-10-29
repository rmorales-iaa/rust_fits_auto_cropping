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
    /// Second-difference (gradient-immune) noise and its effective count.
    pub diff_sigma: f64,
    pub diff_n: f64,
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
    /// Per-mesh statistics before trust selection. They depend only on the
    /// data and the exclusion bits inside the mesh, so a refit reuses them
    /// for meshes whose exclusions did not change.
    raw_meshes: Vec<Mesh>,
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
    /// Interpolation grid of second-difference noise (for dispersion tests).
    pub diff_noise_grid: Vec<f64>,
    /// Median effective second-difference count of trusted meshes.
    pub reference_diff_n: f64,
    /// Stored-value quantum (1 for integer-valued data, else 0).
    pub quantum: f64,
    /// Pooled lag-one residual autocorrelation, clamped to [0, 0.9].
    pub rho: f64,
    pub trusted_count: usize,
    pub width: usize,
    pub height: usize,
    /// Mean sampling variance of a trusted mesh median (for SE_B).
    pub mean_mesh_sampling_var: f64,
}

/// Polynomial extrapolation allowed beyond trusted support (D_max, meshes).
const EXTRAPOLATION_MESHES: f64 = 2.0;

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
/// `previous` is the exclusion set and result of an earlier fit of the same
/// frame with the same rules; meshes whose exclusion bits are unchanged reuse
/// its per-mesh statistics (identical by construction), the rest are recomputed.
pub fn fit(
    data: &[f64],
    width: usize,
    height: usize,
    excluded: &Bitset,
    quantum: f64,
    rules: &V2Rules,
    previous: Option<(&Bitset, &Background)>,
) -> Option<Background> {
    // At least 8x8 meshes on small frames, each still >= 256 samples.
    let b = rules.mesh_size_px.min(width.min(height) / 8).max(16);
    if width < 3 * b || height < 3 * b {
        return None;
    }
    let cols = width.div_ceil(b);
    let rows = height.div_ceil(b);
    // Strided lattice for mesh statistics (plan R6): keep at least
    // `mesh_min_samples` samples per full mesh.
    let step = if rules.mesh_min_samples == 0 {
        1
    } else {
        (((b * b) as f64 / rules.mesh_min_samples as f64)
            .sqrt()
            .floor() as usize)
            .max(1)
    };
    // Mesh rows are independent; ordered collection keeps results identical
    // for any worker count.
    let mesh_row = |my: usize| -> Vec<Mesh> {
        let mut samples = Vec::with_capacity(b * b);
        let mut scratch = Vec::with_capacity(b * b);
        (0..cols)
            .map(|mx| {
                let (x0, x1) = (mx * b, ((mx + 1) * b).min(width));
                let (y0, y1) = (my * b, ((my + 1) * b).min(height));
                if let Some((old_excluded, old)) = previous {
                    let unchanged = (y0..y1).all(|y| {
                        (x0..x1)
                            .all(|x| excluded.get(y * width + x) == old_excluded.get(y * width + x))
                    });
                    if unchanged {
                        return old.raw_meshes[my * cols + mx];
                    }
                }
                samples.clear();
                for y in (y0..y1).step_by(step) {
                    for x in (x0..x1).step_by(step) {
                        let i = y * width + x;
                        if data[i].is_finite() && !excluded.get(i) {
                            samples.push(data[i]);
                        }
                    }
                }
                let area = (x1 - x0).div_ceil(step) * (y1 - y0).div_ceil(step);
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
                let mut diffs = Vec::with_capacity(2 * b * b);
                super::noise::second_differences_strided(
                    data,
                    width,
                    x0,
                    x1,
                    y0,
                    y1,
                    step,
                    |i| excluded.get(i),
                    &mut diffs,
                );
                let (diff_sigma, diff_n) =
                    super::noise::sigma_from_differences(&diffs, quantum, &mut scratch);
                Mesh {
                    median,
                    sigma,
                    diff_sigma,
                    diff_n,
                    count: samples.len(),
                    trusted: valid_fraction >= rules.trusted_valid_fraction
                        && samples.len() >= 64
                        && median.is_finite()
                        && sigma.is_finite()
                        && diff_sigma.is_finite(),
                }
            })
            .collect()
    };
    if let Some((_, old)) = previous {
        assert!(
            old.mesh_size == b && old.raw_meshes.len() == cols * rows && old.quantum == quantum,
            "previous background must come from the same frame and rules"
        );
    }
    let raw_meshes: Vec<Mesh> = (0..rows)
        .into_par_iter()
        .map(mesh_row)
        .collect::<Vec<_>>()
        .into_iter()
        .flatten()
        .collect();
    let mut meshes = raw_meshes.clone();
    let mut scratch = Vec::with_capacity(b * b);
    // Reject trusted meshes whose level is a gross outlier (5 robust sigma), or
    // whose gradient-immune noise implies illumination outside the valid range:
    // |ln(s_diff / median s_diff)| > 0.5 ln(1/eta_valid) (decision D4). Raw
    // noise tracks illumination (Poisson) and flat-divided noise scales
    // inversely, so such meshes are candidates, not reference. A fixed
    // physical tolerance is used because a robust scale of mesh noises is
    // itself contaminated when a large fraction of the frame is affected.
    let center = |i: usize| {
        let mx = i % cols;
        let my = i / cols;
        let xc = ((mx * b) as f64 + (((mx + 1) * b).min(width) - 1) as f64) * 0.5;
        let yc = ((my * b) as f64 + (((my + 1) * b).min(height) - 1) as f64) * 0.5;
        (normalized(xc, width), normalized(yc, height))
    };
    restrict_to_consistent_cluster(&mut meshes, cols, rows, 0.5 * (1.0 / rules.eta_valid).ln());
    for _ in 0..3 {
        let trusted: Vec<usize> = (0..meshes.len()).filter(|&i| meshes[i].trusted).collect();
        if trusted.len() < rules.min_trusted_meshes {
            return None;
        }
        let (lc, ls) = location_scale_with(trusted.iter().map(|&i| meshes[i].median), &mut scratch);
        let (nc, _) = location_scale_with(
            trusted.iter().map(|&i| meshes[i].diff_sigma.ln()),
            &mut scratch,
        );
        let noise_tolerance = 0.5 * (1.0 / rules.eta_valid).ln();
        let mut changed = false;
        for &i in &trusted {
            let level_out = ls > 0.0
                && (meshes[i].median - lc).abs()
                    > 5.0 * ls.max(meshes[i].sigma / (meshes[i].count as f64).sqrt());
            let noise_out = (meshes[i].diff_sigma.ln() - nc).abs() > noise_tolerance;
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
    let noise_grid = fill_noise_grid(&meshes, cols, rows, |m| m.sigma);
    let diff_noise_grid = fill_noise_grid(&meshes, cols, rows, |m| m.diff_sigma);
    let reference_diff_n = {
        let mut counts: Vec<f64> = trusted.iter().map(|&i| meshes[i].diff_n).collect();
        crate::detection::robust::median(&mut counts)
    };
    let mut background = Background {
        mesh_size: b,
        cols,
        rows,
        meshes,
        raw_meshes,
        coefficients,
        degree,
        support,
        structure_sigma,
        noise_grid,
        diff_noise_grid,
        reference_diff_n,
        quantum,
        rho: 0.0,
        trusted_count: trusted.len(),
        width,
        height,
        mean_mesh_sampling_var,
    };
    background.rho = pooled_rho(data, excluded, &background);
    Some(background)
}

/// Keep only the largest 4-connected cluster of trusted meshes whose
/// second-difference noise lies within `tolerance` (in ln sigma) of a common
/// level. Candidate levels are the 21 quantiles of trusted mesh ln-noise;
/// the largest cluster wins (ties: level nearest the median). Real sky support
/// is spatially coherent, while dark corners or field-stop exteriors form
/// separate clusters and partly covered meshes form a continuum, so this is
/// robust even when affected meshes outnumber clean ones. On a uniform frame
/// it keeps every trusted mesh.
fn restrict_to_consistent_cluster(meshes: &mut [Mesh], cols: usize, rows: usize, tolerance: f64) {
    let trusted: Vec<usize> = (0..meshes.len()).filter(|&i| meshes[i].trusted).collect();
    if trusted.is_empty() {
        return;
    }
    let mut levels: Vec<f64> = trusted.iter().map(|&i| meshes[i].diff_sigma.ln()).collect();
    levels.sort_by(f64::total_cmp);
    let median = levels[levels.len() / 2];
    let mut best: (usize, f64, Vec<usize>) = (0, f64::INFINITY, Vec::new());
    let mut seen = vec![false; meshes.len()];
    for q in 0..=20 {
        let level = levels[(q * (levels.len() - 1)) / 20];
        let member =
            |i: usize| meshes[i].trusted && (meshes[i].diff_sigma.ln() - level).abs() <= tolerance;
        seen.fill(false);
        for &start in &trusted {
            if seen[start] || !member(start) {
                continue;
            }
            let mut stack = vec![start];
            seen[start] = true;
            let mut cluster = Vec::new();
            while let Some(i) = stack.pop() {
                cluster.push(i);
                let (x, y) = (i % cols, i / cols);
                for (nx, ny) in [
                    (x.wrapping_sub(1), y),
                    (x + 1, y),
                    (x, y.wrapping_sub(1)),
                    (x, y + 1),
                ] {
                    if nx < cols && ny < rows {
                        let j = ny * cols + nx;
                        if !seen[j] && member(j) {
                            seen[j] = true;
                            stack.push(j);
                        }
                    }
                }
            }
            let distance = (level - median).abs();
            if cluster.len() > best.0 || (cluster.len() == best.0 && distance < best.1) {
                cluster.sort_unstable();
                best = (cluster.len(), distance, cluster);
            }
        }
    }
    let keep: std::collections::HashSet<usize> = best.2.into_iter().collect();
    for (i, mesh) in meshes.iter_mut().enumerate() {
        mesh.trusted &= keep.contains(&i);
    }
}

/// Trusted mesh sigma with 3x3 median smoothing; untrusted cells take the
/// median of the nearest ring of trusted cells.
fn fill_noise_grid(
    meshes: &[Mesh],
    cols: usize,
    rows: usize,
    field: impl Fn(&Mesh) -> f64,
) -> Vec<f64> {
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
                            values.push(field(m));
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

/// Pixel-grid evaluator for `level`, `noise` and `diff_noise` at integer
/// pixel coordinates. Per-column and per-row terms are computed once with the
/// same expressions as the `Background` methods, so every value is
/// bit-identical to the corresponding method call; only repeated work is
/// removed.
pub struct PixelEval<'a> {
    bg: &'a Background,
    u: Vec<f64>,
    v: Vec<f64>,
    gx: Vec<(usize, usize, f64)>,
    gy: Vec<(usize, usize, f64)>,
}

impl PixelEval<'_> {
    #[inline]
    pub fn level(&self, x: usize, y: usize) -> f64 {
        evaluate(&self.bg.coefficients, self.bg.degree, self.u[x], self.v[y])
    }

    #[inline]
    pub fn noise(&self, x: usize, y: usize) -> f64 {
        self.interpolate(&self.bg.noise_grid, x, y)
    }

    #[inline]
    pub fn diff_noise(&self, x: usize, y: usize) -> f64 {
        self.interpolate(&self.bg.diff_noise_grid, x, y)
    }

    #[inline]
    fn interpolate(&self, grid: &[f64], x: usize, y: usize) -> f64 {
        let (x0, x1, fx) = self.gx[x];
        let (y0, y1, fy) = self.gy[y];
        let g = |cx: usize, cy: usize| grid[cy * self.bg.cols + cx];
        (1.0 - fy) * ((1.0 - fx) * g(x0, y0) + fx * g(x1, y0))
            + fy * ((1.0 - fx) * g(x0, y1) + fx * g(x1, y1))
    }
}

impl Background {
    /// Precompute the per-column and per-row terms of the pixel evaluators.
    pub fn pixel_eval(&self) -> PixelEval<'_> {
        let (mesh_u, mesh_v) = self.mesh_extent();
        let du = EXTRAPOLATION_MESHES * mesh_u;
        let dv = EXTRAPOLATION_MESHES * mesh_v;
        let u = (0..self.width)
            .map(|x| {
                normalized(x as f64, self.width).clamp(self.support[0] - du, self.support[1] + du)
            })
            .collect();
        let v = (0..self.height)
            .map(|y| {
                normalized(y as f64, self.height).clamp(self.support[2] - dv, self.support[3] + dv)
            })
            .collect();
        let b = self.mesh_size as f64;
        let axis = |p: usize, cells: usize| {
            let g = ((p as f64 + 0.5) / b - 0.5).clamp(0.0, (cells - 1) as f64);
            let p0 = g.floor() as usize;
            (p0, (p0 + 1).min(cells - 1), g - p0 as f64)
        };
        PixelEval {
            bg: self,
            u,
            v,
            gx: (0..self.width).map(|x| axis(x, self.cols)).collect(),
            gy: (0..self.height).map(|y| axis(y, self.rows)).collect(),
        }
    }

    /// Surface value at a pixel. The polynomial is extrapolated up to
    /// `EXTRAPOLATION_MESHES` mesh widths beyond the trusted support box
    /// (mesh centres stop half a mesh short of the frame edge) and held
    /// constant beyond that, so a distant fit cannot diverge (plan R2.1).
    pub fn level(&self, x: f64, y: f64) -> f64 {
        let (mesh_u, mesh_v) = self.mesh_extent();
        let du = EXTRAPOLATION_MESHES * mesh_u;
        let dv = EXTRAPOLATION_MESHES * mesh_v;
        let u = normalized(x, self.width).clamp(self.support[0] - du, self.support[1] + du);
        let v = normalized(y, self.height).clamp(self.support[2] - dv, self.support[3] + dv);
        evaluate(&self.coefficients, self.degree, u, v)
    }

    /// One mesh width in normalised coordinates along each axis.
    fn mesh_extent(&self) -> (f64, f64) {
        (
            2.0 * self.mesh_size as f64 / self.width.max(2) as f64,
            2.0 * self.mesh_size as f64 / self.height.max(2) as f64,
        )
    }

    /// Extrapolation distance outside the trusted support, in mesh widths.
    pub fn extrapolation(&self, x: f64, y: f64) -> f64 {
        let u = normalized(x, self.width);
        let v = normalized(y, self.height);
        let du = (self.support[0] - u).max(u - self.support[1]).max(0.0);
        let dv = (self.support[2] - v).max(v - self.support[3]).max(0.0);
        let (mesh_u, mesh_v) = self.mesh_extent();
        (du / mesh_u).hypot(dv / mesh_v)
    }

    /// Bilinear interpolation of the second-difference noise grid.
    pub fn diff_noise(&self, x: f64, y: f64) -> f64 {
        self.interpolate(&self.diff_noise_grid, x, y)
    }

    /// Bilinear interpolation of the mesh noise grid at a pixel.
    pub fn noise(&self, x: f64, y: f64) -> f64 {
        self.interpolate(&self.noise_grid, x, y)
    }

    fn interpolate(&self, grid: &[f64], x: f64, y: f64) -> f64 {
        let b = self.mesh_size as f64;
        let gx = ((x + 0.5) / b - 0.5).clamp(0.0, (self.cols - 1) as f64);
        let gy = ((y + 0.5) / b - 0.5).clamp(0.0, (self.rows - 1) as f64);
        let (x0, y0) = (gx.floor() as usize, gy.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(self.cols - 1), (y0 + 1).min(self.rows - 1));
        let (fx, fy) = (gx - x0 as f64, gy - y0 as f64);
        let g = |cx: usize, cy: usize| grid[cy * self.cols + cx];
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
/// Solve a 3x3 system by Cramer's rule; `None` when nearly singular.
fn solve_weighted_3(a: [[f64; 3]; 3], b: [f64; 3]) -> Option<[f64; 3]> {
    let det = |m: [[f64; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let d = det(a);
    let scale = a[0][0] * a[1][1] * a[2][2];
    if !d.is_finite() || d.abs() <= 1e-12 * scale.abs().max(1e-300) {
        return None;
    }
    let mut out = [0.0; 3];
    for (k, slot) in out.iter_mut().enumerate() {
        let mut m = a;
        for r in 0..3 {
            m[r][k] = b[r];
        }
        *slot = det(m) / d;
    }
    Some(out)
}

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
        let raw = |x: usize, y: usize| {
            let k = y * bg.width + x;
            let r = data[k] - m.median;
            (data[k].is_finite() && !excluded.get(k) && r.abs() <= clip).then_some(r)
        };
        // Detrend with a least-squares plane so in-mesh gradients do not
        // masquerade as correlated noise (plan R2.2).
        let (xc, yc) = ((x0 + x1 - 1) as f64 * 0.5, (y0 + y1 - 1) as f64 * 0.5);
        let (mut n, mut sx, mut sy, mut sxx_, mut syy_, mut sxy_) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        let (mut sr, mut sxr, mut syr) = (0.0, 0.0, 0.0);
        for y in y0..y1 {
            for x in x0..x1 {
                if let Some(r) = raw(x, y) {
                    let (u, v) = (x as f64 - xc, y as f64 - yc);
                    n += 1.0;
                    sx += u;
                    sy += v;
                    sxx_ += u * u;
                    syy_ += v * v;
                    sxy_ += u * v;
                    sr += r;
                    sxr += u * r;
                    syr += v * r;
                }
            }
        }
        let plane = solve_weighted_3(
            [[n, sx, sy], [sx, sxx_, sxy_], [sy, sxy_, syy_]],
            [sr, sxr, syr],
        )
        .unwrap_or([sr / n.max(1.0), 0.0, 0.0]);
        let value = |x: usize, y: usize| {
            raw(x, y)
                .map(|r| r - plane[0] - plane[1] * (x as f64 - xc) - plane[2] * (y as f64 - yc))
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
        let bg = fit(
            &data,
            w,
            h,
            &Bitset::new(w * h),
            0.0,
            &V2Rules::default(),
            None,
        )
        .unwrap();
        assert!(bg.structure_sigma < 1.0, "{}", bg.structure_sigma);
        assert!(bg.rho < 0.05);
        let noise = bg.noise(100.0, 100.0);
        assert!((noise - 10.0).abs() < 1.0, "noise {noise}");
    }

    #[test]
    fn pixel_eval_is_bit_identical_to_the_background_methods() {
        // Odd frame size, curved background and an excluded block so the
        // trusted support box is smaller than the frame (clamped extrapolation).
        let (w, h) = (301, 257);
        let mut rng = crate::detection::v2::tests::Rng(7);
        let data: Vec<f64> = (0..w * h)
            .map(|i| {
                let (x, y) = ((i % w) as f64 / w as f64, (i / w) as f64 / h as f64);
                100.0 + 40.0 * x * x - 25.0 * x * y + 30.0 * y + 3.0 * rng.gauss()
            })
            .collect();
        let mut excluded = Bitset::new(w * h);
        for y in 0..h {
            for x in 0..90 {
                excluded.set(y * w + x);
            }
        }
        let bg = fit(&data, w, h, &excluded, 0.0, &V2Rules::default(), None).unwrap();
        assert!(bg.degree >= 1, "degree {}", bg.degree);
        let eval = bg.pixel_eval();
        for y in 0..h {
            for x in 0..w {
                let (fx, fy) = (x as f64, y as f64);
                assert_eq!(
                    eval.level(x, y).to_bits(),
                    bg.level(fx, fy).to_bits(),
                    "level {x},{y}"
                );
                assert_eq!(
                    eval.noise(x, y).to_bits(),
                    bg.noise(fx, fy).to_bits(),
                    "noise {x},{y}"
                );
                assert_eq!(
                    eval.diff_noise(x, y).to_bits(),
                    bg.diff_noise(fx, fy).to_bits(),
                    "diff noise {x},{y}"
                );
            }
        }
    }
}
