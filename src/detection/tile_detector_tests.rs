use super::*;
fn config() -> HeuristicRules {
    HeuristicRules {
        tile_size: 8,
        minimum_border_tiles: 2,
        minimum_abs_z_score: 6.0,
        maximum_tile_noise_ratio: 0.1,
        minimum_spatial_correlation: 0.95,
        minimum_mask_fraction: 0.01,
        maximum_mask_fraction: 0.35,
        minimum_evidence_ratio: 1.25,
        maximum_evidence_ratio: 5.0,
        valid_pixel_loss_cost: 1.5,
        minimum_net_utility: 0.001,
    }
}
#[test]
fn detects_connected_low_corner_and_preserves_center() {
    let (w, h) = (96, 96);
    let mut d = vec![100.0; w * h];
    for y in 0..16 {
        for x in 0..24 {
            d[y * w + x] = 0.0;
        }
    }
    let found = detect_border(&d, w, h, &config());
    assert!(found.accepted);
    assert!(found.mask[0]);
    assert!(!found.mask[48 * w + 48]);
}

#[test]
fn preexisting_undefined_samples_are_not_added_to_the_proposed_mask() {
    let (w, h) = (96, 96);
    let mut data = vec![100.0; w * h];
    for y in 0..16 {
        for x in 0..24 {
            data[y * w + x] = 0.0;
        }
    }
    data[0] = f64::NAN;
    let found = detect_border(&data, w, h, &config());
    assert!(found.accepted, "{}", found.reason);
    assert!(!found.mask[0]);
    assert!(found.mask[1]);
}
#[test]
fn detects_single_sided_pattern_without_forcing_symmetry() {
    let (w, h) = (96, 96);
    let mut data = vec![100.0; w * h];
    for y in 0..16 {
        for x in 0..w {
            data[y * w + x] = 0.0;
        }
    }
    let found = detect_border(&data, w, h, &config());
    assert!(found.accepted);
    assert!(found.mask[0]);
    assert!(!found.mask[48 * w + 48]);
}
#[test]
fn detects_pixelated_circular_border_without_centering_a_crop() {
    let (w, h) = (96, 96);
    let mut data = vec![0.0; w * h];
    let cx = 47.5;
    let cy = 47.5;
    for y in 0..h {
        for x in 0..w {
            if ((x as f64 - cx).powi(2) + (y as f64 - cy).powi(2)).sqrt() < 44.0 {
                data[y * w + x] = 100.0;
            }
        }
    }
    let mut cfg = config();
    cfg.maximum_mask_fraction = 0.50;
    let found = detect_border(&data, w, h, &cfg);
    assert!(
        found.accepted,
        "{} tiles={} score={}",
        found.reason, found.candidate_tiles, found.score
    );
    assert!(found.mask[0]);
    assert!(!found.mask[48 * w + 48]);
}
#[test]
fn rejects_a_connected_region_that_would_mask_too_much() {
    let (w, h) = (96, 96);
    let mut data = vec![100.0; w * h];
    for y in 0..24 {
        for x in 0..w {
            data[y * w + x] = 0.0;
        }
    }
    for y in 72..h {
        for x in 0..w {
            data[y * w + x] = 0.0;
        }
    }
    let mut cfg = config();
    cfg.maximum_mask_fraction = 0.20;
    let found = detect_border(&data, w, h, &cfg);
    assert!(!found.accepted);
    assert_eq!(found.reason, "candidate exceeds maximum mask fraction");
    assert!(found.candidate_fraction > 0.20);
    assert!(found.artifact_benefit > 0.0);
    assert!(found.valid_pixel_loss > 0.0);
    assert_eq!(
        found.net_utility,
        found.artifact_benefit - found.valid_pixel_loss
    );
}
#[test]
fn smooth_sky_gradient_without_border_seam_is_retained() {
    let (w, h) = (96, 96);
    let mut data = vec![0.0; w * h];
    for y in 0..h {
        for x in 0..w {
            data[y * w + x] = 100.0 + x as f64 * 0.25 + y as f64 * 0.25;
        }
    }
    assert!(!detect_border(&data, w, h, &config()).accepted);
}
#[test]
fn leaves_interior_anomaly_untouched() {
    let (w, h) = (96, 96);
    let mut d = vec![100.0; w * h];
    for y in 32..48 {
        for x in 32..48 {
            d[y * w + x] = 0.0;
        }
    }
    assert!(!detect_border(&d, w, h, &config()).accepted);
}

#[test]
fn detects_rectangular_valid_field_with_instrumental_border() {
    let (w, h) = (96, 96);
    let mut data = vec![0.0; w * h];
    for y in 16..80 {
        for x in 12..84 {
            data[y * w + x] = 100.0;
        }
    }
    let mut cfg = config();
    cfg.maximum_mask_fraction = 0.60;
    let found = detect_border(&data, w, h, &cfg);
    assert!(
        found.accepted,
        "{} / {}",
        found.reason, found.candidate_tiles
    );
    assert!(found.mask[0]);
    assert!(!found.mask[48 * w + 48]);
}

#[test]
fn detects_multiple_asymmetric_corner_components() {
    let (w, h) = (96, 96);
    let mut data = vec![100.0; w * h];
    for y in 0..16 {
        for x in 0..24 {
            data[y * w + x] = 0.0;
        }
    }
    for y in 80..h {
        for x in 80..w {
            data[y * w + x] = 0.0;
        }
    }
    let found = detect_border(&data, w, h, &config());
    assert!(found.accepted);
    assert!(found.mask[0]);
    assert!(found.mask[h * w - 1]);
    assert!(!found.mask[48 * w + 48]);
}

#[test]
fn weak_net_utility_keeps_the_image_and_reports_the_tradeoff() {
    let (w, h) = (96, 96);
    let mut data = vec![100.0; w * h];
    for y in 0..24 {
        for x in 0..24 {
            data[y * w + x] = 0.0;
        }
    }
    let mut rules = config();
    rules.valid_pixel_loss_cost = 100.0;
    let found = detect_border(&data, w, h, &rules);
    assert!(!found.accepted);
    assert!(found.candidate_fraction > 0.0);
    assert!(found.valid_pixel_loss > found.artifact_benefit);
    assert!(found.reason.contains("candidate retained"));
}
