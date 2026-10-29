//! Product-action selection after detection (plan R4.3/R4.4).
//!
//! Detection proposes artifact pixels; this module chooses the product: keep,
//! dimension-preserving mask, rectangular crop, `crop_masked` (decision D2) or
//! the `bbox_mask` fallback (decision D1). All actions share one accounting so
//! reports compare them on the same terms. Detector flags are not ground truth.
use crate::config::{CropFallback, HeuristicRules, ProcessingMode};
use crate::crop::{
    CropCandidate, CropPolicy, CropRect, CropSearchResult, largest_clean_region,
    residual_tolerant_rectangle,
};
use crate::detection::Detection;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    Keep,
    Mask,
    Crop,
    CropMasked,
    BboxMask,
}

impl ActionKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Keep => "keep",
            Self::Mask => "mask",
            Self::Crop => "crop",
            Self::CropMasked => "crop_masked",
            Self::BboxMask => "bbox_mask",
        }
    }

    /// Whether the product changes geometry and receives a `_cropped` name.
    pub fn has_geometry(self) -> bool {
        matches!(self, Self::Crop | Self::CropMasked | Self::BboxMask)
    }
}

/// The selected action and everything needed to write and report it.
#[derive(Debug)]
pub struct ActionPlan {
    pub kind: ActionKind,
    /// Product rectangle for geometry-changing actions.
    pub rect: Option<CropRect>,
    /// Full-frame samples set undefined in the product (mask/interior flags).
    pub undefined_mask: Vec<bool>,
    /// Full-frame samples absent from the product as defined data: outside
    /// the rectangle or in `undefined_mask`.
    pub exclusion: Vec<bool>,
    pub reason_code: &'static str,
    pub note: String,
    pub search: Option<CropSearchResult>,
    /// Rectangle candidate reported for crop decisions (accepted or proposed).
    pub candidate: Option<CropCandidate>,
    pub crop_rejected: bool,
}

/// Pixel accounting shared by all actions. "Valid" means unflagged defined
/// input, not independently labelled science validity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActionCounts {
    pub artifact_pixels_removed: usize,
    pub unflagged_pixels_sacrificed: usize,
    pub excluded_undefined_pixels: usize,
    pub flagged_pixels_retained: usize,
    pub interior_undefined_pixels: usize,
}

pub fn count(plan: &ActionPlan, detector_mask: &[bool], values: &[f64]) -> ActionCounts {
    count_exclusion(&plan.exclusion, &plan.undefined_mask, detector_mask, values)
}

/// Accounting for an arbitrary exclusion (used for rejected crop proposals).
pub fn count_exclusion(
    exclusion: &[bool],
    undefined_mask: &[bool],
    detector_mask: &[bool],
    values: &[f64],
) -> ActionCounts {
    let mut counts = ActionCounts::default();
    for (((excluded, flagged), value), undefined) in exclusion
        .iter()
        .zip(detector_mask)
        .zip(values)
        .zip(undefined_mask)
    {
        let defined = value.is_finite();
        match (*excluded, *flagged, defined) {
            (true, true, true) => counts.artifact_pixels_removed += 1,
            (true, false, true) => counts.unflagged_pixels_sacrificed += 1,
            (true, _, false) => counts.excluded_undefined_pixels += 1,
            (false, true, true) => counts.flagged_pixels_retained += 1,
            _ => {}
        }
        if *undefined && defined {
            counts.interior_undefined_pixels += 1;
        }
    }
    counts
}

