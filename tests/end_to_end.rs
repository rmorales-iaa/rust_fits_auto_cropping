use fitskit::{FitsFile, Hdu, HduData, HeaderValue, ImageData, PixelData};
use std::{fs, path::Path, process::Command};
use tempfile::tempdir;

fn create_fits(path: &Path, bitpix: i64, width: usize, height: usize, data: &[f64]) {
    let pixels = match bitpix {
        8 => PixelData::U8(data.iter().map(|v| *v as u8).collect()),
        16 => PixelData::I16(data.iter().map(|v| *v as i16).collect()),
        32 => PixelData::I32(data.iter().map(|v| *v as i32).collect()),
        64 => PixelData::I64(data.iter().map(|v| *v as i64).collect()),
        -32 => PixelData::F32(data.iter().map(|v| *v as f32).collect()),
        -64 => PixelData::F64(data.to_vec()),
        _ => panic!("unsupported test BITPIX"),
    };
    let mut f = FitsFile::with_primary_image(ImageData::new(vec![width, height], pixels));
    let h = &mut f.primary_mut().header;
    h.set("OBJECT", HeaderValue::String("test-field".into()), None);
    h.set("CRPIX1", HeaderValue::Float(width as f64 / 2.0 + 0.5), None);
    h.set(
        "CRPIX2",
        HeaderValue::Float(height as f64 / 2.0 + 0.5),
        None,
    );
    h.set("CTYPE1", HeaderValue::String("RA---TAN".into()), None);
    h.push(fitskit::Keyword::commentary(
        "HISTORY",
        "original-history-entry",
    ));
    f.to_file(path).unwrap();
}

fn config(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join("config.toml"), "[detector]\ntile_size=8\nminimum_border_tiles=2\nminimum_abs_z_score=6.0\nmaximum_tile_noise_ratio=0.1\nminimum_spatial_correlation=0.95\nminimum_mask_fraction=0.01\nmaximum_mask_fraction=0.50\n[processing]\nthreads=2\nrecursive=false\nmemory_budget_mib=1024\nmaximum_image_mib=256\nmode=\"mask\"\n").unwrap();
}

fn invoke(input: &Path, output: &Path, cfg: &Path) -> std::process::ExitStatus {
    Command::new(env!("CARGO_BIN_EXE_fits-auto-clipping"))
        .args(["--input-dir"])
        .arg(input)
        .args(["--output-dir"])
        .arg(output)
        .args(["--config"])
        .arg(cfg)
        .status()
        .unwrap()
}

fn artifact() -> Vec<f64> {
    let mut data = vec![100.0; 96 * 96];
    for y in 0..16 {
        for x in 0..24 {
            data[y * 96 + x] = 0.0;
        }
    }
    data
}

fn read_mask(path: &Path) -> Vec<u8> {
    let f = FitsFile::from_file(path).unwrap();
    match &f.primary().data {
        HduData::Image(img) => match &img.pixels {
            PixelData::U8(v) => v.clone(),
            _ => panic!("expected byte mask"),
        },
        _ => panic!("expected image"),
    }
}

