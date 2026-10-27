//! FITS products written with the pure-Rust fitskit crate.
use crate::fits_image::Image;
use anyhow::{Context, bail};
use fitskit::{FitsFile, HduData, HeaderValue, ImageData, Keyword, PixelData};
use std::{fs, path::Path};
use tempfile::NamedTempFile;

/// Create a uint8 mask image with WCS and non-structural source cards.
pub fn write_mask(
    source: &Path,
    path: &Path,
    width: usize,
    height: usize,
    mask: &[bool],
) -> anyhow::Result<()> {
    if mask.len() != width.saturating_mul(height) {
        bail!("mask dimensions do not match its data");
    }
    let source_file = FitsFile::from_file(source)?;
    let source_hdu = source_file.primary();
    let mut output = FitsFile::with_primary_image(ImageData::new(
        vec![width, height],
        PixelData::U8(mask.iter().map(|v| u8::from(*v)).collect()),
    ));
    for keyword in source_hdu.header.iter() {
        if !is_structural_or_data_summary(&keyword.name) {
            output.primary_mut().header.push(keyword.clone());
        }
    }
    output.primary_mut().header.push(Keyword::commentary(
        "HISTORY",
        "fits-auto-clipping: companion mask; 1 marks newly rejected pixels",
    ));
    publish_fits(&output, path)?;
    verify_mask_file(path, width, height, mask)
}

/// Write the science image by replacing rejected samples with FITS undefined values.
pub fn write_masked_image(
    source: &Path,
    destination: &Path,
    mask: &[bool],
    bitpix: i64,
    blank_value: Option<i64>,
) -> anyhow::Result<()> {
    let mut output = FitsFile::from_file(source)
        .with_context(|| format!("reading source FITS {}", source.display()))?;
    if output.len() != 1 {
        bail!("source changed: expected exactly one HDU");
    }
    let hdu = output.primary_mut();
    let image = match &mut hdu.data {
        HduData::Image(image) => image,
        _ => bail!("source primary HDU is not an image"),
    };
    if image.pixels.len() != mask.len() || image.bitpix().to_i64() != bitpix {
        bail!("source image dimensions or BITPIX changed during processing");
    }
    if bitpix > 0 {
        let blank = blank_value.context("integer image requires BLANK sentinel")?;
        hdu.header.set(
            "BLANK",
            HeaderValue::Integer(blank),
            Some("undefined pixel value"),
        );
        match &mut image.pixels {
            PixelData::U8(v) => {
                for (p, m) in v.iter_mut().zip(mask) {
                    if *m {
                        *p = u8::try_from(blank).context("BLANK does not fit BITPIX=8")?;
                    }
                }
            }
            PixelData::I16(v) => {
                for (p, m) in v.iter_mut().zip(mask) {
                    if *m {
                        *p = i16::try_from(blank).context("BLANK does not fit BITPIX=16")?;
                    }
                }
            }
            PixelData::I32(v) => {
                for (p, m) in v.iter_mut().zip(mask) {
                    if *m {
                        *p = i32::try_from(blank).context("BLANK does not fit BITPIX=32")?;
                    }
                }
            }
            PixelData::I64(v) => {
                for (p, m) in v.iter_mut().zip(mask) {
                    if *m {
                        *p = blank;
                    }
                }
            }
            _ => bail!("integer BITPIX does not match the pixel buffer"),
        }
    } else {
        match &mut image.pixels {
            PixelData::F32(v) => {
                for (p, m) in v.iter_mut().zip(mask) {
                    if *m {
                        *p = f32::NAN;
                    }
                }
            }
            PixelData::F64(v) => {
                for (p, m) in v.iter_mut().zip(mask) {
                    if *m {
                        *p = f64::NAN;
                    }
                }
            }
            _ => bail!("floating BITPIX does not match the pixel buffer"),
        }
    }
    hdu.header.push(Keyword::commentary(
        "HISTORY",
        "fits-auto-clipping: border pixels marked undefined",
    ));
    publish_fits(&output, destination)?;
    verify_masked_file(source, destination, mask, bitpix, blank_value)
}

/// Publish science and mask outputs together, restoring previous outputs on failure.
pub fn write_products(
    source: &Path,
    image_path: &Path,
    mask_path: &Path,
    image: &Image,
    data_mask: &[bool],
) -> anyhow::Result<()> {
    let image_backup = backup_existing(image_path)?;
    let mask_backup = backup_existing(mask_path)?;
    let result = write_masked_image(
        source,
        image_path,
        data_mask,
        image.bitpix,
        image.blank_value,
    )
    .and_then(|_| write_mask(source, mask_path, image.width, image.height, data_mask));
    if let Err(error) = result {
        let rollback =
            restore_backup(image_path, image_backup).and(restore_backup(mask_path, mask_backup));
        if let Err(rollback) = rollback {
            bail!("output failed ({error}); restoring prior products also failed ({rollback})");
        }
        return Err(error);
    }
    Ok(())
}

