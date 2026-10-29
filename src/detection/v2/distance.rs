//! Distances between pixel centres. Production code uses the exact banded
//! signed distance; the full Felzenszwalb & Huttenlocher (Theory of Computing
//! 8, 2012) transform is kept as an independent test oracle.

#[cfg(test)]
const INF: f64 = 1e20;

#[cfg(test)]
fn transform_1d(f: &[f64], out: &mut [f64], v: &mut [usize], z: &mut [f64]) {
    let n = f.len();
    let mut k = 0usize;
    v[0] = 0;
    z[0] = -INF;
    z[1] = INF;
    for q in 1..n {
        loop {
            let p = v[k];
            let s =
                ((f[q] + (q * q) as f64) - (f[p] + (p * p) as f64)) / (2.0 * (q as f64 - p as f64));
            if s <= z[k] && k > 0 {
                k -= 1;
                continue;
            }
            if s <= z[k] {
                // k == 0: replace the only parabola.
                v[0] = q;
                z[0] = -INF;
                z[1] = INF;
                break;
            }
            k += 1;
            v[k] = q;
            z[k] = s;
            z[k + 1] = INF;
            break;
        }
    }
    k = 0;
    for (q, slot) in out.iter_mut().enumerate() {
        while z[k + 1] < q as f64 {
            k += 1;
        }
        let d = q as f64 - v[k] as f64;
        *slot = d * d + f[v[k]];
    }
}

/// Squared distance from each pixel to the nearest `target` pixel
/// (full-frame reference implementation, used as a test oracle).
#[cfg(test)]
pub fn squared_distance(target: &[bool], width: usize, height: usize) -> Vec<f64> {
    let mut grid: Vec<f64> = target.iter().map(|t| if *t { 0.0 } else { INF }).collect();
    let n = width.max(height);
    let (mut f, mut out, mut v, mut z) = (
        vec![0.0; n],
        vec![0.0; n],
        vec![0usize; n],
        vec![0.0; n + 1],
    );
    for x in 0..width {
        for y in 0..height {
            f[y] = grid[y * width + x];
        }
        transform_1d(&f[..height], &mut out[..height], &mut v, &mut z);
        for y in 0..height {
            grid[y * width + x] = out[y];
        }
    }
    for y in 0..height {
        f[..width].copy_from_slice(&grid[y * width..(y + 1) * width]);
        transform_1d(&f[..width], &mut out[..width], &mut v, &mut z);
        grid[y * width..(y + 1) * width].copy_from_slice(&out[..width]);
    }
    grid
}

/// Signed distance to the boundary of `inside`: positive inside (distance to
/// the nearest outside pixel), negative outside (distance to nearest inside).
#[cfg(test)]
pub fn signed_distance(inside: &[bool], width: usize, height: usize) -> Vec<f64> {
    let outside: Vec<bool> = inside.iter().map(|v| !v).collect();
    let to_outside = squared_distance(&outside, width, height);
    let to_inside = squared_distance(inside, width, height);
    inside
        .iter()
        .zip(to_outside.iter().zip(&to_inside))
        .map(|(i, (o, n))| if *i { o.sqrt() } else { -n.sqrt() })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_brute_force_on_random_masks() {
        let mut state = 7u64;
        for _ in 0..200 {
            let (w, h) = (1 + (state % 9) as usize, 1 + (state / 9 % 9) as usize);
            let mask: Vec<bool> = (0..w * h)
                .map(|_| {
                    state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                    state >> 60 == 0
                })
                .collect();
            if !mask.iter().any(|m| *m) {
                continue;
            }
            let got = squared_distance(&mask, w, h);
            for y in 0..h {
                for x in 0..w {
                    let best = (0..w * h)
                        .filter(|&i| mask[i])
                        .map(|i| {
                            let (dx, dy) = ((i % w) as f64 - x as f64, (i / w) as f64 - y as f64);
                            dx * dx + dy * dy
                        })
                        .fold(f64::INFINITY, f64::min);
                    assert_eq!(got[y * w + x], best);
                }
            }
        }
    }
}

/// Exact signed distance truncated to `radius` pixels, as `Banded` storage.
///
/// The nearest outside pixel to an inside pixel always has an inside
/// 4-neighbour (otherwise a 4-step toward the inside pixel would be a closer
/// outside pixel), and symmetrically; so stamping disks of radius `radius`
/// around the two boundary sets gives exact Euclidean distances wherever
/// |d| <= radius. Cost is O(boundary pixels * radius^2), not O(N) passes.
pub struct Banded {
    /// Exact integer squared distance; `FAR` beyond the band (sign from
    /// `inside`). Radii are limited to 255 px so d^2 fits in u16.
    squared: Vec<u16>,
}

const FAR: u16 = u16::MAX;