/// Tight bounding box of defined samples that are not detector-flagged.
pub fn retained_bounding_box(
    detector_mask: &[bool],
    values: &[f64],
    width: usize,
    height: usize,
) -> Option<CropRect> {
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0usize, 0usize);
    for y in 0..height {
        for x in 0..width {
            let i = y * width + x;
            if values[i].is_finite() && !detector_mask[i] {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    (x0 != usize::MAX).then(|| CropRect {
        x: x0,
        y: y0,
        width: x1 - x0 + 1,
        height: y1 - y0 + 1,
    })
}

/// Full-frame exclusion: outside `rect` (when given) or in `undefined_mask`.
pub fn exclusion_for(rect: Option<CropRect>, undefined_mask: &[bool], width: usize) -> Vec<bool> {
    undefined_mask
        .iter()
        .enumerate()
        .map(|(i, undefined)| {
            *undefined || rect.is_some_and(|rect| !rect.retains(i % width, i / width))
        })
        .collect()
}

fn keep(len: usize, reason_code: &'static str, note: String) -> ActionPlan {
    ActionPlan {
        kind: ActionKind::Keep,
        rect: None,
        undefined_mask: vec![false; len],
        exclusion: vec![false; len],
        reason_code,
        note,
        search: None,
        candidate: None,
        crop_rejected: false,
    }
}

fn mask(detection: &Detection, reason_code: &'static str, note: String) -> ActionPlan {
    ActionPlan {
        kind: ActionKind::Mask,
        rect: None,
        undefined_mask: detection.mask.clone(),
        exclusion: detection.mask.clone(),
        reason_code,
        note,
        search: None,
        candidate: None,
        crop_rejected: false,
    }
}

/// Inputs to action planning for one decoded image.
pub struct PlanInput<'a> {
    pub mode: ProcessingMode,
    pub rules: &'a HeuristicRules,
    pub detection: &'a Detection,
    pub values: &'a [f64],
    pub width: usize,
    pub height: usize,
    /// Header-derived limitation preventing safe crop WCS (not detector evidence).
    pub crop_wcs_unsupported: bool,
}

pub fn plan(input: PlanInput<'_>) -> ActionPlan {
    let PlanInput {
        mode,
        rules,
        detection,
        values,
        width,
        height,
        crop_wcs_unsupported,
    } = input;
    let len = values.len();
    if !detection.accepted {
        return keep(len, detection.reason_code, String::new());
    }
    if !mode.is_crop() {
        return mask(detection, "mask_applied", String::new());
    }
    if crop_wcs_unsupported {
        return mask(
            detection,
            "crop_wcs_mask_fallback",
            "crop was omitted and the detected border pixels were applied as a dimension-preserving full-frame mask".into(),
        );
    }
    let masked_mode = matches!(mode, ProcessingMode::CropMasked);
    let undefined: Vec<bool> = values.iter().map(|v| !v.is_finite()).collect();
    let crop_exclusions: Vec<bool> = detection
        .mask
        .iter()
        .zip(&undefined)
        .map(|(flagged, undefined)| *flagged || *undefined)
        .collect();
    let search = largest_clean_region(&crop_exclusions, width, height).and_then(|anchor| {
        residual_tolerant_rectangle(
            &detection.mask,
            &undefined,
            width,
            height,
            anchor,
            CropPolicy {
                maximum_residual_artifact_fraction: rules.residual_fraction(mode),
                maximum_unmasked_sacrifice_fraction: rules
                    .crop_sacrifice_limit(detection.accepted_components),
                evidence_ratio: detection.evidence_ratio,
                valid_pixel_loss_cost: rules.valid_pixel_loss_cost,
                minimum_net_utility: rules.minimum_net_utility,
                exchange_rate: rules.crop_exchange_rate,
                mask_retained_flags: masked_mode,
            },
        )
    });
    if let Some(search) = search
        && let Some(accepted) = search.accepted
    {
        let r = accepted.rect;
        if r.x == 0 && r.y == 0 && r.width == width && r.height == height {
            // Not a crop: keep dimensions and remove the accepted detections,
            // so crop provenance is only ever written on cropped products.
            let mut plan = mask(
                detection,
                "crop_full_frame_mask",
                "selected rectangle is the full frame, so accepted detections were applied as a mask"
                    .to_owned(),
            );
            plan.search = Some(search);
            plan.candidate = Some(accepted);
            return plan;
        }
        let undefined_mask: Vec<bool> = if masked_mode {
            detection
                .mask
                .iter()
                .enumerate()
                .map(|(i, flagged)| *flagged && accepted.rect.retains(i % width, i / width))
                .collect()
        } else {
            vec![false; len]
        };
        let exclusion = exclusion_for(Some(accepted.rect), &undefined_mask, width);
        return ActionPlan {
            kind: if masked_mode {
                ActionKind::CropMasked
            } else {
                ActionKind::Crop
            },
            rect: Some(accepted.rect),
            undefined_mask,
            exclusion,
            reason_code: if masked_mode {
                "crop_masked_applied"
            } else {
                "crop_applied"
            },
            note: String::new(),
            search: Some(search),
            candidate: Some(accepted),
            crop_rejected: false,
        };
    }
    // No rectangle passed: report why, then apply the configured fallback.
    let (rejection_code, rejection_note) = match search {
        None => (
            "crop_geometry_unavailable",
            "no positive-area rectangle excludes every detected artifact pixel".to_owned(),
        ),
        Some(search) => {
            let proposed = search.proposed;
            let limit = rules.crop_sacrifice_limit(detection.accepted_components);
            if proposed.sacrifice_fraction > limit {
                (
                    "crop_sacrifice_limit",
                    "crop rejected by the sacrifice limit".to_owned(),
                )
            } else if rules.crop_exchange_rate.is_some_and(|rate| {
                proposed.sacrificed_valid_pixels as f64
                    > rate * proposed.removed_artifact_pixels as f64
            }) {
                (
                    "crop_exchange_rate_rejected",
                    "crop rejected by the exchange-rate limit".to_owned(),
                )
            } else {
                (
                    "crop_utility_rejected",
                    "crop rejected by the utility rule".to_owned(),
                )
            }
        }
    };
    let with_search = |mut plan: ActionPlan| {
        plan.search = search;
        plan.candidate = search.map(|search| search.proposed);
        plan.crop_rejected = true;
        plan
    };
    match rules.crop_fallback {
        CropFallback::Keep => with_search(keep(
            len,
            rejection_code,
            format!("{rejection_note}; source retained unchanged"),
        )),
        CropFallback::Mask => with_search(mask(
            detection,
            "crop_fallback_mask",
            format!("{rejection_note}; accepted detections applied as a dimension-preserving mask"),
        )),
        CropFallback::BboxMask => {
            let Some(rect) = retained_bounding_box(&detection.mask, values, width, height) else {
                return with_search(keep(
                    len,
                    rejection_code,
                    format!(
                        "{rejection_note}; no unflagged defined sample remains; source retained unchanged"
                    ),
                ));
            };
            if rect.x == 0 && rect.y == 0 && rect.width == width && rect.height == height {
                return with_search(mask(
                    detection,
                    "crop_fallback_mask",
                    format!(
                        "{rejection_note}; bounding box of retained samples is the full frame, so accepted detections were applied as a mask"
                    ),
                ));
            }
            let undefined_mask: Vec<bool> = detection
                .mask
                .iter()
                .enumerate()
                .map(|(i, flagged)| *flagged && rect.retains(i % width, i / width))
                .collect();
            let exclusion = exclusion_for(Some(rect), &undefined_mask, width);
            with_search(ActionPlan {
                kind: ActionKind::BboxMask,
                rect: Some(rect),
                undefined_mask,
                exclusion,
                reason_code: "crop_fallback_bbox_mask",
                note: format!(
                    "{rejection_note}; cropped to the bounding box of retained unflagged samples and marked remaining flagged samples undefined"
                ),
                search: None,
                candidate: None,
                crop_rejected: true,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::HeuristicsConfig;

    fn rules(fallback: &str) -> HeuristicRules {
        let text = std::fs::read_to_string("input/heuristics.toml").unwrap();
        let mut rules = toml::from_str::<HeuristicsConfig>(&text)
            .unwrap()
            .heuristics;
        rules.crop_fallback = match fallback {
            "keep" => CropFallback::Keep,
            "mask" => CropFallback::Mask,
            _ => CropFallback::BboxMask,
        };
        rules
    }

    fn detection(mask: Vec<bool>, components: usize) -> Detection {
        Detection {
            candidate_mask_pixels: mask.iter().filter(|m| **m).count(),
            mask,
            candidate_tiles: 1,
            candidate_components: components,
            accepted_components: components,
            candidate_fraction: 0.1,
            accepted: true,
            reason_code: "candidate_accepted",
            reason: String::new(),
            score: 10.0,
            evidence_ratio: 5.0,
            artifact_benefit: 0.0,
            valid_pixel_loss: 0.0,
            net_utility: 1.0,
            uncertain_boundary_pixels: 0,
            protected_source_pixels: 0,
        }
    }

    /// Circular field of radius r in an n x n frame: everything outside is flagged.
    fn circular(n: usize, r: f64) -> (Vec<f64>, Vec<bool>) {
        let c = (n as f64 - 1.0) / 2.0;
        let mut values = vec![100.0; n * n];
        let mut mask = vec![false; n * n];
        for y in 0..n {
            for x in 0..n {
                if ((x as f64 - c).powi(2) + (y as f64 - c).powi(2)).sqrt() > r {
                    mask[y * n + x] = true;
                    values[y * n + x] = 1.0;
                }
            }
        }
        (values, mask)
    }

    fn plan_for(
        mode: ProcessingMode,
        rules: &HeuristicRules,
        det: &Detection,
        values: &[f64],
        n: usize,
    ) -> ActionPlan {
        plan(PlanInput {
            mode,
            rules,
            detection: det,
            values,
            width: n,
            height: n,
            crop_wcs_unsupported: false,
        })
    }

    #[test]
    fn bbox_fallback_removes_every_flag_with_zero_valid_loss() {
        // A small circle forces a rectangle sacrifice far above the caps.
        let n = 64;
        let (values, mask) = circular(n, 20.0);
        let mut strict = rules("bbox_mask");
        strict.maximum_residual_artifact_fraction = 0.0;
        strict.crop_sacrifice_fraction_by_border_component_count = None;
        strict.maximum_crop_sacrifice_fraction = 0.10;
        let det = detection(mask.clone(), 1);
        let plan = plan_for(ProcessingMode::Crop, &strict, &det, &values, n);
        assert_eq!(plan.kind, ActionKind::BboxMask);
        assert_eq!(plan.reason_code, "crop_fallback_bbox_mask");
        let rect = plan.rect.unwrap();
        assert_eq!((rect.width, rect.height), (40, 40));
        let counts = count(&plan, &mask, &values);
        assert_eq!(counts.unflagged_pixels_sacrificed, 0);
        assert_eq!(counts.flagged_pixels_retained, 0);
        assert_eq!(
            counts.artifact_pixels_removed,
            mask.iter().filter(|m| **m).count()
        );
        // Pareto check against the other fallbacks on the same mask.
        let mut keep_rules = strict.clone();
        keep_rules.crop_fallback = CropFallback::Keep;
        let kept_strict = plan_for(ProcessingMode::Crop, &keep_rules, &det, &values, n);
        assert_eq!(kept_strict.kind, ActionKind::Keep);
        assert_eq!(
            count(&kept_strict, &mask, &values).artifact_pixels_removed,
            0
        );
        let mut mask_rules = strict.clone();
        mask_rules.crop_fallback = CropFallback::Mask;
        let masked = plan_for(ProcessingMode::Crop, &mask_rules, &det, &values, n);
        assert_eq!(masked.kind, ActionKind::Mask);
        // bbox_mask and mask remove the same flags and retain the same valid
        // samples; bbox_mask additionally stores a smaller array.
        let masked_counts = count(&masked, &mask, &values);
        assert_eq!(
            masked_counts.artifact_pixels_removed,
            counts.artifact_pixels_removed
        );
        assert_eq!(masked_counts.unflagged_pixels_sacrificed, 0);
        assert_eq!(masked_counts.flagged_pixels_retained, 0);
        assert!(rect.area() < n * n);
    }

    #[test]
    fn full_frame_bounding_box_degrades_to_a_mask() {
        let n = 32;
        let values = vec![100.0; n * n];
        let mut mask = vec![false; n * n];
        // Flags in the middle of each edge leave the bounding box full frame.
        for i in 10..20 {
            mask[i] = true;
            mask[(n - 1) * n + i] = true;
        }
        let mut strict = rules("bbox_mask");
        strict.crop_sacrifice_fraction_by_border_component_count = None;
        strict.maximum_crop_sacrifice_fraction = 0.0;
        strict.maximum_residual_artifact_fraction = 0.0;
        let det = detection(mask, 2);
        let plan = plan_for(ProcessingMode::Crop, &strict, &det, &values, n);
        assert_eq!(plan.kind, ActionKind::Mask);
        assert_eq!(plan.reason_code, "crop_fallback_mask");
    }

    #[test]
    fn crop_masked_marks_retained_flags_and_crop_retains_them() {
        let n = 40;
        let values = vec![100.0; n * n];
        let mut mask = vec![false; n * n];
        for y in 0..n {
            mask[y * n] = true; // one flagged column, 2.5% of a full-width crop
        }
        let mut tolerant = rules("bbox_mask");
        tolerant.maximum_residual_artifact_fraction = 0.05;
        let det = detection(mask.clone(), 1);
        let crop = plan_for(ProcessingMode::Crop, &tolerant, &det, &values, n);
        let crop_masked = plan_for(ProcessingMode::CropMasked, &tolerant, &det, &values, n);
        assert_eq!(crop.kind, ActionKind::Crop);
        // Retaining flags as data removes nothing, so the utility rule keeps
        // the clean 39-column anchor. Masking them makes the full frame the
        // best crop_masked rectangle; a full frame is not a crop, so it is
        // applied as a dimension-preserving mask (no crop provenance).
        assert_eq!(crop.rect.unwrap().area(), 39 * n);
        assert_eq!(crop_masked.kind, ActionKind::Mask);
        assert_eq!(crop_masked.reason_code, "crop_full_frame_mask");
        assert!(crop_masked.rect.is_none());
        let c = count(&crop, &mask, &values);
        let cm = count(&crop_masked, &mask, &values);
        assert_eq!(c.flagged_pixels_retained, 0);
        assert_eq!(cm.flagged_pixels_retained, 0);
        assert_eq!(cm.artifact_pixels_removed, n);
        assert_eq!(cm.unflagged_pixels_sacrificed, 0);
    }

    #[test]
    fn rejected_detection_keeps_and_mask_mode_masks() {
        let n = 16;
        let values = vec![1.0; n * n];
        let mut det = detection(vec![false; n * n], 0);
        det.accepted = false;
        det.reason_code = "no_border_candidate";
        let plan = plan_for(ProcessingMode::Crop, &rules("bbox_mask"), &det, &values, n);
        assert_eq!(
            (plan.kind, plan.reason_code),
            (ActionKind::Keep, "no_border_candidate")
        );
        let mut mask = vec![false; n * n];
        mask[0] = true;
        let det = detection(mask, 1);
        let plan = plan_for(ProcessingMode::Mask, &rules("bbox_mask"), &det, &values, n);
        assert_eq!(plan.kind, ActionKind::Mask);
        assert_eq!(count(&plan, &det.mask, &values).artifact_pixels_removed, 1);
    }

    #[test]
    fn undefined_samples_are_never_counted_as_valid_loss() {
        let n = 8;
        let mut values = vec![5.0; n * n];
        let mut mask = vec![false; n * n];
        for x in 0..n {
            values[x] = f64::NAN;
            mask[n + x] = true;
        }
        let det = detection(mask.clone(), 1);
        let plan = plan_for(ProcessingMode::Crop, &rules("bbox_mask"), &det, &values, n);
        let counts = count(&plan, &mask, &values);
        assert_eq!(counts.excluded_undefined_pixels, n);
        assert_eq!(counts.unflagged_pixels_sacrificed, 0);
        assert_eq!(counts.artifact_pixels_removed, n);
    }
}
