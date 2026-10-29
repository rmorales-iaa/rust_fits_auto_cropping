//! Deterministic decision-digest and timing harness used to prove that
//! speed-only changes are bit-identical (plan R0). Run with:
//! `cargo test --release --locked equivalence_harness -- --ignored --nocapture`
//! and compare the printed digest with the previous revision.
use crate::config::{DetectorKind, EvidenceModel, HeuristicsConfig, ReferenceModel};
use crate::crop::{CropPolicy, largest_clean_region, residual_tolerant_rectangle};
use crate::detection::detect_border;
use sha2::{Digest, Sha256};

struct Rng(u64);
impl Rng {
    fn uniform(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    }
    fn gauss(&mut self) -> f64 {
        let (a, b) = (self.uniform(), self.uniform());
        (-2.0 * a.ln()).sqrt() * (2.0 * std::f64::consts::PI * b).cos()
    }
}

/// Seeded fixture family covering strips, corners, circles, gradients,
/// sources, noise-amplified borders, NaNs and quantized data.
pub fn fixtures() -> Vec<(String, usize, usize, Vec<f64>)> {
    let mut out = Vec::new();
    for case in 0..48u64 {
        let mut rng = Rng(case * 7919 + 1);
        let (w, h) = (96 + (case as usize % 5) * 37, 96 + (case as usize % 3) * 53);
        let sigma = [10.0, 2.0, 0.0, 30.0][case as usize % 4];
        let mut d: Vec<f64> = (0..w * h).map(|_| 1000.0 + sigma * rng.gauss()).collect();
        let kind = case % 8;
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                let (fx, fy) = (x as f64, y as f64);
                let r = ((fx - w as f64 / 2.0).powi(2) + (fy - h as f64 / 2.0).powi(2)).sqrt();
                let edge = x.min(y).min(w - 1 - x).min(h - 1 - y) as f64;
                match kind {
                    0 if x < 5 + case as usize % 29 => d[i] = 0.0,
                    1 if r > 0.45 * w.min(h) as f64 => d[i] = 200.0 + 3.0 * rng.gauss(),
                    2 if (x < 20 && y < 24) || (x + 20 >= w && y + 30 >= h) => d[i] = 50.0,
                    3 if edge < 40.0 => d[i] -= 500.0 * (1.0 - edge / 40.0).powi(2),
                    4 if x < 30 => d[i] = 300.0 + 60.0 * rng.gauss(),
                    5 => {
                        d[i] +=
                            3000.0 * (-((fx).powi(2) + (fy - h as f64 / 2.0).powi(2)) / 300.0).exp()
                    }
                    6 if y < 12 || (x * 7 + y * 3) % 97 == 0 => d[i] = f64::NAN,
                    7 => d[i] = d[i].round().max(0.0) * if y < 18 { 0.0 } else { 1.0 },
                    _ => {}
                }
            }
        }
        out.push((format!("case{case}-kind{kind}"), w, h, d));
    }
    out
}

pub fn decision_digest(cfg_text: &str) -> (String, f64) {
    decision_digest_with(cfg_text, false)
}

