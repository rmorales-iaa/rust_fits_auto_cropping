//! Probe regressions from plan Revision 2 (A01, A03, A18, A19, A20) plus
//! nulls, circular stops and quantized data. Fixtures are seeded and
//! deterministic; expectations are method checks, not error-rate estimates.
use super::detect_v2;
use crate::config::{DetectorKind, HeuristicRules, HeuristicsConfig};

pub(crate) struct Rng(pub u64);
impl Rng {
    pub fn uniform(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    }
    pub fn gauss(&mut self) -> f64 {
        let (a, b) = (self.uniform(), self.uniform());
        (-2.0 * a.ln()).sqrt() * (2.0 * std::f64::consts::PI * b).cos()
    }
}

pub(crate) fn rules() -> HeuristicRules {
    let text = std::fs::read_to_string("input/heuristics.toml").unwrap();
    let mut rules = toml::from_str::<HeuristicsConfig>(&text)
        .unwrap()
        .heuristics;
    rules.detector = DetectorKind::BackgroundV2Experimental;
    rules
}

pub(crate) fn sky(w: usize, h: usize, sigma: f64, seed: u64) -> Vec<f64> {
    let mut rng = Rng(seed);
    (0..w * h).map(|_| 1000.0 + sigma * rng.gauss()).collect()
}

pub(crate) fn add_star(d: &mut [f64], w: usize, cx: f64, cy: f64, amp: f64, s: f64) {
    let h = d.len() / w;
    for y in 0..h {
        for x in 0..w {
            let r2 = (x as f64 - cx).powi(2) + (y as f64 - cy).powi(2);
            d[y * w + x] += amp * (-r2 / (2.0 * s * s)).exp();
        }
    }
}

/// (true positives, false positives, false negatives) against `truth`.
fn score(mask: &[bool], w: usize, truth: impl Fn(usize, usize) -> bool) -> (usize, usize, usize) {
    let (mut tp, mut fp, mut fneg) = (0, 0, 0);
    for (i, m) in mask.iter().enumerate() {
        let t = truth(i % w, i / w);
        match (*m, t) {
            (true, true) => tp += 1,
            (true, false) => fp += 1,
            (false, true) => fneg += 1,
            _ => {}
        }
    }
    (tp, fp, fneg)
}

const W: usize = 256;

#[test]
fn null_flat_field_is_never_clipped() {
    for seed in 0..8 {
        let d = sky(W, W, 10.0, seed + 100);
        let r = detect_v2(&d, W, W, &rules());
        assert!(!r.accepted, "seed {seed}: {}", r.reason);
    }
}

#[test]
fn a20_bright_edge_source_is_protected() {
    let mut d = sky(W, W, 10.0, 11);
    add_star(&mut d, W, 0.0, 128.0, 3000.0, 14.0);
    let r = detect_v2(&d, W, W, &rules());
    assert!(!r.accepted, "{}", r.reason);
    assert!(r.protected_source_pixels > 0);
}

#[test]
fn a03_smooth_illuminated_vignetting_is_not_removed() {
    for depth in [400.0, 800.0] {
        let mut d = sky(W, W, 10.0, 13);
        for y in 0..W {
            for x in 0..W {
                let e = x.min(y).min(W - 1 - x).min(W - 1 - y) as f64;
                if e < 48.0 {
                    d[y * W + x] -= depth * (1.0 - e / 48.0).powi(2);
                }
            }
        }
        let r = detect_v2(&d, W, W, &rules());
        let removed = r.mask.iter().filter(|m| **m).count();
        assert!(
            !r.accepted || removed == 0,
            "depth {depth}: removed {removed}: {}",
            r.reason
        );
    }
}

