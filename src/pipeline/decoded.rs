//! Detection, action selection and product writing for one decoded image.
use super::{
    ProductOptions, create_kept_symlink, elapsed_ms, path_exists, remove_existing_product,
};
use crate::{
    action::{self, ActionKind, PlanInput},
    config::{Config, HeuristicsConfig},
    detection::detect_border,
    fits_image::{Image, NoUnusedIntegerCode},
    model::{FileOutcome, OutcomeKind},
    output_layout::{OutputLayout, ProductCategory},
    preview::write_crop_preview,
    reason_mask,
    writer::{
        write_cropped_masked_product, write_cropped_product, write_masked_product,
        write_reason_mask,
    },
};
use std::{path::Path, time::Instant};

/// Process a decoded image. Returns the outcome; product errors are reported
/// in the outcome rather than propagated.
pub(super) fn process_decoded(
    path: &Path,
    rel: &Path,
    image: Image,
    layout: &OutputLayout,
    config: &Config,
    heuristics: &HeuristicsConfig,
    options: ProductOptions,
) -> FileOutcome {
    let rules = &heuristics.heuristics;
    let detect_timer = Instant::now();
    let detection = detect_border(&image.values, image.width, image.height, rules);
    let detect_ms = elapsed_ms(detect_timer);
    let mut outcome = FileOutcome::new(
        rel.to_path_buf(),
        if detection.accepted {
            OutcomeKind::Clipped
        } else {
            OutcomeKind::Kept
        },
        detection.reason.clone(),
    );
    outcome.reason_code = detection.reason_code;
    outcome.stage_ms.detect = detect_ms;
    outcome.width = Some(image.width);
    outcome.height = Some(image.height);
    outcome.source_sha256 = Some(image.source_sha256.clone());
    outcome.original_undefined_pixels = image.values.iter().filter(|v| !v.is_finite()).count();
    outcome.proposed_mask_fraction = detection.candidate_fraction;
    outcome.candidate_mask_pixels = detection.candidate_mask_pixels;
    outcome.candidate_tiles = detection.candidate_tiles;
    outcome.candidate_components = detection.candidate_components;
    outcome.accepted_components = detection.accepted_components;
    outcome.score = detection.score;
    outcome.evidence_ratio = detection.evidence_ratio;
    outcome.artifact_benefit = detection.artifact_benefit;
    outcome.valid_pixel_loss = detection.valid_pixel_loss;
    outcome.net_utility = detection.net_utility;
    if !image.metadata_warnings.is_empty() {
        outcome.reason.push_str("; ");
        outcome.reason.push_str(&image.metadata_warnings.join("; "));
    }
    if detection.accepted {
        let n = detection.mask.iter().filter(|v| **v).count();
        outcome.proposed_mask_pixels = n;
        // Candidate and accepted masks only include originally defined pixels.
        outcome.candidate_proposed_overlap_pixels = n;
    }

    let geometry_timer = Instant::now();
    let plan = action::plan(PlanInput {
        mode: config.processing.mode,
        rules,
        detection: &detection,
        values: &image.values,
        width: image.width,
        height: image.height,
        crop_wcs_unsupported: image.crop_wcs_unsupported.is_some(),
    });
    outcome.stage_ms.geometry = elapsed_ms(geometry_timer);
    outcome.action = plan.kind.name();
    if detection.accepted {
        outcome.reason_code = plan.reason_code;
    }
    if !plan.note.is_empty() {
        outcome.reason.push_str("; ");
        outcome.reason.push_str(&plan.note);
    }
    if let Some(search) = plan.search {
        outcome.crop_search_candidates = search.candidates_examined;
        outcome.crop_search_step_px = search.pixel_step;
    }
    if let Some(candidate) = plan.candidate {
        outcome.crop_residual_artifact_fraction = candidate.residual_artifact_fraction;
        outcome.artifact_benefit = candidate.artifact_benefit;
        outcome.valid_pixel_loss = candidate.valid_pixel_loss;
        outcome.net_utility = candidate.net_utility;
        let rect = candidate.rect;
        let limit = rules.crop_sacrifice_limit(detection.accepted_components);
        outcome.reason.push_str(&format!(
            "; rectangle candidate origin ({}, {}), bounds {}x{}: retains {} flagged pixels ({:.2}% of crop; limit {:.2}%) and sacrifices {} unflagged pixels ({:.1}% of remaining defined area; limit {:.1}% for {} accepted border-connected area(s)); utility {:.4}; search evaluated {} rectangles at {} px step; {}",
            rect.x, rect.y, rect.width, rect.height,
            candidate.retained_artifact_pixels,
            100.0 * candidate.residual_artifact_fraction,
            100.0 * rules.maximum_residual_artifact_fraction,
            candidate.sacrificed_valid_pixels,
            100.0 * candidate.sacrifice_fraction,
            100.0 * limit,
            detection.accepted_components,
            candidate.net_utility,
            outcome.crop_search_candidates,
            outcome.crop_search_step_px,
            match (plan.crop_rejected, plan.kind) {
                (false, _) => "accepted".to_owned(),
                (true, ActionKind::Keep) => "rejected; source retained".to_owned(),
                (true, kind) => format!("rejected; fallback action {} applied", kind.name()),
            },
        ));
    }
    // A kept image reports its rejected rectangle as a proposal; applied
    // counts stay zero below.
    let proposal_rect = match plan.kind {
        ActionKind::Keep => plan.candidate.map(|candidate| candidate.rect),
        _ => plan.rect,
    };
    if let Some(rect) = proposal_rect {
        outcome.crop_shape = Some(rect.shape_name().to_owned());
        outcome.crop_origin_x = Some(rect.x);
        outcome.crop_origin_y = Some(rect.y);
        outcome.crop_width = Some(rect.width);
        outcome.crop_height = Some(rect.height);
        outcome.image_area_retained = rect.area();
    }
    let counts = if plan.kind == ActionKind::Keep && proposal_rect.is_some() {
        let none = vec![false; image.values.len()];
        let exclusion = action::exclusion_for(proposal_rect, &none, image.width);
        action::count_exclusion(&exclusion, &none, &detection.mask, &image.values)
    } else {
        action::count(&plan, &detection.mask, &image.values)
    };
    outcome.artifact_pixels_removed = counts.artifact_pixels_removed;
    outcome.unmasked_pixels_sacrificed = counts.unflagged_pixels_sacrificed;
    outcome.crop_excluded_undefined_pixels = counts.excluded_undefined_pixels;
    outcome.crop_artifact_pixels_retained = counts.flagged_pixels_retained;
    outcome.crop_interior_masked_pixels = if plan.kind.has_geometry() {
        counts.interior_undefined_pixels
    } else {
        0
    };
    if plan.kind == ActionKind::Keep {
        outcome.status = OutcomeKind::Kept;
        // A kept image has no applied removal; geometry counts are proposals.
    }
    if options.dry_run {
        if outcome.status == OutcomeKind::Clipped {
            outcome.status = OutcomeKind::Proposed;
            outcome.reason_code = "dry_run_proposal";
        }
        return outcome;
    }

    let write_timer = Instant::now();
    let result = write_products(
        path,
        rel,
        &image,
        &detection.mask,
        &plan,
        layout,
        options,
        &mut outcome,
    );
    outcome.stage_ms.write = elapsed_ms(write_timer);
    if result.is_err() {
        return outcome;
    }
    if plan.kind != ActionKind::Keep
        && outcome.output_image.is_some()
        && outcome.status != OutcomeKind::Kept
    {
        outcome.masked_pixels = counts.artifact_pixels_removed + counts.unflagged_pixels_sacrificed;
        outcome.mask_fraction = outcome.masked_pixels as f64
            / (image.width * image.height - outcome.original_undefined_pixels).max(1) as f64;
        outcome.applied_artifact_pixels = counts.artifact_pixels_removed;
        outcome.crop_applied_undefined_pixels = counts.excluded_undefined_pixels;
    }
    outcome
}

