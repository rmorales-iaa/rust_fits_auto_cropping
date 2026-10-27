use fitskit::{FitsFile, Hdu, HduData, HeaderValue, ImageData, PixelData};
use flate2::{Compression, write::GzEncoder};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};
use tempfile::tempdir;

fn run_dir(output: &Path) -> PathBuf {
    let mut runs: Vec<PathBuf> = fs::read_dir(output.join("runs"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .collect();
    runs.sort();
    runs.pop().expect("at least one run directory")
}

fn product_path(output: &Path, category: &str, relative: &str) -> PathBuf {
    run_dir(output).join(category).join(relative)
}

fn summary_path(output: &Path) -> PathBuf {
    run_dir(output).join("summary.json")
}

fn file_sha256(path: &Path) -> String {
    format!("{:x}", Sha256::digest(fs::read(path).unwrap()))
}

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
    fs::write(
        dir.join("config.toml"),
        "[processing]\nthreads=2\nrecursive=false\nmemory_budget_mib=1024\nmaximum_image_mib=256\nmode=\"mask\"\n",
    )
    .unwrap();
    fs::write(dir.join("heuristics.toml"), "[heuristics]\ntile_size=8\nminimum_border_tiles=2\nminimum_abs_z_score=6.0\nmaximum_tile_noise_ratio=0.1\nminimum_spatial_correlation=0.95\nminimum_mask_fraction=0.01\nmaximum_mask_fraction=0.50\nminimum_evidence_ratio=1.25\nmaximum_evidence_ratio=5.0\nvalid_pixel_loss_cost=1.5\nminimum_net_utility=0.001\n").unwrap();
}

fn invoke(input: &Path, output: &Path, cfg: &Path) -> std::process::ExitStatus {
    Command::new(env!("CARGO_BIN_EXE_rust_fits_auto_cropping"))
        .args(["--input-dir"])
        .arg(input)
        .args(["--output-dir"])
        .arg(output)
        .args(["--config"])
        .arg(cfg)
        .args(["--heuristics"])
        .arg(cfg.parent().unwrap().join("heuristics.toml"))
        .status()
        .unwrap()
}

fn invoke_recursive(input: &Path, output: &Path, cfg: &Path) -> std::process::ExitStatus {
    Command::new(env!("CARGO_BIN_EXE_rust_fits_auto_cropping"))
        .args(["--input-dir"])
        .arg(input)
        .args(["--output-dir"])
        .arg(output)
        .args(["--config"])
        .arg(cfg)
        .args(["--heuristics"])
        .arg(cfg.parent().unwrap().join("heuristics.toml"))
        .arg("--recursive")
        .status()
        .unwrap()
}

fn invoke_png(input: &Path, output: &Path, cfg: &Path) -> std::process::ExitStatus {
    Command::new(env!("CARGO_BIN_EXE_rust_fits_auto_cropping"))
        .args(["--input-dir"])
        .arg(input)
        .args(["--output-dir"])
        .arg(output)
        .args(["--config"])
        .arg(cfg)
        .args(["--heuristics"])
        .arg(cfg.parent().unwrap().join("heuristics.toml"))
        .args(["--mode", "crop", "--png"])
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
    let mut source = FitsFile::from_file(input.join("float.fits")).unwrap();
    if let HduData::Image(image) = &mut source.primary_mut().data
        && let PixelData::F64(values) = &mut image.pixels
    {
        values[0] = f64::NAN;
    }
    source
        .primary_mut()
        .header
        .set("BUNIT", HeaderValue::String("adu".into()), None);
    source
        .primary_mut()
        .header
        .set("DATAMIN", HeaderValue::Float(0.0), None);
    source
        .primary_mut()
        .header
        .set("DATAMAX", HeaderValue::Float(100.0), None);
    source.to_file(input.join("float.fits")).unwrap();
    create_fits(
        &input.join("clean.fits"),
        -64,
        96,
        96,
        &vec![100.0; 96 * 96],
    );
    assert!(invoke(&input, &output, &input.join("config.toml")).success());
    let f = FitsFile::from_file(product_path(&output, "images", "float.fits")).unwrap();
    let img = match &f.primary().data {
        HduData::Image(i) => i,
        _ => panic!(),
    };
    assert!(matches!(&img.pixels,PixelData::F64(v) if v[0].is_nan() && v[95*96+95]==100.0));
    assert_eq!(f.primary().header.get_string("OBJECT"), Some("test-field"));
    assert_eq!(f.primary().header.get_string("BUNIT"), Some("adu"));
    assert_eq!(f.primary().header.get_float("DATAMIN"), None);
    assert_eq!(f.primary().header.get_float("DATAMAX"), None);
    assert!(
        f.primary()
            .header
            .get_string("CREATOR")
            .unwrap()
            .contains(env!("RUST_FITS_AUTO_CROPPING_VERSION"))
    );
    let product_date = f.primary().header.get_string("DATE").unwrap();
    assert!(product_date.len() >= 19 && product_date.contains('T'));
    assert!(f.primary().header.iter().any(|card| {
        card.name == "HISTORY"
            && card
                .comment
                .as_deref()
                .is_some_and(|value| value.contains("generated_utc="))
    }));
    assert!(f.primary().verify_datasum().is_ok());
    let mask = read_mask(&product_path(&output, "masks", "float.fits"));
    assert_eq!(mask.len(), 96 * 96);
    assert_eq!(mask[0], 0);
    assert_eq!(mask[95 * 96 + 95], 0);
    assert!(
        fs::symlink_metadata(product_path(&output, "images", "clean.fits"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let clean_manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(
            summary_path(&output)
                .parent()
                .unwrap()
                .join("manifests/clean.fits.complete.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(clean_manifest["complete"], true);
    assert_eq!(clean_manifest["manifest_version"], 3);
    assert_eq!(clean_manifest["input_sha256"].as_str().unwrap().len(), 64);
    assert_eq!(clean_manifest["products"][0]["symlink"], true);
    assert_eq!(
        clean_manifest["products"][0]["sha256"],
        file_sha256(&product_path(&output, "images", "clean.fits"))
    );
    let m = FitsFile::from_file(product_path(&output, "masks", "float.fits")).unwrap();
    assert_eq!(m.primary().header.get_float("CRPIX1"), Some(48.5));
    assert_eq!(m.primary().header.get_string("BUNIT"), None);
    assert_eq!(m.primary().header.get_string("CTYPE1"), Some("RA---TAN"));
    let report = summary_path(&output);
    let v: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    let float_result = v["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["input"] == "float.fits")
        .unwrap();
    assert_eq!(float_result["original_undefined_pixels"], 1);
    assert_eq!(float_result["reason_code"], "mask_applied");
    assert!(
        float_result["candidate_mask_pixels"].as_u64().unwrap()
            >= float_result["proposed_mask_pixels"].as_u64().unwrap()
    );
    assert_eq!(
        float_result["applied_artifact_pixels"],
        float_result["masked_pixels"]
    );
    assert_eq!(float_result["crop_excluded_undefined_pixels"], 0);
    assert_eq!(
        float_result["masked_pixels"],
        float_result["proposed_mask_pixels"]
    );
    assert_eq!(v["program"], "rust_fits_auto_cropping");
    assert_eq!(v["version"], env!("RUST_FITS_AUTO_CROPPING_VERSION"));
    assert_eq!(v["clipped"], 1);
    assert_eq!(v["kept"], 1);
    assert_eq!(v["total"], 2);
    assert!(product_path(&output, "images", "clean.fits").is_file());
    assert!(!product_path(&output, "masks", "clean.fits").exists());
    assert!(run_dir(&output).read_dir().unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("activity_")
    }));
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
    let out = FitsFile::from_file(product_path(&output, "images", "large-offset.fits")).unwrap();
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
    let out = FitsFile::from_file(product_path(&output, "images", "scaled.fits")).unwrap();
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
    let f = FitsFile::from_file(product_path(&output, "images", "byte.fits")).unwrap();
    let blank = f.primary().header.get_int("BLANK").unwrap();
    assert!((0..=255).contains(&blank));
    let img = match &f.primary().data {
        HduData::Image(i) => i,
        _ => panic!(),
    };
    assert!(matches!(&img.pixels,PixelData::U8(v) if i64::from(v[0])==blank && v[95*96+95]==100));
}

#[test]
fn full_range_byte_image_is_kept_without_eager_blank_allocation() {
    let t = tempdir().unwrap();
    let input = t.path().join("in");
    let output = t.path().join("out");
    fs::create_dir(&input).unwrap();
    config(&input);
    let samples: Vec<f64> = (0..96 * 96).map(|i| (i % 256) as f64).collect();
    create_fits(&input.join("full-range.fits"), 8, 96, 96, &samples);
    assert!(invoke(&input, &output, &input.join("config.toml")).success());
    let output_image = product_path(&output, "images", "full-range.fits");
    assert!(
        fs::symlink_metadata(output_image)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let summary: serde_json::Value =
        serde_json::from_slice(&fs::read(summary_path(&output)).unwrap()).unwrap();
    assert_eq!(summary["errors"], 0);
    assert_eq!(summary["kept"], 1);
}

#[test]
fn integer_mask_without_available_blank_keeps_the_source_unchanged() {
    let t = tempdir().unwrap();
    let input = t.path().join("in");
    let output = t.path().join("out");
    fs::create_dir(&input).unwrap();
    config(&input);
    let (width, height) = (96, 96);
    let mut values = vec![100_u8; width * height];
    for y in 0..24 {
        for x in 0..24 {
            values[y * width + x] = 0;
        }
    }
    for value in 0..=255_u8 {
        let offset = 40 * width + 40 + usize::from(value);
        values[offset] = value;
    }
    let source = input.join("no-blank-code.fits");
    FitsFile::with_primary_image(ImageData::new(vec![width, height], PixelData::U8(values)))
        .to_file(&source)
        .unwrap();

    assert!(invoke(&input, &output, &input.join("config.toml")).success());
    let image_path = product_path(&output, "images", "no-blank-code.fits");
    assert!(
        fs::symlink_metadata(image_path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(!product_path(&output, "masks", "no-blank-code.fits").exists());
    let summary: serde_json::Value =
        serde_json::from_slice(&fs::read(summary_path(&output)).unwrap()).unwrap();
    assert_eq!(summary["errors"], 0);
    assert_eq!(summary["kept"], 1);
    assert_eq!(summary["files"][0]["masked_pixels"], 0);
    assert!(
        summary["files"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("no unused integer pixel value")
    );
}

#[test]
fn detection_is_invariant_to_scaling_and_descriptive_headers() {
    let t = tempdir().unwrap();
    let input = t.path().join("in");
    let output = t.path().join("out");
    fs::create_dir(&input).unwrap();
    config(&input);
    let base = input.join("base.fits");
    let scaled = input.join("scaled.fits");
    create_fits(&base, 16, 96, 96, &artifact());
    fs::copy(&base, &scaled).unwrap();
    let mut file = FitsFile::from_file(&scaled).unwrap();
    file.primary_mut()
        .header
        .set("BZERO", HeaderValue::Float(1.0e15), None);
    file.primary_mut()
        .header
        .set("BSCALE", HeaderValue::Float(0.003), None);
    file.primary_mut().header.set(
        "OBJECT",
        HeaderValue::String("unrelated-object-name".into()),
        None,
    );
    file.to_file(&scaled).unwrap();
    assert!(invoke(&input, &output, &input.join("config.toml")).success());
    let base_mask = read_mask(&product_path(&output, "masks", "base.fits"));
    let scaled_mask = read_mask(&product_path(&output, "masks", "scaled.fits"));
    assert_eq!(base_mask, scaled_mask);
}

#[test]
fn existing_blank_pixels_remain_undefined_and_are_not_newly_masked() {
    let t = tempdir().unwrap();
    let input = t.path().join("in");
    let output = t.path().join("out");
    fs::create_dir(&input).unwrap();
    config(&input);
    let mut raw = vec![100i16; 96 * 96];
    for y in 0..16 {
        for x in 0..24 {
            raw[y * 96 + x] = 0;
        }
    }
    raw[48 * 96 + 48] = i16::MIN;
    let mut file = FitsFile::with_primary_image(ImageData::new(vec![96, 96], PixelData::I16(raw)));
    file.primary_mut()
        .header
        .set("BLANK", HeaderValue::Integer(i16::MIN as i64), None);
    file.to_file(input.join("with-blank.fits")).unwrap();
    assert!(invoke(&input, &output, &input.join("config.toml")).success());
    let mask = read_mask(&product_path(&output, "masks", "with-blank.fits"));
    assert_eq!(mask[48 * 96 + 48], 0);
    let out = FitsFile::from_file(product_path(&output, "images", "with-blank.fits")).unwrap();
    assert_eq!(out.primary().header.get_int("BLANK"), Some(i16::MIN as i64));
    let img = match &out.primary().data {
        HduData::Image(v) => v,
        _ => panic!("expected image"),
    };
    assert!(matches!(&img.pixels, PixelData::I16(v) if v[48*96+48] == i16::MIN));
}

#[test]
fn out_of_range_blank_is_ignored_for_detection_and_replaced_in_masked_output() {
    let t = tempdir().unwrap();
    let input = t.path().join("in");
    let output = t.path().join("out");
    fs::create_dir(&input).unwrap();
    config(&input);
    create_fits(&input.join("invalid-blank.fits"), 16, 96, 96, &artifact());
    let mut file = FitsFile::from_file(input.join("invalid-blank.fits")).unwrap();
    file.primary_mut()
        .header
        .set("BLANK", HeaderValue::Integer(32768), None);
    file.to_file(input.join("invalid-blank.fits")).unwrap();
    assert!(invoke(&input, &output, &input.join("config.toml")).success());
    let out = FitsFile::from_file(product_path(&output, "images", "invalid-blank.fits")).unwrap();
    let blank = out.primary().header.get_int("BLANK").unwrap();
    assert!((-32768..=32767).contains(&blank));
    let img = match &out.primary().data {
        HduData::Image(v) => v,
        _ => panic!("expected image"),
    };
    assert!(matches!(&img.pixels, PixelData::I16(v) if i64::from(v[0]) == blank));
    let report = summary_path(&output);
    let result: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(result["errors"], 0);
    assert!(
        result["files"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("ignored out-of-range BLANK=32768")
    );
}

#[test]
fn crop_mode_writes_maximal_clean_rectangle_for_single_side_and_shifts_reference_pixels() {
    let t = tempdir().unwrap();
    let input = t.path().join("in");
    let output = t.path().join("out");
    fs::create_dir(&input).unwrap();
    config(&input);
    let config_path = input.join("config.toml");
    let config_text = fs::read_to_string(&config_path)
        .unwrap()
        .replace("mode=\"mask\"", "mode=\"crop\"");
    fs::write(&config_path, config_text).unwrap();

    let (width, height) = (96, 96);
    let mut data = vec![100.0; width * height];
    for y in 0..16 {
        for x in 0..width {
            data[y * width + x] = 0.0;
        }
    }
    let input_path = input.join("four-corners.fits");
    create_fits(&input_path, 16, width, height, &data);
    assert!(invoke(&input, &output, &config_path).success());

    let original = FitsFile::from_file(&input_path).unwrap();
    let cropped =
        FitsFile::from_file(product_path(&output, "images", "four-corners.fits")).unwrap();
    let before = match &original.primary().data {
        HduData::Image(image) => match &image.pixels {
            PixelData::I16(values) => values,
            _ => panic!("expected int16 source"),
        },
        _ => panic!("expected image"),
    };
    let after = match &cropped.primary().data {
        HduData::Image(image) => match &image.pixels {
            PixelData::I16(values) => values,
            _ => panic!("expected int16 crop"),
        },
        _ => panic!("expected image"),
    };
    let (crop_x, crop_y, crop_width, crop_height) = (0, 16, 96, 80);
    assert_eq!(
        cropped.primary().header.require_int("NAXIS1").unwrap(),
        crop_width as i64
    );
    assert_eq!(
        cropped.primary().header.require_int("NAXIS2").unwrap(),
        crop_height as i64
    );
    assert_eq!(
        cropped.primary().header.get_int("CLIPX0"),
        Some(crop_x as i64)
    );
    assert_eq!(
        cropped.primary().header.get_int("CLIPY0"),
        Some(crop_y as i64)
    );
    assert_eq!(
        cropped.primary().header.get_float("CRPIX1"),
        original
            .primary()
            .header
            .get_float("CRPIX1")
            .map(|v| v - crop_x as f64)
    );
    assert_eq!(
        cropped.primary().header.get_float("CRPIX2"),
        original
            .primary()
            .header
            .get_float("CRPIX2")
            .map(|v| v - crop_y as f64)
    );
    for y in 0..crop_height {
        let src = (crop_y + y) * width + crop_x;
        let dst = y * crop_width;
        assert_eq!(
            &after[dst..dst + crop_width],
            &before[src..src + crop_width]
        );
    }
    assert!(cropped.primary().verify_datasum().is_ok());
    let mask_file =
        FitsFile::from_file(product_path(&output, "masks", "four-corners.fits")).unwrap();
    assert_eq!(
        mask_file.primary().header.require_int("NAXIS1").unwrap(),
        width as i64
    );
    assert_eq!(
        mask_file.primary().header.require_int("NAXIS2").unwrap(),
        height as i64
    );
}

#[test]
fn crop_mode_keeps_images_with_lookup_table_distortions() {
    let t = tempdir().unwrap();
    let input = t.path().join("in");
    let output = t.path().join("out");
    fs::create_dir(&input).unwrap();
    config(&input);
    let config_path = input.join("config.toml");
    let config_text = fs::read_to_string(&config_path)
        .unwrap()
        .replace("mode=\"mask\"", "mode=\"crop\"");
    fs::write(&config_path, config_text).unwrap();
    let source = input.join("lookup-distortion.fits");
    create_fits(&source, -32, 96, 96, &artifact());
    let mut fits = FitsFile::from_file(&source).unwrap();
    fits.primary_mut()
        .header
        .set("CPDIS1", HeaderValue::String("LOOKUP".into()), None);
    fits.to_file(&source).unwrap();

    assert!(invoke(&input, &output, &config_path).success());
    assert!(
        fs::symlink_metadata(product_path(&output, "images", "lookup-distortion.fits"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(!product_path(&output, "masks", "lookup-distortion.fits").exists());
    let summary: serde_json::Value =
        serde_json::from_slice(&fs::read(summary_path(&output)).unwrap()).unwrap();
    assert_eq!(summary["kept"], 1);
    assert!(
        summary["files"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("lookup-table distortion")
    );
}

#[test]
fn crop_mode_selects_circle_when_it_retains_more_pixels_than_rectangle() {
    let t = tempdir().unwrap();
    let input = t.path().join("in");
    let output = t.path().join("out");
    fs::create_dir(&input).unwrap();
    config(&input);
    let config_path = input.join("config.toml");
    let config_text = fs::read_to_string(&config_path)
        .unwrap()
        .replace("mode=\"mask\"", "mode=\"crop\"");
    fs::write(&config_path, config_text).unwrap();

    let (width, height) = (128, 128);
    let mut pixels = vec![0.0; width * height];
    for y in 0..height {
        for x in 0..width {
            if (x as f64 - 63.5).powi(2) + (y as f64 - 63.5).powi(2) <= 56.0_f64.powi(2) {
                pixels[y * width + x] = 100.0;
            }
        }
    }
    let source = input.join("circular-field.fits");
    create_fits(&source, 16, width, height, &pixels);
    assert!(invoke_png(&input, &output, &config_path).success());

    let original = FitsFile::from_file(&source).unwrap();
    let cropped =
        FitsFile::from_file(product_path(&output, "images", "circular-field.fits")).unwrap();
    assert_eq!(
        cropped.primary().header.get_string("CLIPSHAP"),
        Some("CIRCLE")
    );
    let x0 = cropped.primary().header.get_int("CLIPX0").unwrap() as usize;
    let y0 = cropped.primary().header.get_int("CLIPY0").unwrap() as usize;
    let cx = cropped.primary().header.get_float("CLIPCX").unwrap();
    let cy = cropped.primary().header.get_float("CLIPCY").unwrap();
    let radius = cropped.primary().header.get_float("CLIPRAD").unwrap();
    let blank = cropped.primary().header.get_int("BLANK").unwrap() as i16;
    let before = match &original.primary().data {
        HduData::Image(img) => match &img.pixels {
            PixelData::I16(v) => v,
            _ => panic!("expected int16 input"),
        },
        _ => panic!("expected image"),
    };
    let after_image = match &cropped.primary().data {
        HduData::Image(img) => img,
        _ => panic!("expected cropped image"),
    };
    let after = match &after_image.pixels {
        PixelData::I16(v) => v,
        _ => panic!("expected int16 crop"),
    };
    let crop_width = after_image.axes[0];
    for y in 0..after_image.axes[1] {
        for x in 0..crop_width {
            let global_x = x + x0;
            let global_y = y + y0;
            let inside =
                (global_x as f64 - cx).powi(2) + (global_y as f64 - cy).powi(2) <= radius.powi(2);
            let actual = after[y * crop_width + x];
            if inside {
                assert_eq!(actual, before[global_y * width + global_x]);
            } else {
                assert_eq!(actual, blank);
            }
        }
    }
    assert!(cropped.primary().verify_datasum().is_ok());
    let preview_path = product_path(&output, "previews", "circular-field.fits.png");
    let preview = image::open(&preview_path).unwrap().to_luma8();
    assert_eq!(
        preview.dimensions(),
        (
            width as u32 + crop_width as u32,
            height.max(after_image.axes[1]) as u32 + 18
        )
    );
    // Panels use the same source-derived display interval and FITS lower origin.
    let local_x = 20usize;
    let local_y = after_image.axes[1] / 2;
    let global_x = x0 + local_x;
    let global_y = y0 + local_y;
    if (global_x as f64 - cx).powi(2) + (global_y as f64 - cy).powi(2) <= radius.powi(2) {
        assert_eq!(
            preview.get_pixel(global_x as u32, 18 + height as u32 - 1 - global_y as u32),
            preview.get_pixel(
                width as u32 + local_x as u32,
                18 + after_image.axes[1] as u32 - 1 - local_y as u32,
            )
        );
    }
    assert_eq!(
        preview
            .get_pixel(width as u32, 18 + after_image.axes[1] as u32 - 1)
            .0[0],
        0
    );
    let report = summary_path(&output);
    let summary: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert!(
        summary["files"][0]["output_png"]
            .as_str()
            .unwrap()
            .ends_with("circular-field.fits.png")
    );
    assert_eq!(
        summary["files"][0]["output_manifest"],
        summary_path(&output)
            .parent()
            .unwrap()
            .join("manifests/circular-field.fits.complete.json")
            .to_string_lossy()
            .as_ref()
    );
    let manifest_path = summary_path(&output)
        .parent()
        .unwrap()
        .join("manifests/circular-field.fits.complete.json");
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    assert_eq!(manifest["complete"], true);
    assert_eq!(manifest["manifest_version"], 3);
    assert_eq!(manifest["input_sha256"].as_str().unwrap().len(), 64);
    assert_eq!(summary["files"][0]["reason_code"], "crop_applied");
    assert_eq!(
        summary["files"][0]["applied_artifact_pixels"],
        summary["files"][0]["artifact_pixels_removed"]
    );
    assert_eq!(summary["files"][0]["crop_excluded_undefined_pixels"], 0);
    assert_eq!(manifest["products"].as_array().unwrap().len(), 3);
    for product in manifest["products"].as_array().unwrap() {
        let product_path = run_dir(&output).join(product["path"].as_str().unwrap());
        assert_eq!(product["sha256"], file_sha256(&product_path));
    }
    assert!(
        manifest["products"]
            .as_array()
            .unwrap()
            .iter()
            .any(|product| product["kind"] == "preview")
    );
    let output_mask = read_mask(&product_path(&output, "masks", "circular-field.fits"));
    assert_eq!(output_mask.len(), width * height);
    assert_eq!(output_mask[0], 1);
    assert_eq!(output_mask[64 * width + 64], 0);
}

#[test]
fn crop_accounting_reports_preexisting_undefined_geometry_overlap() {
    let t = tempdir().unwrap();
    let input = t.path().join("in");
    let output = t.path().join("out");
    fs::create_dir(&input).unwrap();
    config(&input);
    let mut pixels = vec![100.0; 96 * 96];
    for y in 0..16 {
        for x in 0..24 {
            pixels[y * 96 + x] = 0.0;
        }
    }
    pixels[0] = f64::NAN;
    create_fits(&input.join("undefined-corner.fits"), -64, 96, 96, &pixels);

    let status = Command::new(env!("CARGO_BIN_EXE_rust_fits_auto_cropping"))
        .args(["--input-dir"])
        .arg(&input)
        .args(["--output-dir"])
        .arg(&output)
        .args(["--config"])
        .arg(input.join("config.toml"))
        .args(["--heuristics"])
        .arg(input.join("heuristics.toml"))
        .args(["--mode", "crop"])
        .status()
        .unwrap();
    assert!(status.success());
    let summary: serde_json::Value =
        serde_json::from_slice(&fs::read(summary_path(&output)).unwrap()).unwrap();
    let image = &summary["files"][0];
    assert_eq!(image["original_undefined_pixels"], 1);
    assert_eq!(image["crop_excluded_undefined_pixels"], 1);
    assert!(image["candidate_mask_pixels"].as_u64().unwrap() > 0);
    assert!(image["proposed_mask_pixels"].as_u64().unwrap() > 0);
    assert!(matches!(
        image["reason_code"].as_str().unwrap(),
        "crop_applied" | "crop_utility_rejected"
    ));
}

#[test]
fn crop_mode_keeps_source_when_valid_pixel_loss_dominates() {
    let t = tempdir().unwrap();
    let input = t.path().join("in");
    let output = t.path().join("out");
    fs::create_dir(&input).unwrap();
    config(&input);
    let config_path = input.join("config.toml");
    let config_text = fs::read_to_string(&config_path)
        .unwrap()
        .replace("mode=\"mask\"", "mode=\"crop\"");
    fs::write(&config_path, config_text).unwrap();
    let heuristic_path = input.join("heuristics.toml");
    let heuristic_text = fs::read_to_string(&heuristic_path)
        .unwrap()
        .replace("valid_pixel_loss_cost=1.5", "valid_pixel_loss_cost=4.0");
    fs::write(&heuristic_path, heuristic_text).unwrap();

    let source = input.join("four-corners.fits");
    create_fits(&source, 16, 96, 96, &artifact());
    assert!(invoke(&input, &output, &config_path).success());

    let kept = FitsFile::from_file(product_path(&output, "images", "four-corners.fits")).unwrap();
    assert_eq!(kept.primary().header.require_int("NAXIS1").unwrap(), 96);
    assert_eq!(kept.primary().header.require_int("NAXIS2").unwrap(), 96);
    assert_eq!(kept.primary().header.get_int("CLIPX0"), None);
    assert!(
        fs::symlink_metadata(product_path(&output, "images", "four-corners.fits"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(!product_path(&output, "masks", "four-corners.fits").exists());
    let report = summary_path(&output);
    let result: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(result["kept"], 1);
    let manifest_path = summary_path(&output)
        .parent()
        .unwrap()
        .join("manifests/four-corners.fits.complete.json");
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(manifest_path).unwrap()).unwrap();
    assert_eq!(manifest["complete"], true);
    assert_eq!(manifest["products"][0]["kind"], "science_image");
    assert_eq!(manifest["products"][0]["symlink"], true);
    assert!(
        result["files"][0]["unmasked_pixels_sacrificed"]
            .as_u64()
            .unwrap()
            > 0,
        "{}",
        result["files"][0]
    );
    assert!(result["files"][0]["net_utility"].as_f64().unwrap() < 0.0);
    assert!(
        result["files"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("rejected; source retained")
    );
}

#[test]
fn png_flag_requires_effective_crop_mode() {
    let t = tempdir().unwrap();
    let input = t.path().join("in");
    let output = t.path().join("out");
    fs::create_dir(&input).unwrap();
    config(&input);
    let result = Command::new(env!("CARGO_BIN_EXE_rust_fits_auto_cropping"))
        .args(["--input-dir"])
        .arg(&input)
        .args(["--output-dir"])
        .arg(&output)
        .args(["--config"])
        .arg(input.join("config.toml"))
        .arg("--png")
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&result.stderr).contains("--png requires crop mode"));
    assert!(!output.exists());
}

#[test]
fn int32_and_odd_rectangular_images_keep_dimensions() {
    let t = tempdir().unwrap();
    let input = t.path().join("in");
    let output = t.path().join("out");
    fs::create_dir(&input).unwrap();
    config(&input);
    let (width, height) = (97, 89);
    let mut data = vec![100.0; width * height];
    for y in 0..16 {
        for x in 0..24 {
            data[y * width + x] = 0.0;
        }
    }
    create_fits(&input.join("odd-int32.fits"), 32, width, height, &data);
    assert!(invoke(&input, &output, &input.join("config.toml")).success());
    let out = FitsFile::from_file(product_path(&output, "images", "odd-int32.fits")).unwrap();
    let mask = read_mask(&product_path(&output, "masks", "odd-int32.fits"));
    assert_eq!(mask.len(), width * height);
    let img = match &out.primary().data {
        HduData::Image(v) => v,
        _ => panic!("expected image"),
    };
    assert_eq!(img.axes, vec![width, height]);
    assert!(
        matches!(&img.pixels, PixelData::I32(v) if v[0] == out.primary().header.get_int("BLANK").unwrap() as i32 && v[(height-1)*width+width-1] == 100)
    );
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
    assert!(!product_path(&output, "images", "multi.fits").exists());
    let report = summary_path(&output);
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
        Command::new(env!("CARGO_BIN_EXE_rust_fits_auto_cropping"))
            .args(["--input-dir"])
            .arg(&input)
            .args(["--output-dir"])
            .arg(output)
            .args(["--config"])
            .arg(input.join("config.toml"))
            .args(["--heuristics"])
            .arg(input.join("heuristics.toml"))
            .args(["--threads", threads])
            .status()
            .unwrap()
    };
    assert!(run(&serial, "1").success());
    assert!(run(&parallel, "4").success());
    let summary = |root: &Path| {
        let path = summary_path(root);
        serde_json::from_slice::<serde_json::Value>(&fs::read(path).unwrap()).unwrap()
    };
    let serial_files = summary(&serial)["files"].as_array().unwrap().clone();
    let parallel_files = summary(&parallel)["files"].as_array().unwrap().clone();
    assert_eq!(serial_files.len(), parallel_files.len());
    for (a, b) in serial_files.iter().zip(&parallel_files) {
        for key in [
            "input",
            "status",
            "candidate_tiles",
            "score",
            "evidence_ratio",
            "artifact_benefit",
            "valid_pixel_loss",
            "net_utility",
        ] {
            assert_eq!(a[key], b[key], "parallel mismatch in {key}");
        }
    }
    for name in ["a.fits", "b.fits"] {
        let serial_mask = run_dir(&serial).join("masks").join(name);
        let parallel_mask = run_dir(&parallel).join("masks").join(name);
        assert_eq!(serial_mask.exists(), parallel_mask.exists());
        if serial_mask.exists() {
            assert_eq!(read_mask(&serial_mask), read_mask(&parallel_mask));
        } else {
            assert!(run_dir(&serial).join("images").join(name).exists());
            assert!(run_dir(&parallel).join("images").join(name).exists());
        }
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
    let status = Command::new(env!("CARGO_BIN_EXE_rust_fits_auto_cropping"))
        .args(["--input-dir"])
        .arg(&input)
        .args(["--output-dir"])
        .arg(&output)
        .args(["--config"])
        .arg(input.join("config.toml"))
        .args(["--heuristics"])
        .arg(input.join("heuristics.toml"))
        .arg("--dry-run")
        .status()
        .unwrap();
    assert!(status.success());
    assert!(!product_path(&output, "images", "candidate.fits").exists());
    assert!(!product_path(&output, "masks", "candidate.fits").exists());
    assert!(run_dir(&output).read_dir().unwrap().next().is_some());
    assert!(summary_path(&output).is_file());
    let summary: serde_json::Value =
        serde_json::from_slice(&fs::read(summary_path(&output)).unwrap()).unwrap();
    assert_eq!(summary["dry_run"], true);
    assert_eq!(summary["schema_version"], 5);
    assert_eq!(summary["algorithm_version"], "tile-detector-v1");
    assert_eq!(summary["policy_version"], "heuristics-v1");
    assert_eq!(summary["executable_sha256"].as_str().unwrap().len(), 64);
    assert_eq!(summary["source_config_sha256"].as_str().unwrap().len(), 64);
    assert_eq!(summary["clipped"], 0);
    assert_eq!(summary["proposed"], 1);
    assert_eq!(summary["files"][0]["masked_pixels"], 0);
    assert_eq!(summary["files"][0]["reason_code"], "dry_run_proposal");
    assert_eq!(summary["files"][0]["applied_artifact_pixels"], 0);
    assert!(summary["files"][0]["output_manifest"].is_null());
    assert!(
        summary["files"][0]["proposed_mask_pixels"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(summary["files"][0]["status"], "proposed");
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
    let report = summary_path(&output);
    let v: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(v["errors"], 1);
    assert!(
        !summary_path(&output)
            .parent()
            .unwrap()
            .join("manifests/broken.fits.complete.json")
            .exists()
    );
}

#[test]
fn each_run_is_isolated_and_previous_results_are_preserved() {
    let t = tempdir().unwrap();
    let input = t.path().join("in");
    let output = t.path().join("out");
    fs::create_dir(&input).unwrap();
    config(&input);
    create_fits(&input.join("candidate.fits"), -64, 96, 96, &artifact());
    let run = |overwrite: bool| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_rust_fits_auto_cropping"));
        cmd.args(["--input-dir"])
            .arg(&input)
            .args(["--output-dir"])
            .arg(&output)
            .args(["--config"])
            .arg(input.join("config.toml"))
            .args(["--heuristics"])
            .arg(input.join("heuristics.toml"));
        if overwrite {
            cmd.arg("--overwrite");
        }
        cmd.status().unwrap()
    };
    assert!(run(false).success());
    let first_run = run_dir(&output);
    assert!(run(true).success());
    let second_run = run_dir(&output);
    assert_ne!(first_run, second_run);
    assert!(first_run.join("summary.json").is_file());
    assert!(first_run.join("images/candidate.fits").is_file());
    assert!(fs::read_dir(output.join("runs")).unwrap().count() == 2);
    assert!(
        FitsFile::from_file(product_path(&output, "images", "candidate.fits"))
            .unwrap()
            .len()
            == 1
    );
}

#[test]
fn compressed_and_multi_hdu_fits_are_ignored() {
    let t = tempdir().unwrap();
    let input = t.path().join("in");
    let output = t.path().join("out");
    fs::create_dir(&input).unwrap();
    config(&input);
    let regular = input.join("multi.fits");
    create_fits(&regular, -32, 96, 96, &artifact());
    let mut multi = FitsFile::from_file(&regular).unwrap();
    multi.push_extension(Hdu::image_extension(ImageData::new(
        vec![2, 2],
        PixelData::U8(vec![1; 4]),
    )));
    multi.to_file(&regular).unwrap();
    let plain = input.join("compressed-source.fits");
    create_fits(&plain, -32, 96, 96, &artifact());
    let fits_bytes = fs::read(&plain).unwrap();
    let mut gz = GzEncoder::new(
        fs::File::create(input.join("compressed.fits.gz")).unwrap(),
        Compression::fast(),
    );
    gz.write_all(&fits_bytes).unwrap();
    gz.finish().unwrap();
    let mut disguised = GzEncoder::new(
        fs::File::create(input.join("compressed-but-named-fits.fits")).unwrap(),
        Compression::fast(),
    );
    disguised.write_all(&fits_bytes).unwrap();
    disguised.finish().unwrap();
    fs::remove_file(plain).unwrap();
    let image = ImageData::new(vec![96, 96], PixelData::I16(vec![7; 96 * 96]));
    let compressed = image
        .compress(&fitskit::CompressOptions::default())
        .unwrap();
    let mut tiled = FitsFile::with_empty_primary();
    tiled.push_extension(compressed);
    tiled
        .to_file(input.join("tile-compressed.fits.fz"))
        .unwrap();
    assert!(invoke(&input, &output, &input.join("config.toml")).success());
    let report = summary_path(&output);
    let v: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(v["total"], 4);
    assert_eq!(v["ignored"], 4);
    assert_eq!(v["errors"], 0);
    assert!(!product_path(&output, "images", "multi.fits").exists());
}

#[test]
fn recursive_flag_discovers_nested_fits() {
    let t = tempdir().unwrap();
    let input = t.path().join("in");
    let nested = input.join("night1");
    let output = t.path().join("out");
    fs::create_dir_all(&nested).unwrap();
    config(&input);
    create_fits(&nested.join("candidate.fits"), -32, 96, 96, &artifact());
    assert!(invoke_recursive(&input, &output, &input.join("config.toml")).success());
    assert!(product_path(&output, "images", "night1/candidate.fits").exists());
}
