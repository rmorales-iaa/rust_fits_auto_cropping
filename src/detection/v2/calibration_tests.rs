//! Null false-alarm and injection-recovery harness (plan R1.2/R1.4).
//!
//! Run in release mode, e.g.
//! `FAR_FRAMES=2995 cargo test --release --locked calibration -- --ignored --nocapture`.
//! Each null family is generated with independent seeds; a frame counts as a
//! false alarm when the detector accepts any component. The exact one-sided
//! 95% Clopper-Pearson upper bound is reported. Injection families report the
//! frame detection rate and pixel recall/precision against exact truth with
//! the uncertain band (eta in [0.2, 0.5)) excluded, at the same thresholds.
use super::tests::{Rng, add_star, rules};
use crate::config::DetectorKind;
use crate::detection::detect_border;

/// Frame side in pixels (FAR_SIZE, default 256).
fn size() -> usize {
    std::env::var("FAR_SIZE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(256)
}

fn frames() -> usize {
    std::env::var("FAR_FRAMES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200)
}

/// Pixel noise model sigma^2 = read^2 + level/gain (gain 2, read 5).
fn noisy(level: f64, rng: &mut Rng) -> f64 {
    level + (25.0 + level.max(0.0) / 2.0).sqrt() * rng.gauss()
}

fn null_frame(family: &str, seed: u64) -> Vec<f64> {
    let n_side = size();
    #[allow(non_snake_case)]
    let N = n_side;
    let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15).wrapping_add(1));
    let mut level = vec![1000.0; N * N];
    match family {
        "flat" => {}
        "gradient" => {
            // Linear sky gradient spanning at most +/-40% of the level across
            // the frame, so relative illumination stays >= 0.6 (a true null).
            let scale = 0.4 * 1000.0 / N as f64;
            let (gx, gy) = ((rng.uniform() - 0.5) * scale, (rng.uniform() - 0.5) * scale);
            for y in 0..N {
                for x in 0..N {
                    level[y * N + x] += gx * x as f64 + gy * y as f64;
                }
            }
        }
        "illuminated_vignetting" => {
            let depth = 0.4 * rng.uniform();
            let width = 30.0 + 40.0 * rng.uniform();
            for y in 0..N {
                for x in 0..N {
                    let e = x.min(y).min(N - 1 - x).min(N - 1 - y) as f64;
                    level[y * N + x] *= 1.0 - depth * (1.0 - e / width).max(0.0).powi(2);
                }
            }
        }
        "edge_sources" => {
            for _ in 0..3 {
                let along = rng.uniform() * N as f64;
                let side = (rng.uniform() * 4.0) as usize;
                let (cx, cy) = match side {
                    0 => (0.0, along),
                    1 => (N as f64 - 1.0, along),
                    2 => (along, 0.0),
                    _ => (along, N as f64 - 1.0),
                };
                add_star(
                    &mut level,
                    N,
                    cx,
                    cy,
                    500.0 + 4500.0 * rng.uniform(),
                    2.0 + 13.0 * rng.uniform(),
                );
            }
        }
        "nebulosity" => {
            for _ in 0..4 {
                let (cx, cy) = (rng.uniform() * N as f64, rng.uniform() * N as f64);
                add_star(
                    &mut level,
                    N,
                    cx,
                    cy,
                    50.0 + 250.0 * rng.uniform(),
                    30.0 + 50.0 * rng.uniform(),
                );
            }
        }
        "correlated_noise" => {}
        "quantized_low_counts" => {
            return (0..N * N)
                .map(|_| (3.0 + 3.0_f64.sqrt() * rng.gauss()).round().max(0.0))
                .collect();
        }
        other => panic!("unknown family {other}"),
    }
    let mut data: Vec<f64> = level.iter().map(|&l| noisy(l, &mut rng)).collect();
    if family == "correlated_noise" {
        // 3x3 box filter, rescaled to the original noise level.
        let raw = data.clone();
        for y in 1..N - 1 {
            for x in 1..N - 1 {
                let mut s = 0.0;
                for dy in 0..3 {
                    for dx in 0..3 {
                        s += raw[(y + dy - 1) * N + x + dx - 1] - 1000.0;
                    }
                }
                data[y * N + x] = 1000.0 + s / 3.0;
            }
        }
    }
    data
}

