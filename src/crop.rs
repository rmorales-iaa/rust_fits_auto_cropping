//! Deterministic largest axis-aligned rectangle that excludes all proposed artifacts.

mod global_search;
use rayon::prelude::*;

const MAX_RESIDUAL_CROP_CANDIDATES: usize = 100_000;

#[derive(Clone, Copy, Debug)]
pub struct CropPolicy {
    pub maximum_residual_artifact_fraction: f64,
    pub maximum_unmasked_sacrifice_fraction: f64,
    pub evidence_ratio: f64,
    pub valid_pixel_loss_cost: f64,
    pub minimum_net_utility: f64,
    /// Optional lambda: sacrificed unflagged pixels <= lambda * flags removed.
    pub exchange_rate: Option<f64>,
    /// True when retained flags will be set undefined in the product
    /// (`crop_masked`), so they count as removed artifacts.
    pub mask_retained_flags: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CropCandidate {
    pub rect: CropRect,
    pub retained_artifact_pixels: usize,
    pub removed_artifact_pixels: usize,
    pub sacrificed_valid_pixels: usize,
    pub residual_artifact_fraction: f64,
    pub sacrifice_fraction: f64,
    pub artifact_benefit: f64,
    pub valid_pixel_loss: f64,
    pub net_utility: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct CropSearchResult {
    /// Best candidate satisfying residual, sacrifice and utility limits.
    pub accepted: Option<CropCandidate>,
    /// Best candidate satisfying only the residual limit, for an abstention report.
    pub proposed: CropCandidate,
    pub candidates_examined: usize,
    pub pixel_step: usize,
    /// Row-grid step of the global search family (1 = exact row sweep).
    pub global_row_step: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CropRect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

/// Choose an expanded rectangle under a residual detector-artifact allowance.
/// The bounded search considers rectangles containing the exact largest clean
/// rectangle as an anchor. It is exact for this expansion family only.
pub fn residual_tolerant_rectangle(
    artifact_mask: &[bool],
    undefined_mask: &[bool],
    width: usize,
    height: usize,
    clean_anchor: CropRect,
    policy: CropPolicy,
) -> Option<CropSearchResult> {
    let total = width.checked_mul(height)?;
    if width == 0
        || height == 0
        || total != artifact_mask.len()
        || total != undefined_mask.len()
        || !policy.maximum_residual_artifact_fraction.is_finite()
        || !(0.0..=1.0).contains(&policy.maximum_residual_artifact_fraction)
        || !policy.maximum_unmasked_sacrifice_fraction.is_finite()
        || !(0.0..=1.0).contains(&policy.maximum_unmasked_sacrifice_fraction)
        || !policy.evidence_ratio.is_finite()
        || policy.evidence_ratio < 0.0
        || !policy.valid_pixel_loss_cost.is_finite()
        || policy.valid_pixel_loss_cost < 0.0
        || !policy.minimum_net_utility.is_finite()
        || policy.minimum_net_utility < 0.0
        || policy
            .exchange_rate
            .is_some_and(|rate| !rate.is_finite() || rate < 0.0)
        || clean_anchor.width == 0
        || clean_anchor.height == 0
        || clean_anchor.x.checked_add(clean_anchor.width)? > width
        || clean_anchor.y.checked_add(clean_anchor.height)? > height
        || (clean_anchor.y..clean_anchor.y + clean_anchor.height).any(|y| {
            (clean_anchor.x..clean_anchor.x + clean_anchor.width)
                .any(|x| undefined_mask[y * width + x])
        })
    {
        return None;
    }

    let ctx = SearchContext::new(artifact_mask, undefined_mask, width, height, policy)?;
    let stride = choose_search_stride(clean_anchor, width, height);
    let left_offsets = expansion_offsets(clean_anchor.x, stride);
    let top_offsets = expansion_offsets(clean_anchor.y, stride);
    let right_offsets = expansion_offsets(width - clean_anchor.x - clean_anchor.width, stride);
    let bottom_offsets = expansion_offsets(height - clean_anchor.y - clean_anchor.height, stride);
    let mut candidates_examined = left_offsets
        .len()
        .saturating_mul(top_offsets.len())
        .saturating_mul(right_offsets.len())
        .saturating_mul(bottom_offsets.len());
    // Each left offset is searched independently; merging the per-offset
    // bests in offset order keeps the sequential first-seen tie rule, so the
    // result is identical to the nested sequential loops.
    let partial: Vec<Best> = left_offsets
        .par_iter()
        .map(|&left| {
            let mut best = Best::default();
            for &top in &top_offsets {
                for &right in &right_offsets {
                    for &bottom in &bottom_offsets {
                        let rect = CropRect {
                            x: clean_anchor.x - left,
                            y: clean_anchor.y - top,
                            width: clean_anchor.width + left + right,
                            height: clean_anchor.height + top + bottom,
                        };
                        if let Some(candidate) = ctx.evaluate(rect) {
                            best.offer(candidate, ctx.accepts(&candidate));
                        }
                    }
                }
            }
            best
        })
        .collect();
    let mut best = Best::default();
    for part in partial {
        best.merge(part);
    }
    // Plan R4.2: also search rectangles that need not contain the anchor and
    // keep the better feasible result, so this can never return a smaller crop
    // than the anchor family alone.
    let global = global_search::search(&ctx, &mut best);
    candidates_examined = candidates_examined.saturating_add(global.candidates_examined);

    Some(CropSearchResult {
        accepted: best.accepted,
        proposed: best.residual?,
        candidates_examined,
        pixel_step: stride,
        global_row_step: global.row_step,
    })
}

/// Best residual-feasible and best fully accepted candidates seen so far.
#[derive(Default)]
pub(crate) struct Best {
    pub residual: Option<CropCandidate>,
    pub accepted: Option<CropCandidate>,
}

impl Best {
    /// Fold a later partial result into this one, as if its candidates had
    /// been offered after ours (ties keep the earlier candidate).
    pub fn merge(&mut self, later: Best) {
        if let Some(candidate) = later.residual {
            self.offer(candidate, false);
        }
        if let Some(candidate) = later.accepted {
            self.offer(candidate, true);
        }
    }

    pub fn offer(&mut self, candidate: CropCandidate, accepted: bool) {
        if self
            .residual
            .is_none_or(|best| candidate_is_better(candidate, best))
        {
            self.residual = Some(candidate);
        }
        if accepted
            && self
                .accepted
                .is_none_or(|best| candidate_is_better(candidate, best))
        {
            self.accepted = Some(candidate);
        }
    }
}

/// Summed-area counts and policy shared by all candidate families.
pub(crate) struct SearchContext {
    pub artifact_prefix: Vec<u32>,
    pub undefined_prefix: Vec<u32>,
    pub width: usize,
    pub height: usize,
    defined_pixels: usize,
    total_artifacts: usize,
    pub policy: CropPolicy,
}

impl SearchContext {
    fn new(
        artifact_mask: &[bool],
        undefined_mask: &[bool],
        width: usize,
        height: usize,
        policy: CropPolicy,
    ) -> Option<Self> {
        let (artifact_prefix, undefined_prefix) =
            build_prefix_counts(artifact_mask, undefined_mask, width, height)?;
        // The full-frame table entries are the exact totals.
        let last = artifact_prefix.len() - 1;
        let defined_pixels = width * height - undefined_prefix[last] as usize;
        let total_artifacts = artifact_prefix[last] as usize;
        Some(Self {
            artifact_prefix,
            undefined_prefix,
            width,
            height,
            defined_pixels,
            total_artifacts,
            policy,
        })
    }

    pub fn flags_in(&self, rect: CropRect) -> usize {
        prefix_rect_sum(&self.artifact_prefix, self.width + 1, rect)
    }

    pub fn undefined_in(&self, rect: CropRect) -> usize {
        prefix_rect_sum(&self.undefined_prefix, self.width + 1, rect)
    }

    /// Candidate metrics, or `None` when the rectangle contains an originally
    /// undefined sample or exceeds the residual flagged fraction.
    pub fn evaluate(&self, rect: CropRect) -> Option<CropCandidate> {
        let area = rect.area();
        if area == 0 || self.undefined_in(rect) != 0 {
            return None;
        }
        let retained_artifact_pixels = self.flags_in(rect);
        let residual_artifact_fraction = retained_artifact_pixels as f64 / area as f64;
        if residual_artifact_fraction > self.policy.maximum_residual_artifact_fraction {
            return None;
        }
        let policy = self.policy;
        let defined_pixels = self.defined_pixels;
        let excluded_artifact_pixels = self.total_artifacts - retained_artifact_pixels;
        // Every sample inside the crop occupies area, including tolerated
        // flags; remove those and excluded flags to get the number of defined
        // unflagged samples sacrificed.
        let sacrificed_valid_pixels = defined_pixels
            .saturating_sub(area)
            .saturating_sub(excluded_artifact_pixels);
        let removed_artifact_pixels = if policy.mask_retained_flags {
            self.total_artifacts
        } else {
            excluded_artifact_pixels
        };
        let sacrifice_denominator = defined_pixels
            .saturating_sub(removed_artifact_pixels)
            .max(1);
        let sacrifice_fraction = sacrificed_valid_pixels as f64 / sacrifice_denominator as f64;
        let valid_loss_fraction = sacrificed_valid_pixels as f64 / defined_pixels.max(1) as f64;
        let artifact_benefit =
            removed_artifact_pixels as f64 / defined_pixels.max(1) as f64 * policy.evidence_ratio;
        let valid_pixel_loss = policy.valid_pixel_loss_cost * valid_loss_fraction
            / (1.0 - valid_loss_fraction).max(f64::EPSILON);
        Some(CropCandidate {
            rect,
            retained_artifact_pixels,
            removed_artifact_pixels,
            sacrificed_valid_pixels,
            residual_artifact_fraction,
            sacrifice_fraction,
            artifact_benefit,
            valid_pixel_loss,
            net_utility: artifact_benefit - valid_pixel_loss,
        })
    }

    /// Sacrifice, utility and optional exchange-rate gates.
    pub fn accepts(&self, candidate: &CropCandidate) -> bool {
        let policy = self.policy;
        candidate.sacrifice_fraction <= policy.maximum_unmasked_sacrifice_fraction
            && candidate.net_utility >= policy.minimum_net_utility
            && policy.exchange_rate.is_none_or(|rate| {
                candidate.sacrificed_valid_pixels as f64
                    <= rate * candidate.removed_artifact_pixels as f64
            })
    }
}

pub(crate) fn candidate_is_better(candidate: CropCandidate, best: CropCandidate) -> bool {
    candidate.rect.area() > best.rect.area()
        || (candidate.rect.area() == best.rect.area()
            && (candidate.retained_artifact_pixels < best.retained_artifact_pixels
                || (candidate.retained_artifact_pixels == best.retained_artifact_pixels
                    && (candidate.net_utility > best.net_utility
                        || (candidate.net_utility == best.net_utility
                            && (candidate.rect.y, candidate.rect.x)
                                < (best.rect.y, best.rect.x))))))
}

fn choose_search_stride(anchor: CropRect, width: usize, height: usize) -> usize {
    let margins = [
        anchor.x,
        anchor.y,
        width - anchor.x - anchor.width,
        height - anchor.y - anchor.height,
    ];
    let mut stride = 1;
    while margins
        .iter()
        .map(|margin| offset_count(*margin, stride))
        .fold(1usize, usize::saturating_mul)
        > MAX_RESIDUAL_CROP_CANDIDATES
    {
        stride += 1;
    }
    stride
}

fn expansion_offsets(maximum: usize, stride: usize) -> Vec<usize> {
    let mut offsets: Vec<usize> = (0..=maximum).step_by(stride).collect();
    if offsets.last().copied() != Some(maximum) {
        offsets.push(maximum);
    }
    offsets
}

fn offset_count(maximum: usize, stride: usize) -> usize {
    maximum / stride + 1 + usize::from(!maximum.is_multiple_of(stride))
}

/// Summed-area tables of (flagged and defined) and undefined samples, with a
/// zero first row and column. Rows are prefixed in parallel, then accumulated
/// down the image; integer arithmetic makes the result identical to a
/// sequential construction.
fn build_prefix_counts(
    artifacts: &[bool],
    undefined: &[bool],
    width: usize,
    height: usize,
) -> Option<(Vec<u32>, Vec<u32>)> {
    let total = width.checked_mul(height)?;
    if total != artifacts.len() || total != undefined.len() || total >= u32::MAX as usize {
        return None;
    }
    let stride = width.checked_add(1)?;
    let prefix_len = stride.checked_mul(height.checked_add(1)?)?;
    let mut artifact_prefix = vec![0u32; prefix_len];
    let mut undefined_prefix = vec![0u32; prefix_len];
    artifact_prefix[stride..]
        .par_chunks_mut(stride)
        .zip(undefined_prefix[stride..].par_chunks_mut(stride))
        .enumerate()
        .for_each(|(y, (artifact_row, undefined_row))| {
            let (mut a, mut u) = (0u32, 0u32);
            for x in 0..width {
                let index = y * width + x;
                a += u32::from(artifacts[index] && !undefined[index]);
                u += u32::from(undefined[index]);
                artifact_row[x + 1] = a;
                undefined_row[x + 1] = u;
            }
        });
    for table in [&mut artifact_prefix, &mut undefined_prefix] {
        for y in 1..height {
            let (above, row) = table.split_at_mut((y + 1) * stride);
            let above = &above[y * stride..];
            for (value, add) in row[..stride].iter_mut().zip(above) {
                *value += *add;
            }
        }
    }
    Some((artifact_prefix, undefined_prefix))
}

fn prefix_rect_sum(prefix: &[u32], stride: usize, rect: CropRect) -> usize {
    let x1 = rect.x + rect.width;
    let y1 = rect.y + rect.height;
    let bottom_right = prefix[y1 * stride + x1] as u64;
    let top_left = prefix[rect.y * stride + rect.x] as u64;
    let top_right = prefix[rect.y * stride + x1] as u64;
    let bottom_left = prefix[y1 * stride + rect.x] as u64;
    (bottom_right + top_left - top_right - bottom_left) as usize
}

impl CropRect {
    pub fn area(self) -> usize {
        self.width * self.height
    }

    pub fn bounds(self) -> CropRect {
        self
    }

    pub fn shape_name(self) -> &'static str {
        "rectangle"
    }

    pub fn retains(self, x: usize, y: usize) -> bool {
        let rect = self.bounds();
        x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height
    }
}

/// Choose the maximum-area axis-aligned rectangle containing only defined,
/// unmasked samples. Optimality is exact for this mask and geometry family;
/// it does not certify that the detector mask matches scientific ground truth.
pub fn largest_clean_region(mask: &[bool], width: usize, height: usize) -> Option<CropRect> {
    largest_unmasked_rectangle(mask, width, height)
}

/// Find the maximum-area rectangle containing no `true` pixels.
/// Ties choose the topmost then leftmost rectangle for reproducible output.
pub fn largest_unmasked_rectangle(mask: &[bool], width: usize, height: usize) -> Option<CropRect> {
    if width == 0 || height == 0 || width.checked_mul(height) != Some(mask.len()) {
        return None;
    }
    let mut heights = vec![0usize; width];
    let mut best: Option<(usize, usize, usize, usize, usize)> = None;
    for y in 0..height {
        for x in 0..width {
            heights[x] = if mask[y * width + x] {
                0
            } else {
                heights[x] + 1
            };
        }
        let mut stack: Vec<(usize, usize)> = Vec::new();
        for x in 0..=width {
            let current = heights.get(x).copied().unwrap_or(0);
            let mut start = x;
            while stack.last().is_some_and(|(_, h)| *h > current) {
                let (left, h) = stack.pop().expect("stack checked above");
                let area = h * (x - left);
                let top = y + 1 - h;
                let candidate = (area, top, left, x, y + 1);
                if best.is_none_or(|old| {
                    candidate.0 > old.0
                        || (candidate.0 == old.0 && (candidate.1, candidate.2) < (old.1, old.2))
                }) {
                    best = Some(candidate);
                }
                start = left;
            }
            if stack.last().is_none_or(|(_, h)| *h < current) {
                stack.push((start, current));
            }
        }
    }
    let (area, y, x, right, bottom) = best?;
    (area > 0).then_some(CropRect {
        x,
        y,
        width: right - x,
        height: bottom - y,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parallel_prefix_counts_equal_a_naive_summed_area_table() {
        let mut state = 9u64;
        for (w, h) in [(1, 1), (7, 3), (64, 65), (130, 9)] {
            let mut bit = || {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                state >> 61 == 0
            };
            let artifacts: Vec<bool> = (0..w * h).map(|_| bit()).collect();
            let undefined: Vec<bool> = (0..w * h).map(|_| bit()).collect();
            let (a, u) = build_prefix_counts(&artifacts, &undefined, w, h).unwrap();
            for y in 0..=h {
                for x in 0..=w {
                    let (mut na, mut nu) = (0u32, 0u32);
                    for yy in 0..y {
                        for xx in 0..x {
                            let i = yy * w + xx;
                            na += u32::from(artifacts[i] && !undefined[i]);
                            nu += u32::from(undefined[i]);
                        }
                    }
                    assert_eq!(
                        (a[y * (w + 1) + x], u[y * (w + 1) + x]),
                        (na, nu),
                        "{w}x{h} at {x},{y}"
                    );
                }
            }
        }
    }

    #[test]
    fn finds_expected_rectangle_around_four_corner_pattern() {
        let (width, height) = (8, 6);
        let mut mask = vec![false; width * height];
        for y in 0..2 {
            for x in 0..2 {
                mask[y * width + x] = true;
                mask[y * width + width - 1 - x] = true;
                mask[(height - 1 - y) * width + x] = true;
                mask[(height - 1 - y) * width + width - 1 - x] = true;
            }
        }
        assert_eq!(
            largest_unmasked_rectangle(&mask, width, height),
            Some(CropRect {
                x: 2,
                y: 0,
                width: 4,
                height: 6
            })
        );
    }

    #[test]
    fn preserves_entire_image_when_there_are_no_artifacts() {
        assert_eq!(
            largest_unmasked_rectangle(&[false; 12], 4, 3),
            Some(CropRect {
                x: 0,
                y: 0,
                width: 4,
                height: 3
            })
        );
    }

    #[test]
    fn rejects_empty_or_mismatched_geometry() {
        assert_eq!(largest_unmasked_rectangle(&[], 0, 0), None);
        assert_eq!(largest_unmasked_rectangle(&[false], 2, 2), None);
    }

    #[test]
    fn maximal_rectangle_matches_exhaustive_small_grid_oracle() {
        let (width, height) = (3, 3);
        for pattern in 0_u16..(1 << (width * height)) {
            let mask: Vec<bool> = (0..width * height)
                .map(|index| pattern & (1 << index) != 0)
                .collect();
            let mut expected = 0;
            for y0 in 0..height {
                for x0 in 0..width {
                    for h in 1..=height - y0 {
                        for w in 1..=width - x0 {
                            let clean =
                                (y0..y0 + h).all(|y| (x0..x0 + w).all(|x| !mask[y * width + x]));
                            if clean {
                                expected = expected.max(w * h);
                            }
                        }
                    }
                }
            }
            let actual = largest_unmasked_rectangle(&mask, width, height)
                .map(CropRect::area)
                .unwrap_or(0);
            assert_eq!(actual, expected, "pattern={pattern:09b}");
        }
    }

    #[test]
    fn production_crop_selector_returns_only_the_maximal_clean_rectangle() {
        let (width, height) = (11, 11);
        let mut mask = vec![true; width * height];
        for y in 0..height {
            for x in 0..width {
                if (x as f64 - 5.0).powi(2) + (y as f64 - 5.0).powi(2) <= 25.0 {
                    mask[y * width + x] = false;
                }
            }
        }
        let selected = largest_clean_region(&mask, width, height).unwrap();
        let rect = selected;
        assert!(rect.area() > 0);
        assert!(
            (rect.y..rect.y + rect.height)
                .all(|y| (rect.x..rect.x + rect.width).all(|x| !mask[y * width + x]))
        );
        assert_eq!(
            rect.area(),
            largest_unmasked_rectangle(&mask, width, height)
                .unwrap()
                .area()
        );
    }

    fn policy(residual: f64, sacrifice: f64, utility: f64) -> CropPolicy {
        CropPolicy {
            maximum_residual_artifact_fraction: residual,
            maximum_unmasked_sacrifice_fraction: sacrifice,
            evidence_ratio: 2.0,
            valid_pixel_loss_cost: 0.0,
            minimum_net_utility: utility,
            exchange_rate: None,
            mask_retained_flags: false,
        }
    }

    #[test]
    fn exchange_rate_rejects_crops_that_sacrifice_too_many_valid_pixels() {
        // One flagged corner pixel; any rectangle excluding it sacrifices
        // at least one valid pixel per flag removed.
        let artifacts = [true, false, false, false];
        let anchor = CropRect {
            x: 1,
            y: 0,
            width: 1,
            height: 2,
        };
        let mut strict = policy(0.0, 1.0, 0.0);
        strict.exchange_rate = Some(0.5);
        let rejected =
            residual_tolerant_rectangle(&artifacts, &[false; 4], 2, 2, anchor, strict).unwrap();
        assert!(rejected.accepted.is_none());
        strict.exchange_rate = Some(1.0);
        let accepted =
            residual_tolerant_rectangle(&artifacts, &[false; 4], 2, 2, anchor, strict).unwrap();
        assert_eq!(accepted.accepted.unwrap().sacrificed_valid_pixels, 1);
    }

    #[test]
    fn masked_retained_flags_count_as_removed_without_changing_sacrifice() {
        let width = 5;
        let mut artifacts = vec![false; 10];
        artifacts[0] = true;
        artifacts[5] = true;
        let anchor = CropRect {
            x: 1,
            y: 0,
            width: 4,
            height: 2,
        };
        let mut masked = policy(0.25, 1.0, 0.0);
        masked.mask_retained_flags = true;
        let result =
            residual_tolerant_rectangle(&artifacts, &[false; 10], width, 2, anchor, masked)
                .unwrap()
                .accepted
                .unwrap();
        assert_eq!(result.rect.area(), 10);
        assert_eq!(result.retained_artifact_pixels, 2);
        assert_eq!(result.removed_artifact_pixels, 2);
        assert_eq!(result.sacrificed_valid_pixels, 0);
    }

    #[test]
    fn residual_allowance_expands_rectangle_and_accounts_for_retained_flags() {
        // Strict clean anchor is columns 1..3. Expanding through either
        // flagged edge yields a larger crop at exactly 25% residual flags.
        let width = 5;
        let height = 2;
        let mut artifacts = vec![false; width * height];
        for y in 0..height {
            artifacts[y * width] = true;
            artifacts[y * width + width - 1] = true;
        }
        let result = residual_tolerant_rectangle(
            &artifacts,
            &[false; 10],
            width,
            height,
            CropRect {
                x: 1,
                y: 0,
                width: 3,
                height: 2,
            },
            policy(0.25, 1.0, 0.0),
        )
        .unwrap();
        let selected = result.accepted.unwrap();
        assert_eq!(selected.rect.area(), 8);
        assert_eq!(selected.retained_artifact_pixels, 2);
        assert_eq!(selected.sacrificed_valid_pixels, 0);
        assert_eq!(selected.residual_artifact_fraction, 0.25);
    }

    #[test]
    fn zero_residual_budget_reproduces_strict_clean_anchor() {
        let artifacts = [true, false, true, true, false, true];
        let anchor = CropRect {
            x: 1,
            y: 0,
            width: 1,
            height: 2,
        };
        let result = residual_tolerant_rectangle(
            &artifacts,
            &[false; 6],
            3,
            2,
            anchor,
            policy(0.0, 1.0, 0.0),
        )
        .unwrap();
        assert_eq!(result.accepted.unwrap().rect, anchor);
    }

    #[test]
    fn undefined_pixels_remain_hard_exclusions() {
        let artifacts = [false; 9];
        let mut undefined = [false; 9];
        undefined[1] = true;
        let anchor = CropRect {
            x: 1,
            y: 1,
            width: 1,
            height: 1,
        };
        let result = residual_tolerant_rectangle(
            &artifacts,
            &undefined,
            3,
            3,
            anchor,
            policy(1.0, 1.0, 0.0),
        )
        .unwrap();
        let selected = result.accepted.unwrap().rect;
        assert!(
            (selected.y..selected.y + selected.height)
                .all(|y| (selected.x..selected.x + selected.width).all(|x| !undefined[y * 3 + x]))
        );
    }

    #[test]
    fn residual_limit_is_inclusive_and_sacrifice_limit_can_reject() {
        let artifacts = [false, true, false, false];
        let anchor = CropRect {
            x: 0,
            y: 0,
            width: 1,
            height: 2,
        };
        let at_limit = residual_tolerant_rectangle(
            &artifacts,
            &[false; 4],
            2,
            2,
            anchor,
            policy(0.5, 1.0, 0.0),
        )
        .unwrap();
        assert_eq!(at_limit.accepted.unwrap().rect.area(), 4);
        let rejected = residual_tolerant_rectangle(
            &artifacts,
            &[false; 4],
            2,
            2,
            anchor,
            policy(0.0, 0.0, 0.0),
        )
        .unwrap();
        assert!(rejected.accepted.is_none());
        assert_eq!(rejected.proposed.rect.area(), 2);
    }

    #[test]
    fn utility_gate_is_applied_to_the_proposed_largest_candidate() {
        let artifacts = [false, true, false, false];
        let anchor = CropRect {
            x: 0,
            y: 0,
            width: 1,
            height: 2,
        };
        let result = residual_tolerant_rectangle(
            &artifacts,
            &[false; 4],
            2,
            2,
            anchor,
            policy(0.5, 1.0, 10.0),
        )
        .unwrap();
        assert!(result.accepted.is_none());
        assert_eq!(result.proposed.rect.area(), 4);
    }

    #[test]
    fn bounded_search_stays_within_its_evaluation_budget() {
        let (width, height) = (600, 600);
        let mut artifacts = vec![true; width * height];
        let anchor = CropRect {
            x: width / 2,
            y: height / 2,
            width: 1,
            height: 1,
        };
        artifacts[anchor.y * width + anchor.x] = false;
        let result = residual_tolerant_rectangle(
            &artifacts,
            &vec![false; width * height],
            width,
            height,
            anchor,
            policy(0.0, 1.0, 0.0),
        )
        .unwrap();
        assert!(result.candidates_examined <= MAX_RESIDUAL_CROP_CANDIDATES);
        assert!(result.pixel_step > 1);
        assert_eq!(result.accepted.unwrap().rect, anchor);
    }

    #[test]
    fn equal_area_candidates_use_residual_utility_and_origin_tiebreaks() {
        let base = CropCandidate {
            rect: CropRect {
                x: 2,
                y: 3,
                width: 5,
                height: 4,
            },
            retained_artifact_pixels: 4,
            removed_artifact_pixels: 10,
            sacrificed_valid_pixels: 2,
            residual_artifact_fraction: 0.2,
            sacrifice_fraction: 0.1,
            artifact_benefit: 1.0,
            valid_pixel_loss: 0.5,
            net_utility: 0.5,
        };
        let fewer_residual = CropCandidate {
            retained_artifact_pixels: 3,
            ..base
        };
        let higher_utility = CropCandidate {
            net_utility: 0.6,
            ..fewer_residual
        };
        let topmost = CropCandidate {
            rect: CropRect {
                x: 4,
                y: 2,
                width: 5,
                height: 4,
            },
            ..higher_utility
        };
        assert!(candidate_is_better(fewer_residual, base));
        assert!(candidate_is_better(higher_utility, fewer_residual));
        assert!(candidate_is_better(topmost, higher_utility));
        assert!(!candidate_is_better(base, fewer_residual));
    }
}
