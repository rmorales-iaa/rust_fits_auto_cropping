//! Global residual-tolerant rectangle family (plan R4.2).
//!
//! For each pair of row boundaries `(t, b)` the columns are swept exactly: with
//! per-column flag counts `c_j` and weights `c_j - rho*h`, a rectangle
//! `[l, r] x [t, b)` satisfies the residual limit iff
//! `P[r+1] - P[l] <= 0` for the prefix sums `P`, and contains no originally
//! undefined sample iff `[l, r]` lies inside one undefined-free column run.
//! For every right edge `r` the leftmost feasible `l` is the first index whose
//! running prefix maximum reaches `P[r+1]` (binary search). With a row step of
//! one this family contains a maximum-area rectangle under the residual and
//! undefined constraints, so it is exact for that problem; the sacrifice,
//! utility and exchange gates are then evaluated on every generated candidate.
//! Large frames use a coarser row grid within a fixed work budget followed by a
//! local edge refinement; that case is a bounded heuristic and makes no global
//! optimality claim. Every candidate is re-verified with exact integer counts.
use super::{Best, CropRect, SearchContext};

/// Upper bound on column evaluations (row pairs x columns x log2 columns).
const WORK_BUDGET: usize = 60_000_000;

pub(crate) struct GlobalSummary {
    pub candidates_examined: usize,
    pub row_step: usize,
}

fn row_boundaries(height: usize, step: usize) -> Vec<usize> {
    let mut rows: Vec<usize> = (0..=height).step_by(step).collect();
    if rows.last() != Some(&height) {
        rows.push(height);
    }
    rows
}

fn work(height: usize, width: usize, step: usize) -> usize {
    let n = row_boundaries(height, step).len();
    let pairs = n.saturating_mul(n.saturating_sub(1)) / 2;
    let log = (usize::BITS - width.max(1).leading_zeros()) as usize;
    pairs.saturating_mul(width).saturating_mul(log.max(1))
}

pub(crate) fn search(ctx: &SearchContext, best: &mut Best) -> GlobalSummary {
    let (width, height) = (ctx.width, ctx.height);
    let mut step = 1;
    while step < height && work(height, width, step) > WORK_BUDGET {
        step += 1;
    }
    let rows = row_boundaries(height, step);
    let rho = ctx.policy.maximum_residual_artifact_fraction;
    let mut examined = 0usize;
    let mut prefix = vec![0.0_f64; width + 1];
    let mut running_max: Vec<f64> = Vec::with_capacity(width + 1);
    for (ti, &top) in rows.iter().enumerate() {
        for &bottom in &rows[ti + 1..] {
            let h = bottom - top;
            let mut run_start = 0usize;
            running_max.clear();
            for x in 0..width {
                let column = CropRect {
                    x,
                    y: top,
                    width: 1,
                    height: h,
                };
                if ctx.undefined_in(column) != 0 {
                    // Undefined samples are hard exclusions: restart the run.
                    run_start = x + 1;
                    running_max.clear();
                    continue;
                }
                if running_max.is_empty() {
                    prefix[x] = 0.0;
                    running_max.push(0.0);
                }
                let weight = ctx.flags_in(column) as f64 - rho * h as f64;
                prefix[x + 1] = prefix[x] + weight;
                let target = prefix[x + 1];
                // running_max[k] = max(prefix[run_start..=run_start+k]).
                let first = running_max.partition_point(|&value| value < target);
                if first <= x - run_start {
                    let mut left = run_start + first;
                    // Floating rounding can misplace an equality boundary;
                    // exact counts decide, moving right until feasible.
                    while left <= x {
                        let rect = CropRect {
                            x: left,
                            y: top,
                            width: x - left + 1,
                            height: h,
                        };
                        examined += 1;
                        if let Some(candidate) = ctx.evaluate(rect) {
                            best.offer(candidate, ctx.accepts(&candidate));
                            break;
                        }
                        left += 1;
                    }
                }
                let next = running_max
                    .last()
                    .copied()
                    .unwrap_or(f64::NEG_INFINITY)
                    .max(prefix[x + 1]);
                running_max.push(next);
            }
        }
    }
    if step > 1 {
        examined += refine(ctx, best, step);
    }
    GlobalSummary {
        candidates_examined: examined,
        row_step: step,
    }
}

/// Greedy outward edge refinement of the best candidates at pixel resolution,
/// used only when the row grid is coarser than one pixel.
fn refine(ctx: &SearchContext, best: &mut Best, step: usize) -> usize {
    let mut examined = 0usize;
    for start in [best.accepted, best.residual].into_iter().flatten() {
        let mut rect = start.rect;
        loop {
            let mut improved = None;
            for delta in 1..=step {
                for grown in grow(rect, delta, ctx.width, ctx.height) {
                    examined += 1;
                    if let Some(candidate) = ctx.evaluate(grown) {
                        let accepted = ctx.accepts(&candidate);
                        best.offer(candidate, accepted);
                        if accepted && improved.is_none_or(|r: CropRect| grown.area() > r.area()) {
                            improved = Some(grown);
                        }
                    }
                }
            }
            match improved {
                Some(next) if next.area() > rect.area() => rect = next,
                _ => break,
            }
        }
    }
    examined
}