#[allow(clippy::too_many_arguments)]
fn write_products(
    path: &Path,
    rel: &Path,
    image: &Image,
    detector_mask: &[bool],
    plan: &action::ActionPlan,
    layout: &OutputLayout,
    options: ProductOptions,
    outcome: &mut FileOutcome,
) -> Result<(), ()> {
    let image_path = match if plan.kind.has_geometry() {
        layout.cropped_image_path(rel)
    } else {
        layout.product_path(ProductCategory::Image, rel)
    } {
        Ok(path) => path,
        Err(error) => return fail(outcome, "output_path_error", error.to_string()),
    };
    let png_path = match (options.png_preview, plan.rect) {
        (true, Some(_)) => match layout.preview_path(rel) {
            Ok(path) => Some(path),
            Err(error) => return fail(outcome, "output_path_error", error.to_string()),
        },
        _ => None,
    };
    let reason_mask_path = if options.reason_mask && plan.kind != ActionKind::Keep {
        match layout.reason_mask_path(rel) {
            Ok(path) => Some(path),
            Err(error) => return fail(outcome, "output_path_error", error.to_string()),
        }
    } else {
        None
    };
    if !options.overwrite
        && (path_exists(&image_path)
            || reason_mask_path.as_ref().is_some_and(|p| path_exists(p))
            || png_path.as_ref().is_some_and(|p| path_exists(p)))
    {
        return fail(
            outcome,
            "output_exists_error",
            "output exists; pass --overwrite".into(),
        );
    }
    let write_result = match (plan.kind, plan.rect) {
        (ActionKind::Keep, _) => create_kept_symlink(path, &image_path),
        (ActionKind::Mask, _) => write_masked_product(&image_path, image, &plan.undefined_mask),
        (ActionKind::Crop, Some(rect)) => write_cropped_product(&image_path, image, rect),
        (ActionKind::CropMasked | ActionKind::BboxMask, Some(rect)) => {
            write_cropped_masked_product(&image_path, image, rect, &plan.undefined_mask)
        }
        (_, None) => Err(anyhow::anyhow!("geometry action has no rectangle")),
    };
    if let Err(error) = write_result {
        if error.downcast_ref::<NoUnusedIntegerCode>().is_some() {
            if options.overwrite {
                for stale in std::iter::once(&image_path)
                    .chain(reason_mask_path.iter())
                    .chain(png_path.iter())
                {
                    if let Err(cleanup) = remove_existing_product(stale) {
                        return fail(
                            outcome,
                            "output_cleanup_error",
                            format!(
                                "{error}; could not remove stale product {}: {cleanup}",
                                stale.display()
                            ),
                        );
                    }
                }
            }
            // Keep the source under its original name.
            let kept_path = match layout.product_path(ProductCategory::Image, rel) {
                Ok(path) => path,
                Err(e) => return fail(outcome, "output_path_error", e.to_string()),
            };
            if let Err(fallback) = create_kept_symlink(path, &kept_path) {
                return fail(
                    outcome,
                    "kept_symlink_error",
                    format!(
                        "{error}; keeping the source was required but symlink publication failed: {fallback}"
                    ),
                );
            }
            outcome.status = OutcomeKind::Kept;
            outcome.action = ActionKind::Keep.name();
            outcome.reason_code = "integer_blank_unavailable_kept";
            outcome.reason = format!(
                "{error}; source retained unchanged because this integer action cannot represent added undefined pixels"
            );
            outcome.output_image = Some(kept_path);
            return Ok(());
        }
        let code = if plan.kind == ActionKind::Keep {
            "kept_symlink_error"
        } else {
            "product_write_error"
        };
        return fail(outcome, code, error.to_string());
    }
    outcome.output_image = Some(image_path);
    if let Some(reason_path) = reason_mask_path {
        let codes = match reason_mask::build(&image.values, detector_mask, &plan.exclusion) {
            Ok(codes) => codes,
            Err(error) => return fail(outcome, "reason_plane_error", error.to_string()),
        };
        if let Err(error) = write_reason_mask(
            &image.source_fits.primary().header,
            &reason_path,
            image.width,
            image.height,
            &codes,
        ) {
            return fail(
                outcome,
                "reason_plane_write_error",
                format!(
                    "science product was written but the optional reason plane failed: {error:#}"
                ),
            );
        }
        outcome.output_reason_mask = Some(reason_path);
    }
    if let (Some(preview), Some(rect)) = (png_path, plan.rect) {
        let interior = plan
            .undefined_mask
            .iter()
            .any(|m| *m)
            .then_some(plan.undefined_mask.as_slice());
        if let Err(error) = write_crop_preview(
            &preview,
            &image.values,
            image.width,
            image.height,
            rect,
            interior,
        ) {
            return fail(
                outcome,
                "preview_write_error",
                format!("FITS product was written, but PNG preview failed: {error:#}"),
            );
        }
        outcome.output_png = Some(preview);
    }
    Ok(())
}

fn fail(outcome: &mut FileOutcome, code: &'static str, reason: String) -> Result<(), ()> {
    outcome.status = OutcomeKind::Error;
    outcome.reason_code = code;
    outcome.reason = reason;
    Err(())
}
