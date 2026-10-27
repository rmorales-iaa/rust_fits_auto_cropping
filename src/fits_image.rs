//! Rust-native FITS loading, eligibility checks, and detection-value conversion.
use anyhow::{Context, bail};
use fitskit::{FitsFile, HduData, Header, PixelData};
use std::{
    fs::File,
    io::{Seek, SeekFrom},
    path::Path,
};

#[derive(Debug)]
pub enum Eligibility {
    MultiHdu(usize),
    Not2d,
    EmptyImage,
}

pub struct Image {
    pub width: usize,
    pub height: usize,
    pub values: Vec<f64>,
    pub bitpix: i64,
    pub blank_value: Option<i64>,
    pub metadata_warnings: Vec<String>,
}

/// Parse a FITS file using fitskit and validate the single-HDU 2D image contract.
pub fn read_image(
    path: &Path,
    maximum_image_mib: usize,
) -> anyhow::Result<Result<Image, Eligibility>> {
    // Inspect headers and seek over payloads before allocating parsed pixel buffers.
    let (hdu_count, axes, bitpix) = preflight(path)?;
    if hdu_count != 1 {
        return Ok(Err(Eligibility::MultiHdu(hdu_count)));
    }
    if axes.len() != 2 {
        return Ok(Err(Eligibility::Not2d));
    }
    let [width, height] = [axes[0], axes[1]];
    if width == 0 || height == 0 {
        return Ok(Err(Eligibility::EmptyImage));
    }
    let n = width
        .checked_mul(height)
        .ok_or_else(|| anyhow::anyhow!("image dimensions overflow"))?;
    let max_pixels = maximum_image_mib.saturating_mul(1024 * 1024) / 32;
    if n > max_pixels {
        bail!(
            "image has {n} pixels; estimated workspace exceeds configured maximum_image_mib={maximum_image_mib}"
        );
    }
    let file =
        FitsFile::from_file(path).with_context(|| format!("opening FITS {}", path.display()))?;
    let hdu = file.primary();
    let image = match &hdu.data {
        HduData::Image(image) => image,
        _ => return Ok(Err(Eligibility::Not2d)),
    };
    if image.axes != axes {
        bail!("image dimensions changed between preflight and FITS parse");
    }
    if hdu.header.require_int("BITPIX")? != bitpix {
        bail!("BITPIX changed between preflight and FITS parse");
    }
    let scale = hdu.header.get_float("BSCALE").unwrap_or(1.0);
    let zero = hdu.header.get_float("BZERO").unwrap_or(0.0);
    if !scale.is_finite() || scale == 0.0 || !zero.is_finite() {
        bail!("image has invalid BSCALE/BZERO values");
    }
    let declared_blank = hdu.header.get_int("BLANK");
    let existing_blank = declared_blank.filter(|value| integer_value_in_range(bitpix, *value));
    let mut metadata_warnings = Vec::new();
    if let Some(declared) = declared_blank.filter(|_| existing_blank.is_none()) {
        metadata_warnings.push(format!(
            "ignored out-of-range BLANK={} for BITPIX={bitpix}; pixel statistics use stored image values",
            declared
        ));
    }
    let (values, blank_value) = match &image.pixels {
        PixelData::U8(raw) => integer_values(raw, existing_blank, scale, zero, 8)?,
        PixelData::I16(raw) => integer_values(raw, existing_blank, scale, zero, 16)?,
        PixelData::I32(raw) => integer_values(raw, existing_blank, scale, zero, 32)?,
        PixelData::I64(raw) => integer_values(raw, existing_blank, scale, zero, 64)?,
        PixelData::F32(raw) => (
            raw.iter().map(|&v| zero + scale * f64::from(v)).collect(),
            None,
        ),
        PixelData::F64(raw) => (raw.iter().map(|&v| zero + scale * v).collect(), None),
    };
    if values.len() != n {
        bail!("FITS array size does not match NAXIS dimensions");
    }
    Ok(Ok(Image {
        width,
        height,
        values,
        bitpix,
        blank_value,
        metadata_warnings,
    }))
}