/// Exact one-sided upper Clopper-Pearson bound for k failures in n trials.
fn upper_bound(k: usize, n: usize, alpha: f64) -> f64 {
    if k >= n {
        return 1.0;
    }
    let cdf = |p: f64| -> f64 {
        // P(X <= k | n, p) via log-space terms.
        let mut total = 0.0;
        let mut log_c = 0.0_f64;
        for i in 0..=k {
            if i > 0 {
                log_c += ((n - i + 1) as f64).ln() - (i as f64).ln();
            }
            total += (log_c + i as f64 * p.ln() + (n - i) as f64 * (1.0 - p).ln()).exp();
        }
        total
    };
    let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
    for _ in 0..100 {
        let mid = 0.5 * (lo + hi);
        if cdf(mid) > alpha {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    hi
}

#[test]
fn clopper_pearson_matches_closed_form_for_zero_failures() {
    let n = 2995;
    assert!((upper_bound(0, n, 0.05) - (1.0 - 0.05_f64.powf(1.0 / n as f64))).abs() < 1e-9);
    assert!(upper_bound(0, n, 0.05) <= 1e-3);
}

#[test]
#[ignore = "manual null false-alarm calibration run"]
fn calibration_null_false_alarms() {
    let n = frames();
    for family in [
        "flat",
        "gradient",
        "illuminated_vignetting",
        "edge_sources",
        "nebulosity",
        "correlated_noise",
        "quantized_low_counts",
    ] {
        let mut row = format!("FAR {family:<24} n={n}");
        for detector in [DetectorKind::TileV1, DetectorKind::BackgroundV2Experimental] {
            let mut cfg = rules();
            cfg.detector = detector;
            let side = size();
            let mut false_alarms = 0;
            for seed in 0..n as u64 {
                let r = detect_border(&null_frame(family, seed), side, side, &cfg);
                if r.accepted {
                    false_alarms += 1;
                    if detector == DetectorKind::BackgroundV2Experimental {
                        println!("FALSE_ALARM {family} seed={seed}: {}", r.reason);
                    }
                }
            }
            row.push_str(&format!(
                " | {detector:?}: {false_alarms} false alarms, 95% upper {:.4}",
                upper_bound(false_alarms, n, 0.05)
            ));
        }
        println!("{row}");
    }
}

/// Truth codes: 0 valid, 1 invalid, 2 uncertain (excluded from metrics).
fn injected_frame(family: &str, seed: u64) -> (Vec<f64>, Vec<u8>) {
    #[allow(non_snake_case)]
    let N = size();
    let mut rng = Rng(seed.wrapping_mul(0xd1b5_4a32_d192_ed03).wrapping_add(7));
    let mut eta = vec![1.0_f64; N * N];
    let mut plateau = vec![false; N * N];
    match family {
        "sharp_strip" => {
            let w = 8 + (rng.uniform() * 40.0) as usize;
            for y in 0..N {
                for x in 0..w {
                    plateau[y * N + x] = true;
                }
            }
        }
        "corners" => {
            let c = 10 + (rng.uniform() * 30.0) as usize;
            for y in 0..N {
                for x in 0..N {
                    if (x < c || x >= N - c) && (y < c || y >= N - c) {
                        plateau[y * N + x] = true;
                    }
                }
            }
        }
        "circular_stop_raw" | "circular_stop_calibrated" => {
            let r0 = (0.40 + 0.08 * rng.uniform()) * N as f64;
            let ramp = 4.0 + 12.0 * rng.uniform();
            let c = (N as f64 - 1.0) / 2.0;
            for y in 0..N {
                for x in 0..N {
                    let r = ((x as f64 - c).powi(2) + (y as f64 - c).powi(2)).sqrt();
                    eta[y * N + x] = ((r0 + ramp / 2.0 - r) / ramp).clamp(0.02, 1.0);
                }
            }
        }
        other => panic!("unknown family {other}"),
    }
    let calibrated = family == "circular_stop_calibrated";
    let mut data = Vec::with_capacity(N * N);
    let mut truth = Vec::with_capacity(N * N);
    for i in 0..N * N {
        if plateau[i] {
            data.push(50.0 + 5.0 * rng.gauss());
            truth.push(1);
            continue;
        }
        let raw = noisy(1000.0 * eta[i], &mut rng);
        data.push(if calibrated { raw / eta[i] } else { raw });
        truth.push(if eta[i] >= 0.5 {
            0
        } else if eta[i] < 0.2 {
            1
        } else {
            2
        });
    }
    (data, truth)
}

#[test]
#[ignore = "manual injection-recovery run"]
fn calibration_injection_recovery() {
    let n = (frames() / 4).max(20);
    for family in [
        "sharp_strip",
        "corners",
        "circular_stop_raw",
        "circular_stop_calibrated",
    ] {
        let mut row = format!("POWER {family:<26} n={n}");
        for detector in [DetectorKind::TileV1, DetectorKind::BackgroundV2Experimental] {
            let mut cfg = rules();
            cfg.detector = detector;
            let (mut detected, mut tp, mut fp, mut fneg) = (0usize, 0usize, 0usize, 0usize);
            for seed in 0..n as u64 {
                let (data, truth) = injected_frame(family, seed);
                let r = detect_border(&data, size(), size(), &cfg);
                detected += usize::from(r.accepted);
                if !r.accepted
                    && detector == DetectorKind::BackgroundV2Experimental
                    && std::env::var("FAR_VERBOSE").is_ok()
                {
                    println!(
                        "MISSED {family} seed={seed}: {} {}",
                        r.reason_code, r.reason
                    );
                }
                for (m, t) in r.mask.iter().zip(&truth) {
                    match (*m, *t) {
                        (true, 1) => tp += 1,
                        (true, 0) => fp += 1,
                        (false, 1) => fneg += 1,
                        _ => {}
                    }
                }
            }
            let recall = tp as f64 / (tp + fneg).max(1) as f64;
            let precision = tp as f64 / (tp + fp).max(1) as f64;
            row.push_str(&format!(
                " | {detector:?}: detected {detected}/{n}, recall {recall:.4}, precision {precision:.4}, valid removed {fp}"
            ));
        }
        println!("{row}");
    }
}

#[test]
#[ignore = "debug one null frame"]
fn debug_null_frame() {
    let family = std::env::var("FAR_FAMILY").unwrap_or_else(|_| "illuminated_vignetting".into());
    let seed: u64 = std::env::var("FAR_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(88);
    let mut cfg = rules();
    cfg.detector = DetectorKind::BackgroundV2Experimental;
    let r = detect_border(&null_frame(&family, seed), size(), size(), &cfg);
    println!("DEBUG {}", r.reason);
}
