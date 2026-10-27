//! FITS products written with the pure-Rust fitskit crate.
use crate::fits_image::{Image, unused_integer_value};
use crate::{
    crop::{CropRect, CropRegion},
    durability::sync_parent,
};
use anyhow::{Context, bail};
use chrono::{SecondsFormat, Utc};
use fitskit::{FitsFile, HduData, Header, HeaderValue, ImageData, Keyword, PixelData};
use std::{fs, io::Cursor, path::Path};
use tempfile::{NamedTempFile, TempDir};

/// Create a uint8 mask image with WCS and non-structural source cards.
pub fn write_mask(
    source_header: &Header,
    path: &Path,
    width: usize,
    height: usize,
    mask: &[bool],
    history: &str,
) -> anyhow::Result<()> {
    if mask.len() != width.saturating_mul(height) {
        bail!("mask dimensions do not match its data");
    }
    let mut output = FitsFile::with_primary_image(ImageData::new(
        vec![width, height],
        PixelData::U8(mask.iter().map(|v| u8::from(*v)).collect()),
    ));
    for keyword in source_header.iter() {
        if !is_structural_or_data_summary(&keyword.name) && keyword.name != "BUNIT" {
            output.primary_mut().header.push(keyword.clone());
        }
    }
    stamp_processing_metadata(&mut output.primary_mut().header);
    push_history(
        &mut output.primary_mut().header,
        &format!("rust_fits_auto_cropping: {history}"),
    );
    publish_fits(&output, path)?;
    verify_mask_file(path, width, height, mask)
}