#[test]
fn a18_interior_star_does_not_change_the_border_decision() {
    let mut decisions = Vec::new();
    for with_star in [false, true] {
        let mut d = sky(W, W, 10.0, 23);
        let mut rng = Rng(5);
        for y in 0..W {
            for x in 0..48 {
                d[y * W + x] = 700.0 + 5.0 * rng.gauss();
            }
        }
        if with_star {
            add_star(&mut d, W, 128.0, 128.0, 20000.0, 8.0);
        }
        let r = detect_v2(&d, W, W, &rules());
        let (tp, fp, fneg) = score(&r.mask, W, |x, _| x < 48);
        decisions.push((r.accepted, tp, fp, fneg));
    }
    assert_eq!(decisions[0].0, decisions[1].0, "{decisions:?}");
    assert!(decisions[0].0, "{decisions:?}");
    // Native-resolution boundary: tiny absolute error either way.
    for (_, tp, fp, fneg) in decisions {
        assert!(fp <= 2 * W && fneg <= 2 * W, "tp={tp} fp={fp} fn={fneg}");
    }
}

#[test]
fn a19_quiet_and_flat_amplified_dark_strips_are_both_detected() {
    for sigma in [2.0, 60.0] {
        let mut d = sky(W, W, 10.0, 7);
        let mut rng = Rng(99);
        for y in 0..W {
            for x in 0..40 {
                d[y * W + x] = 300.0 + sigma * rng.gauss();
            }
        }
        let r = detect_v2(&d, W, W, &rules());
        let (tp, fp, fneg) = score(&r.mask, W, |x, _| x < 40);
        assert!(r.accepted, "sigma {sigma}: {}", r.reason);
        assert!(
            fneg <= 2 * W && fp <= 2 * W,
            "sigma {sigma}: tp={tp} fp={fp} fn={fneg}"
        );
    }
}

#[test]
fn a19_calibrated_noise_amplified_strip_at_sky_level_is_detected() {
    let mut d = sky(W, W, 10.0, 31);
    let mut rng = Rng(32);
    for y in 0..W {
        for x in 0..40 {
            d[y * W + x] = 1000.0 + 80.0 * rng.gauss();
        }
    }
    let r = detect_v2(&d, W, W, &rules());
    let (tp, fp, fneg) = score(&r.mask, W, |x, _| x < 40);
    assert!(r.accepted, "{}", r.reason);
    assert!(tp >= 36 * W && fp <= 4 * W, "tp={tp} fp={fp} fn={fneg}");
}

#[test]
fn a01_boundary_error_is_independent_of_tile_phase() {
    for width in [20usize, 28, 37] {
        let mut d = sky(W, W, 10.0, 17);
        for y in 0..W {
            for x in 0..width {
                d[y * W + x] = 0.0 + 5.0 * Rng(y as u64 * 1000 + x as u64).gauss();
            }
        }
        let r = detect_v2(&d, W, W, &rules());
        let (tp, fp, fneg) = score(&r.mask, W, |x, _| x < width);
        assert!(r.accepted, "width {width}: {}", r.reason);
        // At most one column of error on each side of a sharp boundary.
        assert!(
            fp <= W && fneg <= W,
            "width {width}: tp={tp} fp={fp} fn={fneg}"
        );
    }
}

#[test]
fn quantized_integer_like_data_with_zero_mad_is_handled() {
    let mut rng = Rng(41);
    let mut d: Vec<f64> = (0..W * W)
        .map(|_| (3.0 + 1.5 * rng.gauss()).round().max(0.0))
        .collect();
    let r = detect_v2(&d, W, W, &rules());
    assert!(!r.accepted, "null quantized: {}", r.reason);
    for y in 0..W {
        for x in 0..32 {
            d[y * W + x] = 0.0;
        }
    }
    let r = detect_v2(&d, W, W, &rules());
    let (_, fp, fneg) = score(&r.mask, W, |x, _| x < 32);
    assert!(r.accepted, "quantized strip: {}", r.reason);
    assert!(fp <= W && fneg <= W, "fp={fp} fn={fneg}");
}

#[test]
fn small_or_unsupported_frames_abstain_explicitly() {
    let d = sky(40, 40, 10.0, 1);
    assert_eq!(
        detect_v2(&d, 40, 40, &rules()).reason_code,
        "insufficient_data"
    );
    let mut d = sky(W, W, 10.0, 2);
    for (i, v) in d.iter_mut().enumerate() {
        if i % W > 40 {
            *v = f64::NAN;
        }
    }
    let r = detect_v2(&d, W, W, &rules());
    assert!(!r.accepted);
}