/// `decisions_only` hashes only product decisions (acceptance, masks,
/// component counts, crop rectangles), not diagnostics such as the score,
/// candidate counts or reason text.
pub fn decision_digest_with(cfg_text: &str, decisions_only: bool) -> (String, f64) {
    let base = toml::from_str::<HeuristicsConfig>(cfg_text)
        .unwrap()
        .heuristics;
    let mut hasher = Sha256::new();
    let start = std::time::Instant::now();
    // The three tile_v1 model combinations, then v2, whatever the file selects.
    for (detector, reference, evidence) in [
        (
            DetectorKind::TileV1,
            ReferenceModel::CentralTiles,
            EvidenceModel::SingleScale,
        ),
        (
            DetectorKind::TileV1,
            ReferenceModel::RobustAffine,
            EvidenceModel::SingleScale,
        ),
        (
            DetectorKind::TileV1,
            ReferenceModel::CentralTiles,
            EvidenceModel::MultiscaleConsensus,
        ),
        (
            DetectorKind::BackgroundV2,
            ReferenceModel::CentralTiles,
            EvidenceModel::SingleScale,
        ),
    ] {
        let mut cfg = base.clone();
        cfg.detector = detector;
        cfg.reference_model = reference;
        cfg.evidence_model = evidence;
        for (name, w, h, data) in fixtures() {
            let det = detect_border(&data, w, h, &cfg);
            hasher.update(name.as_bytes());
            hasher.update([u8::from(det.accepted)]);
            if decisions_only {
                for n in [
                    det.accepted_components,
                    det.uncertain_boundary_pixels,
                    det.protected_source_pixels,
                ] {
                    hasher.update(n.to_le_bytes());
                }
                hasher.update(det.evidence_ratio.to_bits().to_le_bytes());
            } else {
                hasher.update(det.reason_code.as_bytes());
                hasher.update(det.reason.as_bytes());
                for v in [
                    det.score,
                    det.evidence_ratio,
                    det.net_utility,
                    det.candidate_fraction,
                ] {
                    hasher.update(v.to_bits().to_le_bytes());
                }
                for n in [
                    det.candidate_mask_pixels,
                    det.candidate_tiles,
                    det.candidate_components,
                    det.accepted_components,
                    det.uncertain_boundary_pixels,
                    det.protected_source_pixels,
                ] {
                    hasher.update(n.to_le_bytes());
                }
            }
            hasher.update(det.mask.iter().map(|m| u8::from(*m)).collect::<Vec<_>>());
            if det.accepted {
                let undefined: Vec<bool> = data.iter().map(|v| !v.is_finite()).collect();
                let excl: Vec<bool> = det
                    .mask
                    .iter()
                    .zip(&undefined)
                    .map(|(a, b)| *a || *b)
                    .collect();
                if let Some(anchor) = largest_clean_region(&excl, w, h) {
                    let r = residual_tolerant_rectangle(
                        &det.mask,
                        &undefined,
                        w,
                        h,
                        anchor,
                        CropPolicy {
                            maximum_residual_artifact_fraction: cfg
                                .maximum_residual_artifact_fraction,
                            maximum_unmasked_sacrifice_fraction: cfg
                                .crop_sacrifice_limit(det.accepted_components),
                            evidence_ratio: det.evidence_ratio,
                            valid_pixel_loss_cost: cfg.valid_pixel_loss_cost,
                            minimum_net_utility: cfg.minimum_net_utility,
                            exchange_rate: None,
                            mask_retained_flags: false,
                        },
                    );
                    if let Some(r) = r {
                        let c = r.accepted.unwrap_or(r.proposed);
                        hasher.update(format!("{:?}{:?}", c.rect, r.accepted.is_some()).as_bytes());
                    }
                }
            }
        }
    }
    (
        format!("{:x}", hasher.finalize()),
        start.elapsed().as_secs_f64(),
    )
}

#[test]
#[ignore = "manual R0 equivalence/timing harness"]
fn equivalence_harness() {
    let text = std::fs::read_to_string("input/heuristics.toml").unwrap();
    let (digest, seconds) = decision_digest(&text);
    println!(
        "EQUIVALENCE digest={digest} fixtures={} seconds={seconds:.3}",
        fixtures().len()
    );
    println!("DECISIONS digest={}", decision_digest_with(&text, true).0);
    let (w, h) = (7100usize, 5325usize);
    let mut rng = Rng(1);
    let mut data: Vec<f64> = (0..w * h).map(|_| 1000.0 + 10.0 * rng.gauss()).collect();
    for y in 0..h {
        for x in 0..300 {
            data[y * w + x] = 0.0;
        }
    }
    let cfg = toml::from_str::<HeuristicsConfig>(&text)
        .unwrap()
        .heuristics;
    for run in 0..3 {
        let t = std::time::Instant::now();
        let det = detect_border(&data, w, h, &cfg);
        println!(
            "LARGE run={run} seconds={:.3} masked={}",
            t.elapsed().as_secs_f64(),
            det.mask.iter().filter(|m| **m).count()
        );
    }
}

#[test]
#[ignore = "manual crop-search timing on a 7100x5325 circular field"]
fn crop_search_timing() {
    let (w, h) = (7100usize, 5325usize);
    let (cx, cy, r) = (3550.0, 2662.0, 2500.0);
    let mut flags = vec![false; w * h];
    for y in 0..h {
        for x in 0..w {
            let d = ((x as f64 - cx).powi(2) + (y as f64 - cy).powi(2)).sqrt();
            flags[y * w + x] = d > r;
        }
    }
    let undefined = vec![false; w * h];
    let anchor = largest_clean_region(&flags, w, h).unwrap();
    for rho in [0.0, 0.05] {
        let t = std::time::Instant::now();
        let result = residual_tolerant_rectangle(
            &flags,
            &undefined,
            w,
            h,
            anchor,
            CropPolicy {
                maximum_residual_artifact_fraction: rho,
                maximum_unmasked_sacrifice_fraction: 1.0,
                evidence_ratio: 5.0,
                valid_pixel_loss_cost: 1.5,
                minimum_net_utility: 0.0,
                exchange_rate: None,
                mask_retained_flags: false,
            },
        )
        .unwrap();
        let c = result.accepted.unwrap_or(result.proposed);
        println!(
            "CROP rho={rho} seconds={:.3} anchor_area={} area={} rect={:?} examined={} row_step={}",
            t.elapsed().as_secs_f64(),
            anchor.area(),
            c.rect.area(),
            c.rect,
            result.candidates_examined,
            result.global_row_step
        );
    }
}