/// Write the science image by replacing rejected samples with FITS undefined values.
pub fn write_masked_image(
    source: &FitsFile,
    destination: &Path,
    mask: &[bool],
    bitpix: i64,
    blank_value: Option<i64>,
) -> anyhow::Result<()> {
    let mut output = source.clone();
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
    if bitpix > 0 && mask.iter().any(|masked| *masked) {
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
    remove_data_extrema(&mut hdu.header);
    stamp_processing_metadata(&mut hdu.header);
    push_history(
        &mut hdu.header,
        "rust_fits_auto_cropping: border pixels marked undefined",
    );
    publish_fits(&output, destination)?;
    verify_masked_file(source, destination, mask, bitpix, blank_value)
}

/// Publish science and mask outputs together, restoring previous outputs on failure.
pub fn write_products(
    image_path: &Path,
    mask_path: &Path,
    image: &Image,
    data_mask: &[bool],
) -> anyhow::Result<()> {
    let blank_value = if image.bitpix > 0 && data_mask.iter().any(|masked| *masked) {
        Some(match image.blank_value {
            Some(blank) => blank,
            None => choose_mask_blank(&image.source_fits, image.bitpix, data_mask)?,
        })
    } else {
        image.blank_value
    };
    write_product_pair(
        image_path,
        mask_path,
        |staged_image| {
            write_masked_image(
                &image.source_fits,
                staged_image,
                data_mask,
                image.bitpix,
                blank_value,
            )
        },
        |staged_mask| {
            write_mask(
                &image.source_fits.primary().header,
                staged_mask,
                image.width,
                image.height,
                data_mask,
                "companion mask; 1 marks newly rejected pixels",
            )
        },
    )
}

/// Write a crop and a full-frame mask describing every source pixel excluded from it.
pub fn write_cropped_products(
    image_path: &Path,
    mask_path: &Path,
    excluded_mask: &[bool],
    image: &Image,
    region: CropRegion,
) -> anyhow::Result<()> {
    write_product_pair(
        image_path,
        mask_path,
        |staged_image| write_cropped_image(staged_image, image, region),
        |staged_mask| {
            write_mask(
                &image.source_fits.primary().header,
                staged_mask,
                image.width,
                image.height,
                excluded_mask,
                "crop exclusion mask; 1 marks pixels excluded from retained geometry",
            )
        },
    )
}

fn write_product_pair(
    image_path: &Path,
    mask_path: &Path,
    write_image: impl FnOnce(&Path) -> anyhow::Result<()>,
    write_mask: impl FnOnce(&Path) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let image_stage = create_stage_directory(image_path)?;
    let mask_stage = create_stage_directory(mask_path)?;
    let staged_image = image_stage.path().join("science.fits");
    let staged_mask = mask_stage.path().join("mask.fits");

    // Both complete products pass their writers' FITS/sample verification
    // before either final path is changed.
    write_image(&staged_image)?;
    write_mask(&staged_mask)?;
    publish_product_pair(&staged_image, image_path, &staged_mask, mask_path)
}

fn create_stage_directory(destination: &Path) -> anyhow::Result<TempDir> {
    let parent = destination
        .parent()
        .context("FITS destination has no parent")?;
    tempfile::tempdir_in(parent)
        .with_context(|| format!("creating product staging directory in {}", parent.display()))
}

fn publish_product_pair(
    staged_image: &Path,
    image_path: &Path,
    staged_mask: &Path,
    mask_path: &Path,
) -> anyhow::Result<()> {
    publish_product_pair_with(
        staged_image,
        image_path,
        staged_mask,
        mask_path,
        |from, to| fs::rename(from, to),
    )
}

fn publish_product_pair_with<F>(
    staged_image: &Path,
    image_path: &Path,
    staged_mask: &Path,
    mask_path: &Path,
    mut publish: F,
) -> anyhow::Result<()>
where
    F: FnMut(&Path, &Path) -> std::io::Result<()>,
{
    let image_backup = backup_existing(image_path)?;
    let mask_backup = backup_existing(mask_path)?;
    let result = publish(staged_image, image_path)
        .and_then(|()| publish(staged_mask, mask_path))
        .and_then(|()| sync_parent(image_path))
        .and_then(|()| {
            if image_path.parent() == mask_path.parent() {
                Ok(())
            } else {
                sync_parent(mask_path)
            }
        });
    if let Err(error) = result {
        let image_rollback = restore_backup(image_path, image_backup);
        let mask_rollback = restore_backup(mask_path, mask_backup);
        match (image_rollback, mask_rollback) {
            (Ok(()), Ok(())) => return Err(error.into()),
            (image, mask) => bail!(
                "publishing product pair failed ({error}); rollback image={image:?}, mask={mask:?}"
            ),
        }
    }
    Ok(())
}

/// Crop raw stored values, preserve non-structural cards and shift all primary/alternate CRPIX.
pub fn write_cropped_image(
    destination: &Path,
    image_info: &Image,
    region: CropRegion,
) -> anyhow::Result<()> {
    let mut output = image_info.source_fits.clone();
    if output.len() != 1 {
        bail!("source changed: expected exactly one HDU");
    }
    let hdu = output.primary_mut();
    let image = match &mut hdu.data {
        HduData::Image(image) => image,
        _ => bail!("source primary HDU is not an image"),
    };
    let width = image.axes.first().copied().context("missing image width")?;
    let height = image.axes.get(1).copied().context("missing image height")?;
    let rect = region.bounds();
    if image.axes.len() != 2
        || rect.width == 0
        || rect.height == 0
        || rect.x.checked_add(rect.width).is_none_or(|x| x > width)
        || rect.y.checked_add(rect.height).is_none_or(|y| y > height)
    {
        bail!("crop rectangle is outside the source image");
    }
    let circle_blank = if matches!(region, CropRegion::Circle { .. }) && image_info.bitpix > 0 {
        Some(match image_info.blank_value {
            Some(blank) => blank,
            None => unused_integer_value(
                image_info.bitpix,
                &retained_integer_samples(&image.pixels, width, region)?,
            )?,
        })
    } else {
        image_info.blank_value
    };
    let before = image.pixels.clone();
    image.pixels = crop_region_pixel_data(
        &image.pixels,
        width,
        region,
        circle_blank,
        image_info.bitpix,
    )?;
    image.axes = vec![rect.width, rect.height];
    for axis in 1..=2 {
        let prefix = format!("CRPIX{axis}");
        let offset = if axis == 1 { rect.x } else { rect.y } as f64;
        let keys: Vec<String> = hdu
            .header
            .iter()
            .filter(|card| is_crpix_keyword(&card.name, &prefix))
            .map(|card| card.name.clone())
            .collect();
        for key in keys {
            if let Some(value) = hdu.header.get_float(&key) {
                hdu.header.set(
                    &key,
                    HeaderValue::Float(value - offset),
                    Some("reference pixel after clipping crop"),
                );
            }
        }
    }
    let bitpix = image.pixels.bitpix().to_i64();
    if let Some(blank) = hdu.header.get_int("BLANK")
        && !integer_value_in_range(bitpix, blank)
    {
        hdu.header.keywords.retain(|card| card.name != "BLANK");
    }
    if let CropRegion::Circle { .. } = region
        && bitpix > 0
    {
        let blank = circle_blank.context("integer circle crop has no valid BLANK sentinel")?;
        hdu.header.set(
            "BLANK",
            HeaderValue::Integer(blank),
            Some("undefined pixel value outside selected circle"),
        );
    }
    remove_data_extrema(&mut hdu.header);
    hdu.header.set(
        "CLIPX0",
        HeaderValue::Integer(rect.x as i64),
        Some("zero-based crop origin in original image"),
    );
    hdu.header.set(
        "CLIPY0",
        HeaderValue::Integer(rect.y as i64),
        Some("zero-based crop origin in original image"),
    );
    hdu.header.set(
        "CLIPSHAP",
        HeaderValue::String(region.shape_name().to_ascii_uppercase()),
        Some("pixel-derived crop geometry"),
    );
    if let CropRegion::Circle {
        center_x,
        center_y,
        radius,
        ..
    } = region
    {
        hdu.header.set(
            "CLIPCX",
            HeaderValue::Float(center_x as f64),
            Some("zero-based circle center in original image"),
        );
        hdu.header.set(
            "CLIPCY",
            HeaderValue::Float(center_y as f64),
            Some("zero-based circle center in original image"),
        );
        hdu.header.set(
            "CLIPRAD",
            HeaderValue::Float(radius),
            Some("artifact-free circle radius in pixels"),
        );
    }
    stamp_processing_metadata(&mut hdu.header);
    push_history(
        &mut hdu.header,
        match region {
            CropRegion::Rectangle(_) => {
                "rust_fits_auto_cropping: selected clean axis-aligned rectangle"
            }
            CropRegion::Circle { .. } => "rust_fits_auto_cropping: selected clean circle",
        },
    );
    publish_fits(&output, destination)?;
    verify_cropped_file(
        &image_info.source_fits,
        destination,
        &before,
        width,
        image_info,
        region,
    )
}

fn stamp_processing_metadata(header: &mut Header) {
    let now = Utc::now();
    let creator = format!(
        "{} v{}",
        env!("CARGO_PKG_NAME"),
        env!("RUST_FITS_AUTO_CROPPING_VERSION")
    );
    header.set(
        "CREATOR",
        HeaderValue::String(creator.clone()),
        Some("software that created this FITS product"),
    );
    header.set(
        "DATE",
        HeaderValue::String(now.format("%Y-%m-%dT%H:%M:%S").to_string()),
        Some("UTC product creation date"),
    );
    push_history(
        header,
        &format!(
            "generated_utc={} by {creator}",
            now.to_rfc3339_opts(SecondsFormat::Nanos, true)
        ),
    );
}

fn crop_pixel_data(pixels: &PixelData, source_width: usize, rect: CropRect) -> PixelData {
    macro_rules! crop_values {
        ($values:expr, $variant:ident) => {{
            let mut cropped = Vec::with_capacity(rect.area());
            for y in rect.y..rect.y + rect.height {
                let start = y * source_width + rect.x;
                cropped.extend_from_slice(&$values[start..start + rect.width]);
            }
            PixelData::$variant(cropped)
        }};
    }
    match pixels {
        PixelData::U8(v) => crop_values!(v, U8),
        PixelData::I16(v) => crop_values!(v, I16),
        PixelData::I32(v) => crop_values!(v, I32),
        PixelData::I64(v) => crop_values!(v, I64),
        PixelData::F32(v) => crop_values!(v, F32),
        PixelData::F64(v) => crop_values!(v, F64),
    }
}

fn crop_region_pixel_data(
    pixels: &PixelData,
    source_width: usize,
    region: CropRegion,
    blank_value: Option<i64>,
    bitpix: i64,
) -> anyhow::Result<PixelData> {
    let rect = region.bounds();
    let mut cropped = crop_pixel_data(pixels, source_width, rect);
    if let CropRegion::Circle { .. } = region {
        match &mut cropped {
            PixelData::U8(values) => {
                let blank =
                    u8::try_from(blank_value.context("circle crop requires integer BLANK")?)?;
                for y in 0..rect.height {
                    for x in 0..rect.width {
                        if !region.retains(rect.x + x, rect.y + y) {
                            values[y * rect.width + x] = blank;
                        }
                    }
                }
            }
            PixelData::I16(values) => {
                let blank =
                    i16::try_from(blank_value.context("circle crop requires integer BLANK")?)?;
                for y in 0..rect.height {
                    for x in 0..rect.width {
                        if !region.retains(rect.x + x, rect.y + y) {
                            values[y * rect.width + x] = blank;
                        }
                    }
                }
            }
            PixelData::I32(values) => {
                let blank =
                    i32::try_from(blank_value.context("circle crop requires integer BLANK")?)?;
                for y in 0..rect.height {
                    for x in 0..rect.width {
                        if !region.retains(rect.x + x, rect.y + y) {
                            values[y * rect.width + x] = blank;
                        }
                    }
                }
            }
            PixelData::I64(values) => {
                let blank = blank_value.context("circle crop requires integer BLANK")?;
                for y in 0..rect.height {
                    for x in 0..rect.width {
                        if !region.retains(rect.x + x, rect.y + y) {
                            values[y * rect.width + x] = blank;
                        }
                    }
                }
            }
            PixelData::F32(values) => {
                for y in 0..rect.height {
                    for x in 0..rect.width {
                        if !region.retains(rect.x + x, rect.y + y) {
                            values[y * rect.width + x] = f32::NAN;
                        }
                    }
                }
            }
            PixelData::F64(values) => {
                for y in 0..rect.height {
                    for x in 0..rect.width {
                        if !region.retains(rect.x + x, rect.y + y) {
                            values[y * rect.width + x] = f64::NAN;
                        }
                    }
                }
            }
        }
    }
    if bitpix > 0
        && let CropRegion::Circle { .. } = region
    {
        let blank = blank_value.context("circle crop requires integer BLANK")?;
        if !integer_value_in_range(bitpix, blank) {
            bail!("circle crop BLANK sentinel is outside its stored BITPIX range");
        }
    }
    Ok(cropped)
}

fn choose_mask_blank(source: &FitsFile, bitpix: i64, mask: &[bool]) -> anyhow::Result<i64> {
    let image = match &source.primary().data {
        HduData::Image(image) => image,
        _ => bail!("source primary HDU is not an image"),
    };
    if image.pixels.len() != mask.len() {
        bail!("source dimensions changed before BLANK selection");
    }
    let retained: Vec<i64> = match &image.pixels {
        PixelData::U8(values) => values
            .iter()
            .zip(mask)
            .filter_map(|(&value, &excluded)| (!excluded).then_some(i64::from(value)))
            .collect(),
        PixelData::I16(values) => values
            .iter()
            .zip(mask)
            .filter_map(|(&value, &excluded)| (!excluded).then_some(i64::from(value)))
            .collect(),
        PixelData::I32(values) => values
            .iter()
            .zip(mask)
            .filter_map(|(&value, &excluded)| (!excluded).then_some(i64::from(value)))
            .collect(),
        PixelData::I64(values) => values
            .iter()
            .zip(mask)
            .filter_map(|(&value, &excluded)| (!excluded).then_some(value))
            .collect(),
        _ => bail!("integer BITPIX does not match source pixel storage"),
    };
    unused_integer_value(bitpix, &retained)
}

fn retained_integer_samples(
    pixels: &PixelData,
    source_width: usize,
    region: CropRegion,
) -> anyhow::Result<Vec<i64>> {
    let rect = region.bounds();
    let mut samples = Vec::with_capacity(region.retained_pixels());
    macro_rules! collect {
        ($values:expr) => {{
            for y in rect.y..rect.y + rect.height {
                for x in rect.x..rect.x + rect.width {
                    if region.retains(x, y) {
                        samples.push(i64::try_from($values[y * source_width + x])?);
                    }
                }
            }
        }};
    }
    match pixels {
        PixelData::U8(values) => collect!(values),
        PixelData::I16(values) => collect!(values),
        PixelData::I32(values) => collect!(values),
        PixelData::I64(values) => {
            for y in rect.y..rect.y + rect.height {
                for x in rect.x..rect.x + rect.width {
                    if region.retains(x, y) {
                        samples.push(values[y * source_width + x]);
                    }
                }
            }
        }
        _ => bail!("integer crop sentinel requested for floating pixels"),
    }
    Ok(samples)
}

fn remove_data_extrema(header: &mut Header) {
    header
        .keywords
        .retain(|card| !matches!(card.name.as_str(), "DATAMIN" | "DATAMAX"));
}

fn push_history(header: &mut Header, text: &str) {
    const CARD_TEXT_BYTES: usize = 60;
    let mut remaining = text.trim();
    while !remaining.is_empty() {
        let mut end = remaining.len().min(CARD_TEXT_BYTES);
        while !remaining.is_char_boundary(end) {
            end -= 1;
        }
        if end < remaining.len()
            && let Some(boundary) = remaining[..end].rfind(' ')
        {
            end = boundary.max(1);
        }
        header
            .keywords
            .push(Keyword::commentary("HISTORY", &remaining[..end]));
        remaining = remaining[end..].trim_start();
    }
}

fn is_crpix_keyword(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix).is_some_and(|suffix| {
        suffix.is_empty() || (suffix.len() == 1 && suffix.as_bytes()[0].is_ascii_uppercase())
    })
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

fn verify_cropped_file(
    original: &FitsFile,
    destination: &Path,
    before: &PixelData,
    source_width: usize,
    image_info: &Image,
    region: CropRegion,
) -> anyhow::Result<()> {
    let rect = region.bounds();
    let expected_blank = if matches!(region, CropRegion::Circle { .. }) && image_info.bitpix > 0 {
        Some(match image_info.blank_value {
            Some(blank) => blank,
            None => unused_integer_value(
                image_info.bitpix,
                &retained_integer_samples(before, source_width, region)?,
            )?,
        })
    } else {
        image_info.blank_value
    };
    let check = FitsFile::from_file(destination)?;
    if check.len() != 1 {
        bail!("cropped output does not contain exactly one HDU");
    }
    check.primary().verify_datasum()?;
    let image = match &check.primary().data {
        HduData::Image(image) => image,
        _ => bail!("cropped output primary HDU is not an image"),
    };
    if image.axes != [rect.width, rect.height]
        || image.pixels.to_bytes()
            != crop_region_pixel_data(
                before,
                source_width,
                region,
                expected_blank,
                image_info.bitpix,
            )?
            .to_bytes()
    {
        bail!("cropped FITS shape or raw samples changed during writing");
    }
    if let CropRegion::Circle { .. } = region {
        if image_info.bitpix > 0 && check.primary().header.get_int("BLANK") != expected_blank {
            bail!("circular crop BLANK sentinel is missing or changed");
        }
        if check.primary().header.get_string("CLIPSHAP") != Some("CIRCLE") {
            bail!("circular crop geometry card is missing");
        }
    }
    for axis in 1..=2 {
        let prefix = format!("CRPIX{axis}");
        let old_header = &original.primary().header;
        let new_header = &check.primary().header;
        let offset = if axis == 1 { rect.x } else { rect.y } as f64;
        for card in old_header
            .iter()
            .filter(|card| is_crpix_keyword(&card.name, &prefix))
        {
            if let Some(value) = old_header.get_float(&card.name)
                && new_header
                    .get_float(&card.name)
                    .is_none_or(|actual| (actual - (value - offset)).abs() > 1e-10)
            {
                bail!(
                    "cropped output has incorrect {}: expected {}, got {:?}",
                    card.name,
                    value - offset,
                    new_header.get_float(&card.name)
                );
            }
        }
    }
    Ok(())
}

fn publish_fits(file: &FitsFile, destination: &Path) -> anyhow::Result<()> {
    let parent = destination
        .parent()
        .context("FITS destination has no parent")?;
    let temp = NamedTempFile::new_in(parent)?;
    let bytes = bytes_with_valid_checksum(file)?;
    fs::write(temp.path(), bytes)?;
    temp.as_file().sync_all()?;
    FitsFile::from_file(temp.path())
        .context("reopening generated FITS")?
        .primary()
        .verify_datasum()?;
    temp.persist(destination)
        .map_err(|e| e.error)
        .with_context(|| format!("publishing FITS {}", destination.display()))?;
    sync_parent(destination)?;
    Ok(())
}

/// Stamp checksums over the exact serialized HDU bytes, then verify the FITS
/// one's-complement invariant before any output is published.
fn bytes_with_valid_checksum(file: &FitsFile) -> anyhow::Result<Vec<u8>> {
    if file.len() != 1 {
        bail!("science and mask outputs must contain exactly one HDU");
    }
    let mut staged = file.clone();
    for hdu in &mut staged.hdus {
        hdu.header
            .keywords
            .retain(|card| !matches!(card.name.as_str(), "CHECKSUM" | "DATASUM"));
        let datasum = hdu.datasum();
        hdu.header.set(
            "DATASUM",
            HeaderValue::String(datasum.to_string()),
            Some("data unit checksum"),
        );
        hdu.header.set(
            "CHECKSUM",
            HeaderValue::String("0000000000000000".into()),
            Some("HDU checksum"),
        );
    }
    let mut bytes = staged.to_bytes()?;
    let mut cursor = Cursor::new(bytes.as_slice());
    let _header = Header::read_from(&mut cursor)?;
    let header_len = usize::try_from(cursor.position())?;
    let data_len = staged.primary().data_byte_len();
    let padded_data_len = data_len
        .checked_add(2879)
        .ok_or_else(|| anyhow::anyhow!("FITS data length overflow"))?
        / 2880
        * 2880;
    let hdu_end = header_len
        .checked_add(padded_data_len)
        .ok_or_else(|| anyhow::anyhow!("FITS output length overflow"))?;
    if hdu_end != bytes.len() {
        bail!("serialized FITS length does not match its header/data layout");
    }
    let card = bytes[..header_len]
        .chunks_exact(80)
        .position(|card| card.get(..8).is_some_and(|key| key == b"CHECKSUM"))
        .map(|index| index * 80)
        .context("serialized FITS has no CHECKSUM card")?;
    // FITS checksum cards use the conventional fixed comment column. The
    // generic parsed-header serializer places short-string comments earlier;
    // normalize this reserved card before calculating its checksum.
    bytes[card + 28..card + 80].fill(b' ');
    bytes[card + 31..card + 33].copy_from_slice(b"/ ");
    bytes[card + 33..card + 45].copy_from_slice(b"HDU checksum");
    let sum = fitskit::checksum::checksum(&bytes[..hdu_end]);
    let encoded = fitskit::checksum::encode_checksum(sum, true);
    if encoded.len() != 16 {
        bail!("FITS checksum encoder returned an invalid value");
    }
    bytes[card + 11..card + 27].copy_from_slice(encoded.as_bytes());
    let verified = fitskit::checksum::checksum(&bytes[..hdu_end]);
    if verified != 0xffff_ffff && verified != 0 {
        bail!("serialized FITS CHECKSUM invariant failed: {verified:#010x}");
    }
    Ok(bytes)
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
    backup.as_file().sync_all()?;
    sync_parent(path)?;
    Ok(Some(backup))
}

fn restore_backup(path: &Path, backup: Option<NamedTempFile>) -> anyhow::Result<()> {
    if let Some(backup) = backup {
        fs::rename(backup.path(), path)?;
    } else if path.exists() {
        fs::remove_file(path)?;
    }
    sync_parent(path)?;
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
    original: &FitsFile,
    path: &Path,
    mask: &[bool],
    expected_bitpix: i64,
    blank_value: Option<i64>,
) -> anyhow::Result<()> {
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

#[cfg(test)]
mod tests {
    use super::{publish_product_pair_with, write_products};
    use crate::fits_image::read_image;
    use fitskit::{FitsFile, HduData, HeaderValue, ImageData, PixelData};
    use sha2::{Digest, Sha256};
    use std::fs;

    fn fail_second_publish(
        staged_image: &std::path::Path,
        image: &std::path::Path,
        staged_mask: &std::path::Path,
        mask: &std::path::Path,
    ) -> anyhow::Result<()> {
        let mut calls = 0;
        publish_product_pair_with(staged_image, image, staged_mask, mask, |from, to| {
            calls += 1;
            if calls == 2 {
                return Err(std::io::Error::other("injected second-product failure"));
            }
            fs::rename(from, to)
        })
    }

    #[test]
    fn second_product_publication_failure_restores_previous_pair() {
        let temp = tempfile::tempdir().unwrap();
        let staging = temp.path().join("staging");
        fs::create_dir(&staging).unwrap();
        let staged_image = staging.join("science.fits");
        let staged_mask = staging.join("mask.fits");
        fs::write(&staged_image, b"new science").unwrap();
        fs::write(&staged_mask, b"new mask").unwrap();
        let image = temp.path().join("science.fits");
        let mask = temp.path().join("mask.fits");
        fs::write(&image, b"old science").unwrap();
        fs::write(&mask, b"old mask").unwrap();

        assert!(fail_second_publish(&staged_image, &image, &staged_mask, &mask).is_err());
        assert_eq!(fs::read(image).unwrap(), b"old science");
        assert_eq!(fs::read(mask).unwrap(), b"old mask");
    }

    #[test]
    fn second_product_publication_failure_removes_partial_new_pair() {
        let temp = tempfile::tempdir().unwrap();
        let staging = temp.path().join("staging");
        fs::create_dir(&staging).unwrap();
        let staged_image = staging.join("science.fits");
        let staged_mask = staging.join("mask.fits");
        fs::write(&staged_image, b"new science").unwrap();
        fs::write(&staged_mask, b"new mask").unwrap();
        let image = temp.path().join("science.fits");
        let mask = temp.path().join("mask.fits");

        assert!(fail_second_publish(&staged_image, &image, &staged_mask, &mask).is_err());
        assert!(!image.exists());
        assert!(!mask.exists());
    }

    #[test]
    fn existing_blank_is_reused_without_searching_for_another_integer_code() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.fits");
        let output = temp.path().join("masked.fits");
        let mask_path = temp.path().join("mask.fits");
        let (width, height) = (96, 96);
        let mut values = vec![100_u8; width * height];
        for y in 0..16 {
            for x in 0..32 {
                values[y * width + x] = 0;
            }
        }
        for value in 1..=254_u8 {
            values[48 * width + 32 + usize::from(value)] = value;
        }
        values[48 * width + 32 + 255] = 255;
        let mut fits = FitsFile::with_primary_image(ImageData::new(
            vec![width, height],
            PixelData::U8(values),
        ));
        fits.primary_mut()
            .header
            .set("BLANK", HeaderValue::Integer(255), None);
        fits.to_file(&source).unwrap();

        let image = read_image(&source, 16).unwrap().unwrap();
        let mut mask = vec![false; width * height];
        for y in 0..16 {
            for x in 0..32 {
                mask[y * width + x] = true;
            }
        }
        write_products(&output, &mask_path, &image, &mask).unwrap();
        assert_eq!(
            FitsFile::from_file(&output)
                .unwrap()
                .primary()
                .header
                .get_int("BLANK"),
            Some(255)
        );
        assert!(fs::metadata(&mask_path).unwrap().len() > 0);
    }

    #[test]
    fn output_uses_the_same_parsed_source_snapshot_as_detection() {
        let t = tempfile::tempdir().unwrap();
        let source = t.path().join("source.fits");
        let output = t.path().join("processed.fits");
        let mask_path = t.path().join("mask.fits");
        let (width, height) = (96, 96);
        let mut original = vec![100_i16; width * height];
        for y in 0..16 {
            for x in 0..32 {
                original[y * width + x] = 0;
            }
        }
        let mut original_file = FitsFile::with_primary_image(ImageData::new(
            vec![width, height],
            PixelData::I16(original.clone()),
        ));
        original_file.primary_mut().header.set(
            "OBJECT",
            HeaderValue::String("snapshot".into()),
            None,
        );
        original_file.to_file(&source).unwrap();
        let expected_source_sha256 = format!("{:x}", Sha256::digest(fs::read(&source).unwrap()));
        let image = read_image(&source, 16).unwrap().unwrap();
        assert_eq!(image.source_sha256, expected_source_sha256);

        let mut replacement = FitsFile::with_primary_image(ImageData::new(
            vec![width, height],
            PixelData::I16(vec![250; width * height]),
        ));
        replacement.primary_mut().header.set(
            "OBJECT",
            HeaderValue::String("replacement".into()),
            None,
        );
        replacement.to_file(&source).unwrap();

        let mut mask = vec![false; width * height];
        for y in 0..16 {
            for x in 0..32 {
                mask[y * width + x] = true;
            }
        }
        write_products(&output, &mask_path, &image, &mask).unwrap();

        let written = FitsFile::from_file(output).unwrap();
        assert_eq!(
            written.primary().header.get_string("OBJECT"),
            Some("snapshot")
        );
        let HduData::Image(written_image) = &written.primary().data else {
            panic!("expected primary image")
        };
        let PixelData::I16(written_pixels) = &written_image.pixels else {
            panic!("expected int16 samples")
        };
        assert_eq!(written_pixels[16 * width + 32], original[16 * width + 32]);
        assert_ne!(written_pixels[16 * width + 32], 250);
        assert!(fs::metadata(mask_path).unwrap().len() > 0);
    }
}