fn publish_fits(file: &FitsFile, destination: &Path) -> anyhow::Result<()> {
    let parent = destination
        .parent()
        .context("FITS destination has no parent")?;
    let temp = NamedTempFile::new_in(parent)?;
    let bytes = file.to_bytes_with_checksum()?;
    fs::write(temp.path(), bytes)?;
    FitsFile::from_file(temp.path())
        .context("reopening generated FITS")?
        .primary()
        .verify_datasum()?;
    temp.persist(destination)
        .map_err(|e| e.error)
        .with_context(|| format!("publishing FITS {}", destination.display()))?;
    Ok(())
}

fn backup_existing(path: &Path) -> anyhow::Result<Option<NamedTempFile>> {
    if !path.exists() {
        return Ok(None);
    }
    if path.symlink_metadata()?.file_type().is_symlink() {
        bail!("refusing to replace symlink output {}", path.display());
    }
    let backup = NamedTempFile::new_in(path.parent().context("output path has no parent")?)?;
    fs::copy(path, backup.path())?;
    Ok(Some(backup))
}

fn restore_backup(path: &Path, backup: Option<NamedTempFile>) -> anyhow::Result<()> {
    if let Some(backup) = backup {
        fs::rename(backup.path(), path)?;
    } else if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn is_structural_or_data_summary(key: &str) -> bool {
    matches!(
        key,
        "SIMPLE"
            | "XTENSION"
            | "BITPIX"
            | "NAXIS"
            | "EXTEND"
            | "PCOUNT"
            | "GCOUNT"
            | "BSCALE"
            | "BZERO"
            | "BLANK"
            | "CHECKSUM"
            | "DATASUM"
            | "DATAMIN"
            | "DATAMAX"
            | "END"
    ) || key.starts_with("NAXIS")
        || key.starts_with("ZNAXIS")
}

fn verify_mask_file(
    path: &Path,
    width: usize,
    height: usize,
    expected: &[bool],
) -> anyhow::Result<()> {
    let check = FitsFile::from_file(path)?;
    if check.len() != 1 {
        bail!("written mask does not contain exactly one HDU");
    }
    check.primary().verify_datasum()?;
    let actual = match &check.primary().data {
        HduData::Image(img) => match &img.pixels {
            PixelData::U8(v) => v,
            _ => bail!("mask BITPIX changed from 8"),
        },
        _ => bail!("mask primary HDU is not an image"),
    };
    if actual.len() != width.saturating_mul(height)
        || actual.iter().zip(expected).any(|(a, b)| *a != u8::from(*b))
    {
        bail!("mask samples or dimensions changed during write");
    }
    Ok(())
}

fn verify_masked_file(
    source: &Path,
    path: &Path,
    mask: &[bool],
    expected_bitpix: i64,
    blank_value: Option<i64>,
) -> anyhow::Result<()> {
    let original = FitsFile::from_file(source)?;
    let check = FitsFile::from_file(path)?;
    if check.len() != 1 {
        bail!("written science output does not contain exactly one HDU");
    }
    check.primary().verify_datasum()?;
    let before = match &original.primary().data {
        HduData::Image(img) => &img.pixels,
        _ => bail!("source primary HDU is not an image"),
    };
    let after_image = match &check.primary().data {
        HduData::Image(img) => img,
        _ => bail!("output primary HDU is not an image"),
    };
    if after_image.bitpix().to_i64() != expected_bitpix || after_image.pixels.len() != mask.len() {
        bail!("output BITPIX or dimensions changed");
    }
    if expected_bitpix > 0 {
        let blank = blank_value.context("integer output has no BLANK sentinel")?;
        if check.primary().header.get_int("BLANK") != Some(blank) {
            bail!("output BLANK keyword is missing or changed");
        }
        let before = integer_slice(before)?;
        let after = integer_slice(&after_image.pixels)?;
        for (i, ((got, old), rejected)) in after.iter().zip(before).zip(mask).enumerate() {
            if (*rejected && *got != blank) || (!*rejected && *got != old) {
                bail!("integer output pixel {i} violates masked/retained-value invariant");
            }
        }
    } else {
        let before = float_slice(before)?;
        let after = float_slice(&after_image.pixels)?;
        for (i, ((got, old), rejected)) in after.iter().zip(before).zip(mask).enumerate() {
            if (*rejected && !got.is_nan())
                || (!*rejected && *got != old && !(got.is_nan() && old.is_nan()))
            {
                bail!("floating output pixel {i} violates masked/retained-value invariant");
            }
        }
    }
    Ok(())
}

fn integer_slice(pixels: &PixelData) -> anyhow::Result<Vec<i64>> {
    Ok(match pixels {
        PixelData::U8(v) => v.iter().map(|&x| i64::from(x)).collect(),
        PixelData::I16(v) => v.iter().map(|&x| i64::from(x)).collect(),
        PixelData::I32(v) => v.iter().map(|&x| i64::from(x)).collect(),
        PixelData::I64(v) => v.clone(),
        _ => bail!("expected integer FITS storage"),
    })
}
fn float_slice(pixels: &PixelData) -> anyhow::Result<Vec<f64>> {
    Ok(match pixels {
        PixelData::F32(v) => v.iter().map(|&x| f64::from(x)).collect(),
        PixelData::F64(v) => v.clone(),
        _ => bail!("expected floating FITS storage"),
    })
}