/// Probe family: (name, data, truth predicate over (x, y)).
type Probe = (&'static str, Vec<f64>, Box<dyn Fn(usize, usize) -> bool>);

fn probes() -> Vec<Probe> {
    let mut out: Vec<Probe> = Vec::new();
    out.push(("null flat", sky(W, W, 10.0, 100), Box::new(|_, _| false)));
    let mut d = sky(W, W, 10.0, 11);
    add_star(&mut d, W, 0.0, 128.0, 3000.0, 14.0);
    out.push(("A20 edge source", d, Box::new(|_, _| false)));
    for depth in [400.0, 800.0] {
        let mut d = sky(W, W, 10.0, 13);
        for y in 0..W {
            for x in 0..W {
                let e = x.min(y).min(W - 1 - x).min(W - 1 - y) as f64;
                if e < 48.0 {
                    d[y * W + x] -= depth * (1.0 - e / 48.0).powi(2);
                }
            }
        }
        out.push((
            if depth < 500.0 {
                "A03 vignetting 400"
            } else {
                "A03 vignetting 800"
            },
            d,
            Box::new(|_, _| false),
        ));
    }
    for with_star in [false, true] {
        let mut d = sky(W, W, 10.0, 23);
        let mut rng = Rng(5);
        for y in 0..W {
            for x in 0..48 {
                d[y * W + x] = 700.0 + 5.0 * rng.gauss();
            }
        }
        if with_star {
            add_star(&mut d, W, 128.0, 128.0, 20000.0, 8.0);
        }
        out.push((
            if with_star {
                "A18 step + star"
            } else {
                "A18 step"
            },
            d,
            Box::new(|x, _| x < 48),
        ));
    }
    for (name, level, sigma) in [
        ("A19 quiet dark", 300.0, 2.0),
        ("A19 amplified dark", 300.0, 60.0),
        ("A19 amplified sky-level", 1000.0, 80.0),
    ] {
        let mut d = sky(W, W, 10.0, 7);
        let mut rng = Rng(99);
        for y in 0..W {
            for x in 0..40 {
                d[y * W + x] = level + sigma * rng.gauss();
            }
        }
        out.push((name, d, Box::new(|x, _| x < 40)));
    }
    for width in [20usize, 28] {
        let mut d = sky(W, W, 10.0, 17);
        let mut rng = Rng(3);
        for y in 0..W {
            for x in 0..width {
                d[y * W + x] = 5.0 * rng.gauss();
            }
        }
        out.push((
            if width == 20 {
                "A01 strip 20"
            } else {
                "A01 strip 28"
            },
            d,
            Box::new(move |x, _| x < width),
        ));
    }
    out
}

#[test]
#[ignore = "manual v1/v2 comparison table and large-frame timing"]
fn v1_v2_comparison() {
    for (name, data, truth) in probes() {
        let mut row = format!("COMPARE {name:<26}");
        for detector in [DetectorKind::TileV1, DetectorKind::BackgroundV2Experimental] {
            let mut cfg = rules();
            cfg.detector = detector;
            let r = crate::detection::detect_border(&data, W, W, &cfg);
            let (tp, fp, fneg) = score(&r.mask, W, &truth);
            row.push_str(&format!(
                " | {:?}: acc={} TP={tp} FP={fp} FN={fneg}",
                detector,
                u8::from(r.accepted)
            ));
        }
        println!("{row}");
    }
    let (w, h) = (7100usize, 5325usize);
    let mut data = sky(w, h, 10.0, 1);
    for y in 0..h {
        for x in 0..300 {
            data[y * w + x] = 50.0 + 3.0 * Rng((y * w + x) as u64).gauss();
        }
    }
    for detector in [DetectorKind::TileV1, DetectorKind::BackgroundV2Experimental] {
        let mut cfg = rules();
        cfg.detector = detector;
        let t = std::time::Instant::now();
        let r = crate::detection::detect_border(&data, w, h, &cfg);
        let (tp, fp, fneg) = score(&r.mask, w, |x, _| x < 300);
        println!(
            "LARGE {detector:?} seconds={:.3} TP={tp} FP={fp} FN={fneg}",
            t.elapsed().as_secs_f64()
        );
    }
}
