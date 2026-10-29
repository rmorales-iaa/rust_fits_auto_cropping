use fitskit::{FitsFile, Hdu, HduData, HeaderValue, ImageData, PixelData};
use flate2::{Compression, write::GzEncoder};
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
    let base = run_dir(output).join(category);
    let path = base.join(relative);
    if category == "images" && !path.exists() {
        let relative = Path::new(relative);
        let mut name = relative
            .file_stem()
            .map(std::ffi::OsString::from)
            .unwrap_or_else(|| relative.file_name().unwrap().to_os_string());
        name.push("_cropped");
        if let Some(extension) = relative.extension() {
            name.push(".");
            name.push(extension);
        }
        return base.join(relative.with_file_name(name));
    }
    path
}

fn summary_path(output: &Path) -> PathBuf {
    run_dir(output).join("summary.json")
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
    fs::write(
        dir.join("heuristics.toml"),
        "[heuristics]\ntile_size=8\nminimum_border_tiles=2\nminimum_abs_z_score=6.0\nmaximum_tile_noise_ratio=0.1\nminimum_spatial_correlation=0.95\nminimum_mask_fraction=0.01\nmaximum_mask_fraction=0.50\nminimum_evidence_ratio=1.25\nmaximum_evidence_ratio=5.0\nvalid_pixel_loss_cost=1.5\nminimum_net_utility=0.001\nmaximum_crop_sacrifice_fraction=0.35\n\n[heuristics.crop_sacrifice_fraction_by_border_component_count]\none=0.35\ntwo=0.35\nthree=0.35\nfour_or_more=0.47\n",
    )
    .unwrap();
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

fn invoke_png_with_reason_mask(
    input: &Path,
    output: &Path,
    cfg: &Path,
) -> std::process::ExitStatus {
    Command::new(env!("CARGO_BIN_EXE_rust_fits_auto_cropping"))
        .args(["--input-dir"])
        .arg(input)
        .args(["--output-dir"])
        .arg(output)
        .args(["--config"])
        .arg(cfg)
        .args(["--heuristics"])
        .arg(cfg.parent().unwrap().join("heuristics.toml"))
        .args(["--mode", "crop", "--png", "--reason-mask"])
        .status()
        .unwrap()
}

fn invoke_reason_mask(input: &Path, output: &Path, cfg: &Path) -> std::process::ExitStatus {
    Command::new(env!("CARGO_BIN_EXE_rust_fits_auto_cropping"))
        .args(["--input-dir"])
        .arg(input)
        .args(["--output-dir"])
        .arg(output)
        .args(["--config"])
        .arg(cfg)
        .args(["--heuristics"])
        .arg(cfg.parent().unwrap().join("heuristics.toml"))
        .arg("--reason-mask")
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

#[test]
fn transient_mask_trees_are_removed_and_not_listed_as_run_products() {
    let t = tempdir().unwrap();
    let input = t.path().join("in");
    let output = t.path().join("out");
    fs::create_dir(&input).unwrap();
    config(&input);
    let mut pixels = artifact();
    pixels[0] = f64::NAN;
    create_fits(&input.join("reason.fits"), -64, 96, 96, &pixels);

    assert!(invoke_reason_mask(&input, &output, &input.join("config.toml")).success());
    assert!(!run_dir(&output).join("masks").exists());
    assert!(!run_dir(&output).join("reason_masks").exists());

    let summary: serde_json::Value =
        serde_json::from_slice(&fs::read(summary_path(&output)).unwrap()).unwrap();
    assert_eq!(summary["schema_version"], 12);
    assert_eq!(summary["output_images"], summary["total"]);
    assert!(summary["files"][0]["output_reason_mask"].is_null());
    assert!(!run_dir(&output).join("manifests").exists());
    assert!(!run_dir(&output).join("previews").exists());
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
    assert!(!run_dir(&output).join("reason_masks").exists());
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
    assert!(!run_dir(&output).join("masks").exists());
    assert!(!run_dir(&output).join("reason_masks").exists());
    assert!(
        fs::symlink_metadata(product_path(&output, "images", "clean.fits"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(!run_dir(&output).join("manifests").exists());
    assert!(!run_dir(&output).join("previews").exists());
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
    assert_eq!(float_result["candidate_undefined_pixels"], 0);
    assert_eq!(
        float_result["candidate_proposed_overlap_pixels"],
        float_result["proposed_mask_pixels"]
    );
    assert_eq!(float_result["proposed_undefined_pixels"], 0);
    assert!(
        float_result["candidate_mask_pixels"].as_u64().unwrap()
            >= float_result["proposed_mask_pixels"].as_u64().unwrap()
    );
    assert_eq!(
        float_result["applied_artifact_pixels"],
        float_result["masked_pixels"]
    );
    assert_eq!(float_result["applied_artifact_undefined_pixels"], 0);
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
    let summary: serde_json::Value =
        serde_json::from_slice(&fs::read(summary_path(&output)).unwrap()).unwrap();
    let files = summary["files"].as_array().unwrap();
    assert_eq!(files[0]["input"], "base.fits");
    assert_eq!(files[1]["input"], "scaled.fits");
    assert_eq!(
        files[0]["proposed_mask_pixels"],
        files[1]["proposed_mask_pixels"]
    );
}

#[test]
fn experimental_affine_reference_is_configurable_and_reported() {
    let t = tempdir().unwrap();
    let input = t.path().join("in");
    let output = t.path().join("out");
    fs::create_dir(&input).unwrap();
    config(&input);
    let heuristics_path = input.join("heuristics.toml");
    let heuristics = fs::read_to_string(&heuristics_path).unwrap();
    fs::write(
        &heuristics_path,
        heuristics.replace(
            "[heuristics]",
            "[heuristics]\nreference_model = \"robust_affine\"",
        ),
    )
    .unwrap();
    let gradient: Vec<f64> = (0..96)
        .flat_map(|y| (0..96).map(move |x| 100.0 + 0.1 * x as f64 + 0.05 * y as f64))
        .collect();
    create_fits(&input.join("gradient.fits"), -32, 96, 96, &gradient);

    assert!(invoke(&input, &output, &input.join("config.toml")).success());
    let run = run_dir(&output);
    let summary: serde_json::Value =
        serde_json::from_slice(&fs::read(run.join("summary.json")).unwrap()).unwrap();
    assert_eq!(
        summary["algorithm_version"],
        "tile-detector-v1-robust-affine-experimental"
    );
    let activity = fs::read_to_string(
        fs::read_dir(&run)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("activity_")
            })
            .unwrap(),
    )
    .unwrap();
    assert!(activity.contains("tile-detector-v1-robust-affine-experimental"));
}

#[test]
fn experimental_multiscale_consensus_is_configurable_and_reported() {
    let t = tempdir().unwrap();
    let input = t.path().join("in");
    let output = t.path().join("out");
    fs::create_dir(&input).unwrap();
    config(&input);
    let heuristics_path = input.join("heuristics.toml");
    let heuristics = fs::read_to_string(&heuristics_path).unwrap();
    fs::write(
        &heuristics_path,
        heuristics.replace(
            "[heuristics]",
            "[heuristics]\nevidence_model = \"multiscale_consensus\"",
        ),
    )
    .unwrap();
    let (width, height) = (96, 96);
    let mut pixels = vec![100.0; width * height];
    for y in 0..32 {
        for x in 0..32 {
            pixels[y * width + x] = 0.0;
        }
    }
    create_fits(&input.join("corner.fits"), -32, width, height, &pixels);

    assert!(invoke(&input, &output, &input.join("config.toml")).success());
    let run = run_dir(&output);
    let summary: serde_json::Value =
        serde_json::from_slice(&fs::read(run.join("summary.json")).unwrap()).unwrap();
    assert_eq!(summary["clipped"], 1);
    assert_eq!(
        summary["algorithm_version"],
        "tile-detector-v1-multiscale-consensus-experimental"
    );
    let activity = fs::read_to_string(
        fs::read_dir(&run)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("activity_")
            })
            .unwrap(),
    )
    .unwrap();
    assert!(activity.contains("tile-detector-v1-multiscale-consensus-experimental"));
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
    assert!(!run_dir(&output).join("masks").exists());
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
    assert!(!run_dir(&output).join("masks").exists());
    assert!(!run_dir(&output).join("reason_masks").exists());
}

#[test]
fn crop_mode_processes_lookup_distortion_images_with_full_frame_mask_fallback() {
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
    let image_path = product_path(&output, "images", "lookup-distortion.fits");
    assert!(
        !fs::symlink_metadata(&image_path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let output_image = FitsFile::from_file(image_path).unwrap();
    assert!(matches!(
        &output_image.primary().data,
        HduData::Image(image) if matches!(&image.pixels, PixelData::F32(values)
            if !values[0].is_finite() && values[95 * 96 + 95].is_finite())
    ));
    assert!(!run_dir(&output).join("masks").exists());
    let summary: serde_json::Value =
        serde_json::from_slice(&fs::read(summary_path(&output)).unwrap()).unwrap();
    assert_eq!(summary["clipped"], 1);
    assert_eq!(summary["files"][0]["status"], "clipped");
    assert_eq!(summary["files"][0]["reason_code"], "crop_wcs_mask_fallback");
    assert!(
        summary["files"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("lookup-table distortion")
    );
    assert!(summary["files"][0]["output_mask"].is_null());
}

#[test]
fn crop_mode_outputs_maximum_rectangular_fits_for_circular_field() {
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
        .replace("one=0.35", "one=1.0")
        .replace("two=0.35", "two=1.0")
        .replace("three=0.35", "three=1.0")
        .replace("four_or_more=0.47", "four_or_more=1.0");
    fs::write(&heuristic_path, heuristic_text).unwrap();

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
    assert!(invoke_png_with_reason_mask(&input, &output, &config_path).success());

    let original = FitsFile::from_file(&source).unwrap();
    let cropped =
        FitsFile::from_file(product_path(&output, "images", "circular-field.fits")).unwrap();
    assert!(product_path(&output, "images", "circular-field_cropped.fits").exists());
    assert!(!run_dir(&output).join("images/circular-field.fits").exists());
    let header = &cropped.primary().header;
    assert_eq!(header.get_string("CLIPSHAP"), Some("RECTANGLE"));
    assert!(header.get_float("CLIPRAD").is_none());
    let x0 = header.get_int("CLIPX0").unwrap() as usize;
    let y0 = header.get_int("CLIPY0").unwrap() as usize;
    let before = match &original.primary().data {
        HduData::Image(img) => match &img.pixels {
            PixelData::I16(v) => v,
            _ => panic!("expected int16 input"),
        },
        _ => panic!("expected image"),
    };
    let after_image = match &cropped.primary().data {
        HduData::Image(img) => img,
        _ => panic!("expected image"),
    };
    let after = match &after_image.pixels {
        PixelData::I16(v) => v,
        _ => panic!("expected int16 crop"),
    };
    let (crop_width, crop_height) = (after_image.axes[0], after_image.axes[1]);
    assert_eq!(after_image.axes, [crop_width, crop_height]);
    assert_eq!(header.get_int("NAXIS").unwrap(), 2);
    assert_eq!(header.get_int("NAXIS1").unwrap() as usize, crop_width);
    assert_eq!(header.get_int("NAXIS2").unwrap() as usize, crop_height);
    assert!(crop_width < width && crop_height < height);
    for y in 0..crop_height {
        for x in 0..crop_width {
            let global_x = x + x0;
            let global_y = y + y0;
            let source_value = before[global_y * width + global_x];
            assert_eq!(
                source_value, 100,
                "crop included detected border at ({global_x},{global_y})"
            );
            assert_eq!(after[y * crop_width + x], source_value);
        }
    }
    assert!(cropped.primary().verify_datasum().is_ok());

    let preview_path = product_path(&output, "previews", "circular-field.fits.png");
    let preview = image::open(&preview_path).unwrap().to_rgb8();
    let display_crop_width = (crop_width * height + crop_height / 2) / crop_height;
    assert_eq!(
        preview.dimensions(),
        (width as u32 + display_crop_width as u32, height as u32 + 18)
    );
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(summary_path(&output)).unwrap()).unwrap();
    assert_eq!(report["files"][0]["crop_shape"], "rectangle");
    assert_eq!(report["files"][0]["crop_width"], crop_width);
    assert_eq!(report["files"][0]["crop_height"], crop_height);
    assert!(
        report["files"][0]["output_png"]
            .as_str()
            .unwrap()
            .ends_with("circular-field.fits.png")
    );
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
    assert!(!run_dir(&output).join("previews").exists());
    assert!(!run_dir(&output).join("manifests").exists());
    let summary: serde_json::Value =
        serde_json::from_slice(&fs::read(summary_path(&output)).unwrap()).unwrap();
    let image = &summary["files"][0];
    assert_eq!(image["original_undefined_pixels"], 1);
    assert_eq!(image["crop_excluded_undefined_pixels"], 1);
    assert_eq!(image["crop_applied_undefined_pixels"], 0);
    assert_eq!(image["candidate_undefined_pixels"], 0);
    assert_eq!(image["proposed_undefined_pixels"], 0);
    assert_eq!(image["applied_artifact_undefined_pixels"], 0);
    assert!(image["candidate_mask_pixels"].as_u64().unwrap() > 0);
    assert!(image["proposed_mask_pixels"].as_u64().unwrap() > 0);
    assert!(matches!(
        image["reason_code"].as_str().unwrap(),
        "crop_applied" | "crop_utility_rejected" | "crop_sacrifice_limit"
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
    assert!(!run_dir(&output).join("manifests").exists());
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
fn crop_mode_keeps_source_when_geometry_sacrifices_too_much_clean_area() {
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
        .replace("one=0.35", "one=0.10");
    fs::write(&heuristic_path, heuristic_text).unwrap();

    let source = input.join("four-corners.fits");
    create_fits(&source, 16, 96, 96, &artifact());
    assert!(invoke(&input, &output, &config_path).success());

    let output_path = product_path(&output, "images", "four-corners.fits");
    let kept = FitsFile::from_file(&output_path).unwrap();
    assert_eq!(kept.primary().header.require_int("NAXIS1").unwrap(), 96);
    assert_eq!(kept.primary().header.require_int("NAXIS2").unwrap(), 96);
    assert!(
        fs::symlink_metadata(output_path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(!product_path(&output, "masks", "four-corners.fits").exists());
    let result: serde_json::Value =
        serde_json::from_slice(&fs::read(summary_path(&output)).unwrap()).unwrap();
    assert_eq!(result["kept"], 1);
    assert_eq!(result["files"][0]["reason_code"], "crop_sacrifice_limit");
    assert!(
        result["files"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("10.0% for 1 accepted border-connected area")
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
    assert!(String::from_utf8_lossy(&result.stderr).contains("--png requires a crop mode"));
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
    assert!(!run_dir(&output).join("masks").exists());
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
    f.to_file(&path).unwrap();
    assert!(invoke(&input, &output, &input.join("config.toml")).success());
    let preserved = product_path(&output, "images", "multi.fits");
    assert!(
        fs::symlink_metadata(&preserved)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::canonicalize(preserved).unwrap(),
        fs::canonicalize(path).unwrap()
    );
    let report = summary_path(&output);
    let v: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(v["output_images"], 1);
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
    assert!(!run_dir(&serial).join("masks").exists());
    assert!(!run_dir(&serial).join("reason_masks").exists());
    assert!(!run_dir(&parallel).join("masks").exists());
    assert!(!run_dir(&parallel).join("reason_masks").exists());
    for name in ["a.fits", "b.fits"] {
        assert!(run_dir(&serial).join("images").join(name).exists());
        assert!(run_dir(&parallel).join("images").join(name).exists());
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
    assert_eq!(summary["schema_version"], 12);
    assert_eq!(
        summary["output_images"], 0,
        "dry-run writes no image products"
    );
    assert_eq!(summary["algorithm_version"], "tile-detector-v1");
    assert_eq!(
        summary["policy_version"],
        "heuristics-v4-crop-fallback-actions"
    );
    assert_eq!(summary["executable_sha256"].as_str().unwrap().len(), 64);
    assert_eq!(summary["source_config_sha256"].as_str().unwrap().len(), 64);
    assert_eq!(summary["clipped"], 0);
    assert_eq!(summary["proposed"], 1);
    assert_eq!(summary["files"][0]["masked_pixels"], 0);
    assert_eq!(summary["files"][0]["reason_code"], "dry_run_proposal");
    assert_eq!(summary["files"][0]["applied_artifact_pixels"], 0);
    assert!(summary["files"][0].get("output_manifest").is_none());
    assert!(!run_dir(&output).join("manifests").exists());
    assert!(!run_dir(&output).join("previews").exists());
    assert!(
        summary["files"][0]["proposed_mask_pixels"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(summary["files"][0]["status"], "proposed");
}

#[test]
fn malformed_nonstructural_header_card_is_annotated_and_image_is_processed() {
    let t = tempdir().unwrap();
    let input = t.path().join("in");
    let output = t.path().join("out");
    fs::create_dir(&input).unwrap();
    config(&input);

    let source = input.join("bad-metadata.fits");
    create_fits(&source, -32, 96, 96, &artifact());
    let mut bytes = fs::read(&source).unwrap();
    let card_index = bytes
        .chunks_exact(80)
        .position(|card| &card[..8] == b"OBJECT  ")
        .expect("test FITS has an OBJECT card");
    let card = &mut bytes[card_index * 80..(card_index + 1) * 80];
    let opening_quote = card.iter().position(|&byte| byte == b'\'').unwrap();
    let closing_quote = card.iter().rposition(|&byte| byte == b'\'').unwrap();
    assert!(closing_quote > opening_quote);
    card[closing_quote] = b' ';
    let original_bad_card: [u8; 80] = card.try_into().unwrap();
    let history_index = bytes
        .chunks_exact(80)
        .position(|card| &card[..8] == b"HISTORY ")
        .expect("test FITS has a HISTORY card");
    let mut hierarch_card = [b' '; 80];
    let hierarch_text = b"HIERARCH CAHA INS APERT NAME = 'test'";
    hierarch_card[..hierarch_text.len()].copy_from_slice(hierarch_text);
    bytes[history_index * 80..(history_index + 1) * 80].copy_from_slice(&hierarch_card);
    fs::write(&source, bytes).unwrap();

    assert!(invoke(&input, &output, &input.join("config.toml")).success());
    let result: serde_json::Value =
        serde_json::from_slice(&fs::read(summary_path(&output)).unwrap()).unwrap();
    assert_eq!(result["total"], 1);
    assert_eq!(result["errors"], 0);
    assert_eq!(result["clipped"], 1);
    assert_eq!(result["files"][0]["status"], "clipped");
    let reason = result["files"][0]["reason"].as_str().unwrap();
    assert!(reason.contains("ignored malformed non-structural FITS header card"));
    assert!(reason.contains("OBJECT"));
    assert!(reason.contains("unterminated string value"));

    let output_path = product_path(&output, "images", "bad-metadata.fits");
    assert!(
        !fs::symlink_metadata(&output_path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let output_bytes = fs::read(&output_path).unwrap();
    assert!(
        output_bytes
            .chunks_exact(80)
            .any(|card| card == original_bad_card)
    );
    assert!(
        output_bytes
            .chunks_exact(80)
            .any(|card| card == hierarch_card)
    );
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
    assert_eq!(v["total"], 1);
    assert_eq!(v["output_images"], 1);
    assert_eq!(v["errors"], 1);
    assert_eq!(v["files"][0]["reason_code"], "input_read_error");
    let original = input.join("broken.fits");
    let preserved = product_path(&output, "images", "broken.fits");
    assert!(
        fs::symlink_metadata(&preserved)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::canonicalize(&preserved).unwrap(),
        fs::canonicalize(&original).unwrap()
    );
    assert_eq!(fs::read(&preserved).unwrap(), fs::read(&original).unwrap());
    assert!(
        v["files"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("source preserved unchanged by symlink")
    );
    assert!(!run_dir(&output).join("manifests").exists());
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
    assert_eq!(v["output_images"], v["total"]);
    assert_eq!(v["ignored"], 4);
    assert_eq!(v["errors"], 0);
    for name in [
        "multi.fits",
        "compressed.fits.gz",
        "compressed-but-named-fits.fits",
        "tile-compressed.fits.fz",
    ] {
        let source = input.join(name);
        let preserved = product_path(&output, "images", name);
        assert!(
            fs::symlink_metadata(&preserved)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::canonicalize(&preserved).unwrap(),
            fs::canonicalize(&source).unwrap()
        );
        assert_eq!(fs::read(&preserved).unwrap(), fs::read(&source).unwrap());
    }
}

#[test]
fn flat_output_writes_products_directly_under_output_directory() {
    let t = tempdir().unwrap();
    let input = t.path().join("in");
    let output = t.path().join("science_calibrated");
    fs::create_dir(&input).unwrap();
    fs::create_dir(&output).unwrap();
    fs::write(output.join("stale-output.txt"), b"remove before use").unwrap();
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
        .args(["--mode", "crop", "--flat-output", "--sanitize-output"])
        .status()
        .unwrap();
    assert!(status.success());

    assert!(
        output.join("candidate.fits").is_file(),
        "flat-output entries: {:?}",
        fs::read_dir(&output)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>()
    );
    assert!(!output.join("images").exists());
    assert!(output.join("summary.json").is_file());
    assert!(output.join("per-image.csv").is_file());
    assert!(!output.join("stale-output.txt").exists());
    assert!(!output.join("runs").exists());
    assert!(!output.join("previews").exists());
    assert!(!output.join("manifests").exists());
    let summary: serde_json::Value =
        serde_json::from_slice(&fs::read(output.join("summary.json")).unwrap()).unwrap();
    assert_eq!(summary["total"], 1);
    assert_eq!(summary["output_images"], 1);
    assert!(
        summary["files"][0]["output_image"]
            .as_str()
            .unwrap()
            .starts_with(output.to_str().unwrap())
    );

    let repeated_status = Command::new(env!("CARGO_BIN_EXE_rust_fits_auto_cropping"))
        .args(["--input-dir"])
        .arg(&input)
        .args(["--output-dir"])
        .arg(&output)
        .args(["--config"])
        .arg(input.join("config.toml"))
        .args(["--heuristics"])
        .arg(input.join("heuristics.toml"))
        .args(["--mode", "crop", "--flat-output", "--overwrite"])
        .status()
        .unwrap();
    assert!(repeated_status.success());
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

fn crop_config(input: &Path, extra_heuristics: &str, mode: &str) -> PathBuf {
    config(input);
    let config_path = input.join("config.toml");
    let text = fs::read_to_string(&config_path)
        .unwrap()
        .replace("mode=\"mask\"", &format!("mode=\"{mode}\""));
    fs::write(&config_path, text).unwrap();
    let heuristic_path = input.join("heuristics.toml");
    let text = fs::read_to_string(&heuristic_path).unwrap().replace(
        "maximum_crop_sacrifice_fraction=0.35\n",
        &format!("maximum_crop_sacrifice_fraction=0.35\n{extra_heuristics}"),
    );
    fs::write(&heuristic_path, text).unwrap();
    config_path
}

/// Left strip (16 columns) plus a top-right 16x16 corner on a 96x96 frame.
fn strip_and_corner() -> Vec<f64> {
    let mut pixels = vec![100.0; 96 * 96];
    for y in 0..96 {
        for x in 0..96 {
            if x < 16 || (x >= 80 && y >= 80) {
                pixels[y * 96 + x] = 0.0;
            }
        }
    }
    pixels
}

#[test]
fn bbox_fallback_crops_to_retained_support_and_masks_remaining_flags() {
    for bitpix in [-32_i64, 16] {
        let t = tempdir().unwrap();
        let input = t.path().join("in");
        let output = t.path().join("out");
        fs::create_dir(&input).unwrap();
        let config_path = crop_config(&input, "crop_fallback=\"bbox_mask\"\n", "crop");
        // Force rejection of every rectangle by the sacrifice cap.
        let heuristic_path = input.join("heuristics.toml");
        let text = fs::read_to_string(&heuristic_path)
            .unwrap()
            .replace("one=0.35", "one=0.05")
            .replace("two=0.35", "two=0.05")
            .replace("three=0.35", "three=0.05")
            .replace("four_or_more=0.47", "four_or_more=0.05");
        fs::write(&heuristic_path, text).unwrap();
        let source_pixels = strip_and_corner();
        create_fits(&input.join("frame.fits"), bitpix, 96, 96, &source_pixels);
        assert!(invoke(&input, &output, &config_path).success());

        let summary: serde_json::Value =
            serde_json::from_slice(&fs::read(summary_path(&output)).unwrap()).unwrap();
        let record = &summary["files"][0];
        assert_eq!(record["action"], "bbox_mask", "{record}");
        assert_eq!(record["reason_code"], "crop_fallback_bbox_mask");
        assert_eq!(record["status"], "clipped");
        assert_eq!(record["unmasked_pixels_sacrificed"], 0);
        assert_eq!(record["crop_artifact_pixels_retained"], 0);
        assert_eq!(record["crop_origin_x"], 16);
        assert_eq!(record["crop_width"], 80);
        assert_eq!(record["crop_height"], 96);
        assert_eq!(record["crop_interior_masked_pixels"], 256);
        assert_eq!(
            record["artifact_pixels_removed"], record["proposed_mask_pixels"],
            "every accepted flag is removed"
        );

        let path = product_path(&output, "images", "frame.fits");
        assert!(path.to_string_lossy().ends_with("frame_cropped.fits"));
        assert!(
            !fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        let product = FitsFile::from_file(&path).unwrap();
        let header = &product.primary().header;
        assert_eq!(header.require_int("NAXIS1").unwrap(), 80);
        assert_eq!(header.require_int("NAXIS2").unwrap(), 96);
        assert_eq!(header.get_int("CLIPX0"), Some(16));
        assert_eq!(header.get_float("CRPIX1"), Some(48.5 - 16.0));
        product.primary().verify_datasum().unwrap();
        let HduData::Image(data) = &product.primary().data else {
            panic!()
        };
        let blank = header.get_int("BLANK");
        for y in 0..96 {
            for x in 0..80 {
                let source = source_pixels[y * 96 + x + 16];
                let flagged = x + 16 >= 80 && y >= 80;
                match &data.pixels {
                    PixelData::F32(v) => {
                        let got = v[y * 80 + x];
                        assert!(if flagged {
                            got.is_nan()
                        } else {
                            got == source as f32
                        });
                    }
                    PixelData::I16(v) => {
                        let got = i64::from(v[y * 80 + x]);
                        if flagged {
                            assert_eq!(Some(got), blank);
                        } else {
                            assert_eq!(got, source as i64);
                            assert_ne!(Some(got), blank);
                        }
                    }
                    other => panic!("unexpected storage {other:?}"),
                }
            }
        }
    }
}

#[test]
fn crop_masked_mode_keeps_larger_rectangle_with_flags_undefined() {
    let t = tempdir().unwrap();
    let input = t.path().join("in");
    let output = t.path().join("out");
    fs::create_dir(&input).unwrap();
    let config_path = crop_config(
        &input,
        "maximum_residual_artifact_fraction=0.05\ncrop_fallback=\"bbox_mask\"\n",
        "crop_masked",
    );
    let (width, height) = (192, 96);
    let mut pixels = vec![100.0; width * height];
    for y in 0..height {
        for x in 0..8 {
            pixels[y * width + x] = 0.0;
        }
    }
    create_fits(&input.join("strip.fits"), -32, width, height, &pixels);
    assert!(invoke(&input, &output, &config_path).success());
    let summary: serde_json::Value =
        serde_json::from_slice(&fs::read(summary_path(&output)).unwrap()).unwrap();
    let record = &summary["files"][0];
    assert_eq!(record["action"], "crop_masked", "{record}");
    assert_eq!(record["reason_code"], "crop_masked_applied");
    assert_eq!(record["crop_width"], 192);
    assert_eq!(record["crop_interior_masked_pixels"], 768);
    assert_eq!(record["crop_artifact_pixels_retained"], 0);
    assert_eq!(record["unmasked_pixels_sacrificed"], 0);
    let product = FitsFile::from_file(product_path(&output, "images", "strip.fits")).unwrap();
    let HduData::Image(data) = &product.primary().data else {
        panic!()
    };
    let PixelData::F32(values) = &data.pixels else {
        panic!()
    };
    for y in 0..height {
        for x in 0..width {
            let got = values[y * width + x];
            assert!(if x < 8 { got.is_nan() } else { got == 100.0 });
        }
    }
}

#[test]
fn legacy_rules_without_fallback_still_keep_rejected_crops() {
    let t = tempdir().unwrap();
    let input = t.path().join("in");
    let output = t.path().join("out");
    fs::create_dir(&input).unwrap();
    let config_path = crop_config(&input, "", "crop");
    let heuristic_path = input.join("heuristics.toml");
    let text = fs::read_to_string(&heuristic_path)
        .unwrap()
        .replace("one=0.35", "one=0.05")
        .replace("two=0.35", "two=0.05")
        .replace("three=0.35", "three=0.05")
        .replace("four_or_more=0.47", "four_or_more=0.05");
    fs::write(&heuristic_path, text).unwrap();
    create_fits(&input.join("frame.fits"), -32, 96, 96, &strip_and_corner());
    assert!(invoke(&input, &output, &config_path).success());
    let summary: serde_json::Value =
        serde_json::from_slice(&fs::read(summary_path(&output)).unwrap()).unwrap();
    assert_eq!(summary["files"][0]["action"], "keep");
    assert_eq!(summary["files"][0]["reason_code"], "crop_sacrifice_limit");
    assert!(
        fs::symlink_metadata(product_path(&output, "images", "frame.fits"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}