fn grow(rect: CropRect, delta: usize, width: usize, height: usize) -> Vec<CropRect> {
    let mut out = Vec::with_capacity(4);
    if rect.x >= delta {
        out.push(CropRect {
            x: rect.x - delta,
            width: rect.width + delta,
            ..rect
        });
    }
    if rect.y >= delta {
        out.push(CropRect {
            y: rect.y - delta,
            height: rect.height + delta,
            ..rect
        });
    }
    if rect.x + rect.width + delta <= width {
        out.push(CropRect {
            width: rect.width + delta,
            ..rect
        });
    }
    if rect.y + rect.height + delta <= height {
        out.push(CropRect {
            height: rect.height + delta,
            ..rect
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::{CropPolicy, largest_unmasked_rectangle, residual_tolerant_rectangle};

    fn permissive(rho: f64) -> CropPolicy {
        CropPolicy {
            maximum_residual_artifact_fraction: rho,
            maximum_unmasked_sacrifice_fraction: 1.0,
            evidence_ratio: 1.0,
            valid_pixel_loss_cost: 0.0,
            minimum_net_utility: 0.0,
            exchange_rate: None,
            mask_retained_flags: false,
        }
    }

    /// Brute-force maximum area over all rectangles with no undefined sample
    /// and flagged fraction <= rho.
    fn oracle(flags: &[bool], undefined: &[bool], w: usize, h: usize, rho: f64) -> usize {
        let mut best = 0;
        for y0 in 0..h {
            for x0 in 0..w {
                for y1 in y0 + 1..=h {
                    for x1 in x0 + 1..=w {
                        let (mut f, mut u) = (0usize, 0usize);
                        for y in y0..y1 {
                            for x in x0..x1 {
                                f += usize::from(flags[y * w + x]);
                                u += usize::from(undefined[y * w + x]);
                            }
                        }
                        let area = (x1 - x0) * (y1 - y0);
                        if u == 0 && f as f64 / area as f64 <= rho {
                            best = best.max(area);
                        }
                    }
                }
            }
        }
        best
    }

    #[test]
    fn three_state_grids_match_exhaustive_maximum_area() {
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut checked = 0;
        while checked < 3_000 {
            let w = 1 + (next() % 7) as usize;
            let h = 1 + (next() % 7) as usize;
            let mut flags = vec![false; w * h];
            let mut undefined = vec![false; w * h];
            for i in 0..w * h {
                match next() % 6 {
                    0 | 1 => flags[i] = true,
                    2 => undefined[i] = true,
                    _ => {}
                }
            }
            let exclusions: Vec<bool> = flags
                .iter()
                .zip(&undefined)
                .map(|(f, u)| *f || *u)
                .collect();
            let Some(anchor) = largest_unmasked_rectangle(&exclusions, w, h) else {
                continue;
            };
            let rho = [0.0, 0.1, 0.25, 0.34, 0.5][(next() % 5) as usize];
            let result =
                residual_tolerant_rectangle(&flags, &undefined, w, h, anchor, permissive(rho))
                    .unwrap();
            let chosen = result.accepted.unwrap();
            assert_eq!(result.global_row_step, 1);
            assert_eq!(
                chosen.rect.area(),
                oracle(&flags, &undefined, w, h, rho),
                "w={w} h={h} rho={rho} flags={flags:?} undefined={undefined:?}"
            );
            let r = chosen.rect;
            assert!(
                (r.y..r.y + r.height).all(|y| (r.x..r.x + r.width).all(|x| !undefined[y * w + x]))
            );
            checked += 1;
        }
    }

    #[test]
    fn global_family_finds_a_larger_rectangle_away_from_the_anchor() {
        // Clean 3x6 block on the left, a fully flagged separator column, and
        // a 6x6 block on the right with one flag. The clean anchor ties at 18
        // pixels and resolves to the leftmost block; every expansion of it
        // must cross the separator. The global optimum is the right block.
        let (w, h) = (10, 6);
        let mut flags = vec![false; w * h];
        let undefined = vec![false; w * h];
        for y in 0..h {
            flags[y * w + 3] = true;
        }
        flags[3 * w + 7] = true;
        let anchor = largest_unmasked_rectangle(&flags, w, h).unwrap();
        assert_eq!((anchor.x, anchor.area()), (0, 18));
        let result =
            residual_tolerant_rectangle(&flags, &undefined, w, h, anchor, permissive(0.05))
                .unwrap();
        let rect = result.accepted.unwrap().rect;
        assert_eq!((rect.x, rect.width, rect.height), (4, 6, 6));
    }
}