impl Banded {
    /// Signed distance (positive inside) if within the band.
    pub fn get(&self, index: usize, inside: bool) -> Option<f64> {
        let q = self.squared[index];
        if q == FAR {
            return None;
        }
        let d = f64::from(q).sqrt();
        Some(if inside { d } else { -d })
    }
}

pub fn banded_signed_distance(
    inside: &[bool],
    width: usize,
    height: usize,
    radius: usize,
) -> Banded {
    use rayon::prelude::*;
    let n = width * height;
    let radius = radius.min(255);
    let r = radius as isize;
    let r2 = (radius * radius) as isize;
    let neighbor_differs = |i: usize| {
        let (x, y) = (i % width, i / width);
        let here = inside[i];
        (x > 0 && inside[i - 1] != here)
            || (x + 1 < width && inside[i + 1] != here)
            || (y > 0 && inside[i - width] != here)
            || (y + 1 < height && inside[i + width] != here)
    };
    // Boundary pixels grouped by row.
    let boundary: Vec<Vec<usize>> = (0..height)
        .into_par_iter()
        .map(|y| {
            (y * width..(y + 1) * width)
                .filter(|&i| neighbor_differs(i))
                .collect()
        })
        .collect();
    // Each chunk of output rows takes the minimum over the disks of boundary
    // pixels within `radius` rows. A minimum does not depend on visiting
    // order, so the parallel result equals the serial stamping exactly.
    const CHUNK: usize = 64;
    let mut squared = vec![FAR; n];
    squared
        .par_chunks_mut(CHUNK * width.max(1))
        .enumerate()
        .for_each(|(chunk, rows)| {
            let r0 = chunk * CHUNK;
            let r1 = r0 + rows.len() / width.max(1);
            let b0 = r0.saturating_sub(radius);
            let b1 = (r1 + radius).min(height);
            for row in &boundary[b0..b1] {
                for &b in row {
                    // A boundary pixel of one side is the nearest-candidate
                    // for the other side's pixels.
                    let side = inside[b];
                    let (bx, by) = ((b % width) as isize, (b / width) as isize);
                    let y_lo = (by - r).max(r0 as isize);
                    let y_hi = (by + r).min(r1 as isize - 1);
                    for y in y_lo..=y_hi {
                        let dy = y - by;
                        for dx in -r..=r {
                            let d2 = dx * dx + dy * dy;
                            let x = bx + dx;
                            if d2 > r2 || x < 0 || x >= width as isize {
                                continue;
                            }
                            let j = y as usize * width + x as usize;
                            if inside[j] == side {
                                continue;
                            }
                            let q = d2 as u16;
                            let slot = &mut rows[j - r0 * width];
                            if q < *slot {
                                *slot = q;
                            }
                        }
                    }
                }
            }
        });
    Banded { squared }
}

#[cfg(test)]
mod banded_tests {
    use super::*;

    #[test]
    fn chunked_banded_distance_matches_full_transform_across_chunks() {
        // Several 64-row chunks, blobs straddling chunk edges, radius > 0.
        let (w, h) = (97, 301);
        let mut state = 5u64;
        let mut inside = vec![false; w * h];
        for _ in 0..25 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let (cx, cy) = ((state >> 33) as usize % w, (state >> 13) as usize % h);
            let rad = 3 + (state >> 50) as usize % 9;
            for y in cy.saturating_sub(rad)..(cy + rad).min(h) {
                for x in cx.saturating_sub(rad)..(cx + rad).min(w) {
                    inside[y * w + x] = true;
                }
            }
        }
        let full = signed_distance(&inside, w, h);
        for radius in [1, 7, 30, 70] {
            let banded = banded_signed_distance(&inside, w, h, radius);
            for i in 0..w * h {
                match banded.get(i, inside[i]) {
                    Some(d) => assert_eq!(d, full[i], "radius {radius} pixel {i}"),
                    None => assert!(
                        full[i].abs() > radius as f64,
                        "radius {radius}: missing {}",
                        full[i]
                    ),
                }
            }
        }
    }

    #[test]
    fn banded_distance_matches_full_transform_inside_the_band() {
        let mut state = 11u64;
        for _ in 0..100 {
            let (w, h) = (5 + (state % 20) as usize, 5 + (state / 20 % 20) as usize);
            let inside: Vec<bool> = (0..w * h)
                .map(|_| {
                    state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                    state >> 62 == 0
                })
                .collect();
            if inside.iter().all(|v| *v) || inside.iter().all(|v| !*v) {
                continue;
            }
            let full = signed_distance(&inside, w, h);
            let banded = banded_signed_distance(&inside, w, h, 4);
            for i in 0..w * h {
                match banded.get(i, inside[i]) {
                    Some(d) => assert_eq!(d, full[i]),
                    None => assert!(full[i].abs() > 4.0, "missing {}", full[i]),
                }
            }
        }
    }
}