#[test]
fn float_mask_and_reports_preserve_headers_and_retained_pixels() {
    let t = tempdir().unwrap();
    let input = t.path().join("in");
    let output = t.path().join("out");
    fs::create_dir(&input).unwrap();
    config(&input);
    create_fits(&input.join("float.fits"), -64, 96, 96, &artifact());
    create_fits(
        &input.join("clean.fits"),
        -64,
        96,
        96,
        &vec![100.0; 96 * 96],
    );
    assert!(invoke(&input, &output, &input.join("config.toml")).success());
    let f = FitsFile::from_file(output.join("images/float.fits")).unwrap();
    let img = match &f.primary().data {
        HduData::Image(i) => i,
        _ => panic!(),
    };
    assert!(matches!(&img.pixels,PixelData::F64(v) if v[0].is_nan() && v[95*96+95]==100.0));
    assert_eq!(f.primary().header.get_string("OBJECT"), Some("test-field"));
    assert!(f.primary().verify_datasum().is_ok());
    let mask = read_mask(&output.join("masks/float.fits"));
    assert_eq!(mask.len(), 96 * 96);
    assert_eq!(mask[0], 1);
    assert_eq!(mask[95 * 96 + 95], 0);
    let m = FitsFile::from_file(output.join("masks/float.fits")).unwrap();
    assert_eq!(m.primary().header.get_float("CRPIX1"), Some(48.5));
    assert_eq!(m.primary().header.get_string("CTYPE1"), Some("RA---TAN"));
    let report = fs::read_dir(output.join("reports"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path()
        .join("summary.json");
    let v: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(v["clipped"], 1);
    assert_eq!(v["kept"], 1);
    assert_eq!(v["total"], 2);
    assert!(output.join("images/clean.fits").is_file());
    assert!(!output.join("masks/clean.fits").exists());
    assert!(output.join("logger").read_dir().unwrap().next().is_some());
}

#[test]
fn int64_blank_and_retained_samples_are_preserved() {
    let t = tempdir().unwrap();
    let input = t.path().join("in");
    let output = t.path().join("out");
    fs::create_dir(&input).unwrap();
    config(&input);
    let base = 1_i64 << 60;
    // Construct exact int64 values; this avoids the f64 fixture helper's precision limit.
    let mut raw = vec![base + 100; 96 * 96];
    for y in 0..16 {
        for x in 0..24 {
            raw[y * 96 + x] = base;
        }
    }
    let f = FitsFile::with_primary_image(ImageData::new(vec![96, 96], PixelData::I64(raw)));
    f.to_file(input.join("large-offset.fits")).unwrap();
    assert!(invoke(&input, &output, &input.join("config.toml")).success());
    let out = FitsFile::from_file(output.join("images/large-offset.fits")).unwrap();
    let blank = out.primary().header.get_int("BLANK").unwrap();
    let img = match &out.primary().data {
        HduData::Image(i) => i,
        _ => panic!(),
    };
    assert!(matches!(&img.pixels,PixelData::I64(v) if v[0]==blank && v[95*96+95]==base+100));
}

#[test]
fn scaled_integer_header_and_pixels_are_preserved() {
    let t = tempdir().unwrap();
    let input = t.path().join("in");
    let output = t.path().join("out");
    fs::create_dir(&input).unwrap();
    config(&input);
    create_fits(&input.join("scaled.fits"), 16, 96, 96, &artifact());
    let mut f = FitsFile::from_file(input.join("scaled.fits")).unwrap();
    f.primary_mut()
        .header
        .set("BSCALE", HeaderValue::Float(2.0), None);
    f.primary_mut()
        .header
        .set("BZERO", HeaderValue::Float(100.0), None);
    f.to_file(input.join("scaled.fits")).unwrap();
    assert!(invoke(&input, &output, &input.join("config.toml")).success());
    let out = FitsFile::from_file(output.join("images/scaled.fits")).unwrap();
    assert_eq!(out.primary().header.get_float("BSCALE"), Some(2.0));
    assert_eq!(out.primary().header.get_float("BZERO"), Some(100.0));
    let blank = out.primary().header.get_int("BLANK").unwrap();
    let img = match &out.primary().data {
        HduData::Image(i) => i,
        _ => panic!(),
    };
    assert!(matches!(&img.pixels,PixelData::I16(v) if i64::from(v[0])==blank && v[95*96+95]==100));
}

#[test]
fn byte_images_get_in_range_blank_sentinel() {
    let t = tempdir().unwrap();
    let input = t.path().join("in");
    let output = t.path().join("out");
    fs::create_dir(&input).unwrap();
    config(&input);
    create_fits(&input.join("byte.fits"), 8, 96, 96, &artifact());
    assert!(invoke(&input, &output, &input.join("config.toml")).success());
    let f = FitsFile::from_file(output.join("images/byte.fits")).unwrap();
    let blank = f.primary().header.get_int("BLANK").unwrap();
    assert!((0..=255).contains(&blank));
    let img = match &f.primary().data {
        HduData::Image(i) => i,
        _ => panic!(),
    };
    assert!(matches!(&img.pixels,PixelData::U8(v) if i64::from(v[0])==blank && v[95*96+95]==100));
}

#[test]
fn multiple_hdus_are_ignored() {
    let t = tempdir().unwrap();
    let input = t.path().join("in");
    let output = t.path().join("out");
    fs::create_dir(&input).unwrap();
    config(&input);
    let path = input.join("multi.fits");
    create_fits(&path, -32, 96, 96, &artifact());
    let mut f = FitsFile::from_file(&path).unwrap();
    f.push_extension(Hdu::image_extension(ImageData::new(
        vec![2, 2],
        PixelData::U8(vec![1; 4]),
    )));
    f.to_file(path).unwrap();
    assert!(invoke(&input, &output, &input.join("config.toml")).success());
    assert!(!output.join("images/multi.fits").exists());
    let report = fs::read_dir(output.join("reports"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path()
        .join("summary.json");
    let v: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(v["ignored"], 1);
}

#[test]
fn serial_and_parallel_workers_produce_identical_masks() {
    let t = tempdir().unwrap();
    let input = t.path().join("in");
    let serial = t.path().join("serial");
    let parallel = t.path().join("parallel");
    fs::create_dir(&input).unwrap();
    config(&input);
    create_fits(&input.join("a.fits"), -32, 96, 96, &artifact());
    let mut second = artifact();
    for y in 80..96 {
        for x in 72..96 {
            second[y * 96 + x] = 20.0;
        }
    }
    create_fits(&input.join("b.fits"), -64, 96, 96, &second);
    let run = |output: &Path, threads: &str| {
        Command::new(env!("CARGO_BIN_EXE_fits-auto-clipping"))
            .args(["--input-dir"])
            .arg(&input)
            .args(["--output-dir"])
            .arg(output)
            .args(["--config"])
            .arg(input.join("config.toml"))
            .args(["--threads", threads])
            .status()
            .unwrap()
    };
    assert!(run(&serial, "1").success());
    assert!(run(&parallel, "4").success());
    for name in ["a.fits", "b.fits"] {
        assert_eq!(
            read_mask(&serial.join("masks").join(name)),
            read_mask(&parallel.join("masks").join(name))
        );
    }
}

#[test]
fn dry_run_writes_reports_and_log_without_products() {
    let t = tempdir().unwrap();
    let input = t.path().join("in");
    let output = t.path().join("out");
    fs::create_dir(&input).unwrap();
    config(&input);
    create_fits(&input.join("candidate.fits"), -32, 96, 96, &artifact());
    let status = Command::new(env!("CARGO_BIN_EXE_fits-auto-clipping"))
        .args(["--input-dir"])
        .arg(&input)
        .args(["--output-dir"])
        .arg(&output)
        .args(["--config"])
        .arg(input.join("config.toml"))
        .arg("--dry-run")
        .status()
        .unwrap();
    assert!(status.success());
    assert!(!output.join("images/candidate.fits").exists());
    assert!(!output.join("masks/candidate.fits").exists());
    assert!(output.join("logger").read_dir().unwrap().next().is_some());
    assert!(output.join("reports").read_dir().unwrap().next().is_some());
}

#[test]
fn corrupt_candidate_returns_batch_error_status_and_is_reported() {
    let t = tempdir().unwrap();
    let input = t.path().join("in");
    let output = t.path().join("out");
    fs::create_dir(&input).unwrap();
    config(&input);
    fs::write(input.join("broken.fits"), b"not a FITS file").unwrap();
    assert_eq!(
        invoke(&input, &output, &input.join("config.toml")).code(),
        Some(1)
    );
    let report = fs::read_dir(output.join("reports"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path()
        .join("summary.json");
    let v: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(v["errors"], 1);
}

#[test]
fn explicit_overwrite_replaces_a_complete_output_pair() {
    let t = tempdir().unwrap();
    let input = t.path().join("in");
    let output = t.path().join("out");
    fs::create_dir(&input).unwrap();
    config(&input);
    create_fits(&input.join("candidate.fits"), -64, 96, 96, &artifact());
    let run = |overwrite: bool| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_fits-auto-clipping"));
        cmd.args(["--input-dir"])
            .arg(&input)
            .args(["--output-dir"])
            .arg(&output)
            .args(["--config"])
            .arg(input.join("config.toml"));
        if overwrite {
            cmd.arg("--overwrite");
        }
        cmd.status().unwrap()
    };
    assert!(run(false).success());
    assert!(run(true).success());
    assert!(
        FitsFile::from_file(output.join("images/candidate.fits"))
            .unwrap()
            .len()
            == 1
    );
}