fn preflight(path: &Path) -> anyhow::Result<(usize, Vec<usize>, i64)> {
    let mut file = File::open(path).with_context(|| format!("opening FITS {}", path.display()))?;
    let file_len = file.metadata()?.len();
    let mut count = 0usize;
    let mut first_axes = Vec::new();
    let mut first_bitpix = 0i64;
    loop {
        let position = file.stream_position()?;
        if position == file_len {
            break;
        }
        if position > file_len {
            bail!("FITS HDU exceeds file length");
        }
        let header = Header::read_from(&mut file)
            .with_context(|| format!("reading FITS header in {}", path.display()))?;
        if count == 0 {
            first_bitpix = header.require_int("BITPIX")?;
            let naxis = header.require_int("NAXIS")?;
            if !(0..=999).contains(&naxis) {
                bail!("invalid NAXIS={naxis}");
            }
            for axis in 1..=naxis {
                first_axes.push(
                    usize::try_from(header.require_int(&format!("NAXIS{axis}"))?)
                        .context("negative image axis")?,
                );
            }
        }
        let data_bytes =
            u64::try_from(header.data_byte_count()?).context("negative FITS payload size")?;
        let padded = data_bytes
            .checked_add(2879)
            .ok_or_else(|| anyhow::anyhow!("FITS payload size overflow"))?
            / 2880
            * 2880;
        file.seek(SeekFrom::Current(
            i64::try_from(padded).context("FITS payload too large to seek")?,
        ))?;
        if file.stream_position()? > file_len {
            bail!("truncated FITS payload");
        }
        count += 1;
    }
    if count == 0 {
        bail!("FITS file contains no HDUs");
    }
    Ok((count, first_axes, first_bitpix))
}

fn integer_value_in_range(bitpix: i64, value: i64) -> bool {
    match bitpix {
        8 => (0..=255).contains(&value),
        16 => (i16::MIN as i64..=i16::MAX as i64).contains(&value),
        32 => (i32::MIN as i64..=i32::MAX as i64).contains(&value),
        64 => true,
        _ => false,
    }
}

fn integer_values<T>(
    raw: &[T],
    existing_blank: Option<i64>,
    scale: f64,
    zero: f64,
    bitpix: i64,
) -> anyhow::Result<(Vec<f64>, Option<i64>)>
where
    T: Copy + Into<i64>,
{
    let raw: Vec<i64> = raw.iter().copied().map(Into::into).collect();
    let sentinel = existing_blank.unwrap_or(unused_integer_value(bitpix, &raw)?);
    let mut valid: Vec<i64> = raw
        .iter()
        .copied()
        .filter(|v| Some(*v) != existing_blank)
        .collect();
    if valid.is_empty() {
        bail!("integer image contains no defined pixels");
    }
    valid.sort_unstable();
    let anchor = valid[valid.len() / 2];
    let values = raw
        .iter()
        .map(|&v| {
            if Some(v) == existing_blank {
                f64::NAN
            } else {
                zero + ((v as i128 - anchor as i128) as f64) * scale
            }
        })
        .collect();
    Ok((values, Some(sentinel)))
}

pub fn unused_integer_value(bitpix: i64, raw: &[i64]) -> anyhow::Result<i64> {
    let (min, max) = match bitpix {
        8 => (0, 255),
        16 => (i16::MIN as i64, i16::MAX as i64),
        32 => (i32::MIN as i64, i32::MAX as i64),
        64 => (i64::MIN, i64::MAX),
        _ => bail!("unsupported integer BITPIX={bitpix}"),
    };
    let mut sorted = raw.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let mut candidate = min;
    for value in sorted {
        if value < candidate {
            continue;
        }
        if value > candidate {
            break;
        }
        if candidate == max {
            bail!("no unused integer pixel value is available for BLANK");
        }
        candidate += 1;
    }
    Ok(candidate)
}
