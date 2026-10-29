//! FITS products written with the pure-Rust fitskit crate.
use crate::fits_image::{Image, RawHeaderCard, read_fits_snapshot, unused_integer_value};
use crate::{crop::CropRect, durability::sync_parent};
use anyhow::{Context, bail};
use chrono::{SecondsFormat, Utc};
use fitskit::{FitsFile, HduData, Header, HeaderValue, Keyword, PixelData};
use rayon::prelude::*;
use std::{fs, io::Cursor, path::Path};
use tempfile::{NamedTempFile, TempDir};

mod reason_mask_writer;
pub use reason_mask_writer::write_reason_mask;

/// Write the science image by replacing rejected samples with FITS undefined values.
pub fn write_masked_image(
    source: &FitsFile,
    destination: &Path,
    mask: &[bool],
    bitpix: i64,
    blank_value: Option<i64>,
    raw_header_cards: &[RawHeaderCard],
) -> anyhow::Result<()> {
    let mut output = source.clone();
    if output.len() != 1 {
        bail!("source changed: expected exactly one HDU");
    }
    let hdu = output.primary_mut();
    normalize_history_cards(&mut hdu.header);
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
    let written = publish_fits_with_raw_header_cards(output, destination, raw_header_cards)?;
    verify_masked_file(source, &written, mask, bitpix, blank_value)
}

/// Publish the dimension-preserving science product for `data_mask`.
///
/// The product is written and verified against the parsed source snapshot in
/// a sibling staging directory, then renamed atomically into place. The binary
/// action mask is checked in memory by that verification (every masked sample
/// undefined, every retained sample unchanged); no mask sidecar file is written
/// (plan R0.5).
pub fn write_masked_product(
    image_path: &Path,
    image: &Image,
    data_mask: &[bool],
) -> anyhow::Result<()> {
    if data_mask.len() != image.width.saturating_mul(image.height) {
        bail!("action mask dimensions do not match the image");
    }
    let blank_value = if image.bitpix > 0 && data_mask.iter().any(|masked| *masked) {
        Some(match image.blank_value {
            Some(blank) => blank,
            None => choose_mask_blank(&image.source_fits, image.bitpix, data_mask)?,
        })
    } else {
        image.blank_value
    };
    publish_single_product(image_path, |staged_image| {
        write_masked_image(
            &image.source_fits,
            staged_image,
            data_mask,
            image.bitpix,
            blank_value,
            &image.raw_header_cards,
        )
    })
}

/// Publish a verified rectangular crop of the parsed source snapshot.
pub fn write_cropped_product(
    image_path: &Path,
    image: &Image,
    region: CropRect,
) -> anyhow::Result<()> {
    publish_single_product(image_path, |staged_image| {
        write_cropped_image(staged_image, image, region, None)
    })
}

/// Publish a rectangular crop whose samples flagged in the full-frame
/// `interior_mask` are set undefined (BLANK or NaN). Used by `crop_masked`
/// (decision D2) and the `bbox_mask` fallback (decision D1). Falls back to a
/// plain crop when no flagged sample lies inside `region`.
pub fn write_cropped_masked_product(
    image_path: &Path,
    image: &Image,
    region: CropRect,
    interior_mask: &[bool],
) -> anyhow::Result<()> {
    if interior_mask.len() != image.width.saturating_mul(image.height) {
        bail!("interior mask dimensions do not match the image");
    }
    let rect = region.bounds();
    let inside_flagged = (rect.y..rect.y + rect.height)
        .any(|y| (rect.x..rect.x + rect.width).any(|x| interior_mask[y * image.width + x]));
    if !inside_flagged {
        return write_cropped_product(image_path, image, region);
    }
    let blank_value = if image.bitpix > 0 {
        Some(match image.blank_value {
            Some(blank) => blank,
            None => {
                // A new code must be absent from every retained stored sample.
                let excluded: Vec<bool> = (0..interior_mask.len())
                    .map(|i| !region.retains(i % image.width, i / image.width) || interior_mask[i])
                    .collect();
                choose_mask_blank(&image.source_fits, image.bitpix, &excluded)?
            }
        })
    } else {
        None
    };
    publish_single_product(image_path, |staged_image| {
        write_cropped_image(
            staged_image,
            image,
            region,
            Some(CropInterior {
                mask: interior_mask,
                blank: blank_value,
            }),
        )
    })
}

/// Samples inside a crop to be marked undefined, with the integer sentinel.
#[derive(Clone, Copy)]
pub struct CropInterior<'a> {
    pub mask: &'a [bool],
    pub blank: Option<i64>,
}

/// Mark cropped samples undefined where the full-frame mask is set.
fn apply_crop_interior(
    pixels: &mut PixelData,
    source_width: usize,
    rect: CropRect,
    interior: CropInterior<'_>,
) -> anyhow::Result<usize> {
    let mut count = 0usize;
    macro_rules! mark {
        ($values:expr, $replacement:expr) => {{
            for cy in 0..rect.height {
                for cx in 0..rect.width {
                    if interior.mask[(rect.y + cy) * source_width + rect.x + cx] {
                        $values[cy * rect.width + cx] = $replacement;
                        count += 1;
                    }
                }
            }
        }};
    }
    let blank = interior.blank;
    match pixels {
        PixelData::U8(v) => {
            let b =
                u8::try_from(blank.context("missing BLANK")?).context("BLANK exceeds BITPIX=8")?;
            mark!(v, b)
        }
        PixelData::I16(v) => {
            let b = i16::try_from(blank.context("missing BLANK")?)
                .context("BLANK exceeds BITPIX=16")?;
            mark!(v, b)
        }
        PixelData::I32(v) => {
            let b = i32::try_from(blank.context("missing BLANK")?)
                .context("BLANK exceeds BITPIX=32")?;
            mark!(v, b)
        }
        PixelData::I64(v) => {
            let b = blank.context("missing BLANK")?;
            mark!(v, b)
        }
        PixelData::F32(v) => mark!(v, f32::NAN),
        PixelData::F64(v) => mark!(v, f64::NAN),
    }
    Ok(count)
}

/// Write and verify a product in a sibling staging directory, then atomically
/// rename it over `destination`. A symlinked destination is refused so a kept
/// source link is never written through. On any failure the destination is
/// unchanged and the staging directory is removed.
fn publish_single_product(
    destination: &Path,
    write_and_verify: impl FnOnce(&Path) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    if fs::symlink_metadata(destination).is_ok_and(|meta| meta.file_type().is_symlink()) {
        bail!(
            "refusing to replace symlink output {}",
            destination.display()
        );
    }
    let stage = create_stage_directory(destination)?;
    let staged = stage.path().join("product.fits");
    write_and_verify(&staged)?;
    fs::rename(&staged, destination)
        .with_context(|| format!("publishing FITS {}", destination.display()))?;
    sync_parent(destination)?;
    Ok(())
}

fn create_stage_directory(destination: &Path) -> anyhow::Result<TempDir> {
    let parent = destination
        .parent()
        .context("FITS destination has no parent")?;
    tempfile::tempdir_in(parent)
        .with_context(|| format!("creating product staging directory in {}", parent.display()))
}

/// Crop raw stored values, preserve non-structural cards and shift all primary/alternate CRPIX.
pub fn write_cropped_image(
    destination: &Path,
    image_info: &Image,
    region: CropRect,
    interior: Option<CropInterior<'_>>,
) -> anyhow::Result<()> {
    let mut output = image_info.source_fits.clone();
    if output.len() != 1 {
        bail!("source changed: expected exactly one HDU");
    }
    let hdu = output.primary_mut();
    normalize_history_cards(&mut hdu.header);
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
    let source_len = image.pixels.len();
    image.pixels = crop_pixel_data(&image.pixels, width, rect);
    image.axes = vec![rect.width, rect.height];
    let interior_count = match interior {
        Some(interior) => {
            if interior.mask.len() != source_len {
                bail!("interior mask dimensions do not match the source");
            }
            apply_crop_interior(&mut image.pixels, width, rect, interior)?
        }
        None => 0,
    };
    let expected_pixels = image.pixels.clone();
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
    let interior_blank = interior.and_then(|interior| interior.blank);
    if interior_count > 0
        && let Some(blank) = interior_blank
    {
        hdu.header.set(
            "BLANK",
            HeaderValue::Integer(blank),
            Some("undefined pixel value"),
        );
    }
    remove_data_extrema(&mut hdu.header);
    if rect.x == 0 && rect.y == 0 && rect.width == width && rect.height == height {
        bail!("a full-frame rectangle is not a crop; it must be written as a mask product");
    }
    stamp_crop_provenance(
        &mut hdu.header,
        rect,
        (width, height),
        interior_count,
        &image_info.source_sha256,
        image_info.source_file_name.as_deref(),
    );
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
    stamp_processing_metadata(&mut hdu.header);
    let history = if interior_count > 0 {
        format!(
            "rust_fits_auto_cropping: selected axis-aligned rectangle; {interior_count} detector-flagged samples inside it set undefined"
        )
    } else {
        "rust_fits_auto_cropping: selected axis-aligned rectangle".to_owned()
    };
    push_history(&mut hdu.header, &history);
    let written =
        publish_fits_with_raw_header_cards(output, destination, &image_info.raw_header_cards)?;
    verify_cropped_file(
        &image_info.source_fits,
        &written,
        &expected_pixels,
        region,
        (interior_count > 0).then_some(interior_blank).flatten(),
    )
}

/// Crop provenance, written only on products smaller than their source. It
/// identifies the product as cropped and gives everything needed to apply the
/// same crop to matching calibration frames (`tools/apply_crop.py`):
/// CROPSEC in IRAF/FITS section syntax (1-based, inclusive, original pixel
/// coordinates), the original dimensions, and the IRAF image-to-physical
/// offset LTV (image = physical + LTV), so physical coordinates stay those of
/// the original frame.
fn stamp_crop_provenance(
    header: &mut Header,
    rect: CropRect,
    (source_width, source_height): (usize, usize),
    interior_undefined: usize,
    source_sha256: &str,
    source_file_name: Option<&str>,
) {
    // A source that is itself a cropped product: express the new section in
    // the coordinates of the original frame and keep the original identity.
    let earlier = earlier_crop(header);
    let (section, (original_width, original_height)) = match &earlier {
        Some(prior) => (prior.compose(rect), prior.original_size),
        None => (rect, (source_width, source_height)),
    };
    header.set(
        "CROPPED",
        HeaderValue::Logical(true),
        Some("image was cropped; see CROPSEC"),
    );
    header.set(
        "CROPSEC",
        HeaderValue::String(crop_section(section)),
        Some("kept section of original image (1-based)"),
    );
    header.set(
        "ORIGNAX1",
        HeaderValue::Integer(original_width as i64),
        Some("NAXIS1 of the original image"),
    );
    header.set(
        "ORIGNAX2",
        HeaderValue::Integer(original_height as i64),
        Some("NAXIS2 of the original image"),
    );
    for (key, offset) in [("LTV1", rect.x), ("LTV2", rect.y)] {
        let previous = header.get_float(key).unwrap_or(0.0);
        header.set(
            key,
            HeaderValue::Float(previous - offset as f64),
            Some("image = physical + LTV (IRAF)"),
        );
    }
    header.set(
        "CROPUNDF",
        HeaderValue::Integer(interior_undefined as i64),
        Some("samples inside the crop set undefined"),
    );
    if earlier.is_some() {
        // ORIGSHA/ORIGFILE already identify the original frame.
        return;
    }
    // 64 hex digits fill the card; no room for a comment.
    header.set(
        "ORIGSHA",
        HeaderValue::String(source_sha256.to_owned()),
        None,
    );
    // Long or unusual names are omitted rather than truncated or escaped.
    if let Some(name) = source_file_name.filter(|name| {
        name.len() <= 60
            && name
                .bytes()
                .all(|b| (0x20..0x7f).contains(&b) && b != b'\'')
    }) {
        header.set("ORIGFILE", HeaderValue::String(name.to_owned()), None);
    }
}

/// Check the crop provenance cards of a written product against the source
/// snapshot and the selected rectangle.
fn verify_crop_provenance(
    original: &FitsFile,
    check: &FitsFile,
    rect: CropRect,
) -> anyhow::Result<()> {
    let old = &original.primary().header;
    let new = &check.primary().header;
    let (width, height) = match &original.primary().data {
        HduData::Image(image) if image.axes.len() == 2 => (image.axes[0], image.axes[1]),
        _ => bail!("source primary HDU is not a 2D image"),
    };
    let ltv_ok = |key: &str, offset: usize| {
        let expected = old.get_float(key).unwrap_or(0.0) - offset as f64;
        new.get_float(key)
            .is_some_and(|v| (v - expected).abs() <= 1e-10)
    };
    let earlier = earlier_crop(old);
    let (section, (original_width, original_height)) = match &earlier {
        Some(prior) => (prior.compose(rect), prior.original_size),
        None => (rect, (width, height)),
    };
    if new.get_bool("CROPPED") != Some(true)
        || new.get_string("CROPSEC") != Some(crop_section(section).as_str())
        || new.get_int("ORIGNAX1") != Some(original_width as i64)
        || new.get_int("ORIGNAX2") != Some(original_height as i64)
        || !ltv_ok("LTV1", rect.x)
        || !ltv_ok("LTV2", rect.y)
        || new.get_string("ORIGSHA").is_none_or(|sha| sha.len() != 64)
    {
        bail!("cropped FITS crop provenance cards are missing or inconsistent");
    }
    Ok(())
}

/// Crop recorded in a source header that is itself a cropped product.
struct EarlierCrop {
    x0: usize,
    y0: usize,
    original_size: (usize, usize),
}

impl EarlierCrop {
    fn compose(&self, rect: CropRect) -> CropRect {
        CropRect {
            x: self.x0 + rect.x,
            y: self.y0 + rect.y,
            ..rect
        }
    }
}

/// Parse `CROPPED = T`, `CROPSEC` and `ORIGNAX1/2` from a source header.
fn earlier_crop(header: &Header) -> Option<EarlierCrop> {
    if header.get_bool("CROPPED") != Some(true) {
        return None;
    }
    let section = parse_section(header.get_string("CROPSEC")?)?;
    let width = usize::try_from(header.get_int("ORIGNAX1")?).ok()?;
    let height = usize::try_from(header.get_int("ORIGNAX2")?).ok()?;
    Some(EarlierCrop {
        x0: section.x,
        y0: section.y,
        original_size: (width, height),
    })
}

/// Parse a 1-based inclusive `[x1:x2,y1:y2]` section into a zero-based rectangle.
fn parse_section(text: &str) -> Option<CropRect> {
    let inner = text.trim().strip_prefix('[')?.strip_suffix(']')?;
    let (xs, ys) = inner.split_once(',')?;
    let range = |part: &str| -> Option<(usize, usize)> {
        let (a, b) = part.split_once(':')?;
        let (a, b): (usize, usize) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
        (a >= 1 && b >= a).then(|| (a - 1, b - a + 1))
    };
    let ((x, width), (y, height)) = (range(xs)?, range(ys)?);
    Some(CropRect {
        x,
        y,
        width,
        height,
    })
}

/// IRAF/FITS image section of a zero-based rectangle: `[x1:x2,y1:y2]`,
/// 1-based and inclusive.
pub(crate) fn crop_section(rect: CropRect) -> String {
    format!(
        "[{}:{},{}:{}]",
        rect.x + 1,
        rect.x + rect.width,
        rect.y + 1,
        rect.y + rect.height
    )
}

pub(super) fn stamp_processing_metadata(header: &mut Header) {
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

fn remove_data_extrema(header: &mut Header) {
    header
        .keywords
        .retain(|card| !matches!(card.name.as_str(), "DATAMIN" | "DATAMAX"));
}

pub(super) fn push_history(header: &mut Header, text: &str) {
    header.keywords.extend(history_cards(text));
}

/// Convert value-style HISTORY cards to FITS commentary cards before output.
/// Some legacy inputs encode HISTORY as quoted string keywords; the generic
/// serializer would turn long values into OGIP CONTINUE cards. Converting the
/// same text to standard commentary avoids adding nonstandard long strings.
pub(super) fn normalize_history_cards(header: &mut Header) {
    let previous = std::mem::take(&mut header.keywords);
    let mut normalized = Vec::with_capacity(previous.len());
    for keyword in previous {
        if keyword.name == "HISTORY"
            && let Some(value) = keyword.value
        {
            let mut text = match value {
                HeaderValue::String(value) => value,
                other => other.to_string(),
            };
            if let Some(comment) = keyword.comment {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(&comment);
            }
            normalized.extend(history_cards(&text));
        } else {
            normalized.push(keyword);
        }
    }
    header.keywords = normalized;
}

fn history_cards(text: &str) -> Vec<Keyword> {
    const CARD_TEXT_BYTES: usize = 60;
    let mut cards = Vec::new();
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
        cards.push(Keyword::commentary("HISTORY", &remaining[..end]));
        remaining = remaining[end..].trim_start();
    }
    cards
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

/// `check` is the snapshot re-read from disk (DATASUM already verified).
fn verify_cropped_file(
    original: &FitsFile,
    check: &FitsFile,
    expected: &PixelData,
    region: CropRect,
    interior_blank: Option<i64>,
) -> anyhow::Result<()> {
    let rect = region.bounds();
    if check.len() != 1 {
        bail!("cropped output does not contain exactly one HDU");
    }
    let image = match &check.primary().data {
        HduData::Image(image) => image,
        _ => bail!("cropped output primary HDU is not an image"),
    };
    if image.axes != [rect.width, rect.height] || image.pixels.to_bytes() != expected.to_bytes() {
        bail!("cropped FITS shape or raw samples changed during writing");
    }
    if let Some(blank) = interior_blank
        && check.primary().header.get_int("BLANK") != Some(blank)
    {
        bail!("cropped FITS BLANK card does not match the interior sentinel");
    }
    verify_crop_provenance(original, check, rect)?;
    let header = &check.primary().header;
    if header.get_string("CLIPSHAP") != Some("RECTANGLE")
        || header.get_int("NAXIS") != Some(2)
        || header.get_int("NAXIS1") != Some(rect.width as i64)
        || header.get_int("NAXIS2") != Some(rect.height as i64)
    {
        bail!("cropped FITS geometry or NAXIS cards do not match the selected rectangle");
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

pub(super) fn publish_fits(file: &FitsFile, destination: &Path) -> anyhow::Result<()> {
    publish_fits_with_raw_header_cards(file.clone(), destination, &[]).map(|_| ())
}

/// Serialize with valid checksums, write and fsync, then re-read the written
/// file once from disk and verify its DATASUM. Returns that on-disk snapshot,
/// which callers use for their sample and header checks.
fn publish_fits_with_raw_header_cards(
    file: FitsFile,
    destination: &Path,
    raw_header_cards: &[RawHeaderCard],
) -> anyhow::Result<FitsFile> {
    let parent = destination
        .parent()
        .context("FITS destination has no parent")?;
    let temp = NamedTempFile::new_in(parent)?;
    let bytes = bytes_with_valid_checksum_and_raw_cards(file, raw_header_cards)?;
    fs::write(temp.path(), bytes)?;
    temp.as_file().sync_all()?;
    let written = read_fits_snapshot(temp.path())?;
    written.primary().verify_datasum()?;
    temp.persist(destination)
        .map_err(|e| e.error)
        .with_context(|| format!("publishing FITS {}", destination.display()))?;
    sync_parent(destination)?;
    Ok(written)
}

fn bytes_with_valid_checksum_and_raw_cards(
    file: FitsFile,
    raw_header_cards: &[RawHeaderCard],
) -> anyhow::Result<Vec<u8>> {
    let mut bytes = bytes_with_valid_checksum(file)?;
    if raw_header_cards.is_empty() {
        return Ok(bytes);
    }

    let mut cursor = Cursor::new(bytes.as_slice());
    let _header = Header::read_from(&mut cursor)?;
    let header_len = usize::try_from(cursor.position())?;
    let check_card = bytes[..header_len]
        .chunks_exact(80)
        .position(|card| card.get(..8).is_some_and(|key| key == b"CHECKSUM"))
        .map(|index| index * 80)
        .context("serialized FITS has no CHECKSUM card")?;

    for raw_card in raw_header_cards {
        let marker_position = bytes[..header_len]
            .chunks_exact(80)
            .position(|card| card == raw_card.replacement)
            .map(|index| index * 80)
            .context("serialized FITS lost a raw source-header card marker")?;
        bytes[marker_position..marker_position + 80].copy_from_slice(&raw_card.original);
    }

    // Pixel bytes are unchanged, so DATASUM is valid. Restore the CHECKSUM
    // value over the final header that retains the exact original cards.
    bytes[check_card + 11..check_card + 27].fill(b'0');
    let sum = fitskit::checksum::checksum(&bytes);
    let encoded = fitskit::checksum::encode_checksum(sum, true);
    bytes[check_card + 11..check_card + 27].copy_from_slice(encoded.as_bytes());
    let verified = fitskit::checksum::checksum(&bytes);
    if verified != 0xffff_ffff && verified != 0 {
        bail!("raw-header FITS CHECKSUM invariant failed: {verified:#010x}");
    }
    Ok(bytes)
}

/// Stamp checksums over the exact serialized HDU bytes, then verify the FITS
/// one's-complement invariant before any output is published.
fn bytes_with_valid_checksum(mut staged: FitsFile) -> anyhow::Result<Vec<u8>> {
    if staged.len() != 1 {
        bail!("science and mask outputs must contain exactly one HDU");
    }
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

pub(super) fn is_structural_or_data_summary(key: &str) -> bool {
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

/// First index where `bad(written, source, masked)` holds, searched in
/// parallel; `position_first` keeps the reported index deterministic.
fn first_violation<T: Copy + Sync>(
    after: &[T],
    before: &[T],
    mask: &[bool],
    bad: impl Fn(T, T, bool) -> bool + Sync,
) -> Option<usize> {
    after
        .par_iter()
        .zip(before)
        .zip(mask)
        .position_first(|((got, old), masked)| bad(*got, *old, *masked))
}

/// `check` is the snapshot re-read from disk (DATASUM already verified).
/// Samples are compared in their stored type: widening to i64/f64 preserves
/// equality and NaN exactly, so this is the same invariant as before.
fn verify_masked_file(
    original: &FitsFile,
    check: &FitsFile,
    mask: &[bool],
    expected_bitpix: i64,
    blank_value: Option<i64>,
) -> anyhow::Result<()> {
    if check.len() != 1 {
        bail!("written science output does not contain exactly one HDU");
    }
    let before = match &original.primary().data {
        HduData::Image(img) => &img.pixels,
        _ => bail!("source primary HDU is not an image"),
    };
    let after_image = match &check.primary().data {
        HduData::Image(img) => img,
        _ => bail!("output primary HDU is not an image"),
    };
    if after_image.bitpix().to_i64() != expected_bitpix
        || after_image.pixels.len() != mask.len()
        || before.len() != mask.len()
    {
        bail!("output BITPIX or dimensions changed");
    }
    let violation = if expected_bitpix > 0 {
        let blank = blank_value.context("integer output has no BLANK sentinel")?;
        if check.primary().header.get_int("BLANK") != Some(blank) {
            bail!("output BLANK keyword is missing or changed");
        }
        macro_rules! check_integer {
            ($after:expr, $before:expr) => {
                first_violation($after, $before, mask, |got, old, masked| {
                    (masked && i64::from(got) != blank) || (!masked && got != old)
                })
            };
        }
        let found = match (&after_image.pixels, before) {
            (PixelData::U8(a), PixelData::U8(b)) => check_integer!(a, b),
            (PixelData::I16(a), PixelData::I16(b)) => check_integer!(a, b),
            (PixelData::I32(a), PixelData::I32(b)) => check_integer!(a, b),
            (PixelData::I64(a), PixelData::I64(b)) => check_integer!(a, b),
            _ => bail!("expected matching integer FITS storage"),
        };
        found.map(|i| format!("integer output pixel {i} violates masked/retained-value invariant"))
    } else {
        macro_rules! check_float {
            ($after:expr, $before:expr) => {
                first_violation($after, $before, mask, |got, old, masked| {
                    (masked && !got.is_nan())
                        || (!masked && got != old && !(got.is_nan() && old.is_nan()))
                })
            };
        }
        let found = match (&after_image.pixels, before) {
            (PixelData::F32(a), PixelData::F32(b)) => check_float!(a, b),
            (PixelData::F64(a), PixelData::F64(b)) => check_float!(a, b),
            _ => bail!("expected matching floating FITS storage"),
        };
        found.map(|i| format!("floating output pixel {i} violates masked/retained-value invariant"))
    };
    if let Some(message) = violation {
        bail!(message);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        crop_section, parse_section, publish_single_product, stamp_crop_provenance,
        write_masked_product, write_reason_mask,
    };
    use crate::crop::CropRect;
    use crate::fits_image::read_image;
    use fitskit::{FitsFile, HduData, HeaderValue, ImageData, Keyword, PixelData};
    use sha2::{Digest, Sha256};
    use std::fs;

    #[test]
    fn crop_section_round_trips_and_recrops_compose_on_the_original() {
        let rect = CropRect {
            x: 10,
            y: 0,
            width: 90,
            height: 40,
        };
        assert_eq!(crop_section(rect), "[11:100,1:40]");
        assert_eq!(parse_section("[11:100,1:40]"), Some(rect));
        assert_eq!(parse_section("[0:5,1:2]"), None);
        assert_eq!(parse_section("[5:4,1:2]"), None);

        let mut header = fitskit::Header::new();
        header.set("LTV1", HeaderValue::Float(-3.0), None);
        stamp_crop_provenance(
            &mut header,
            rect,
            (200, 50),
            7,
            &"a".repeat(64),
            Some("raw.fits"),
        );
        assert_eq!(header.get_bool("CROPPED"), Some(true));
        assert_eq!(header.get_float("LTV1"), Some(-13.0));
        assert_eq!(header.get_float("LTV2"), Some(0.0));
        assert_eq!(header.get_int("CROPUNDF"), Some(7));
        assert_eq!(header.get_string("ORIGFILE"), Some("raw.fits"));

        // Cropping that product again: section and size refer to the original.
        let again = CropRect {
            x: 5,
            y: 2,
            width: 20,
            height: 30,
        };
        stamp_crop_provenance(
            &mut header,
            again,
            (90, 40),
            0,
            &"b".repeat(64),
            Some("x.fits"),
        );
        assert_eq!(header.get_string("CROPSEC"), Some("[16:35,3:32]"));
        assert_eq!(header.get_int("ORIGNAX1"), Some(200));
        assert_eq!(header.get_int("ORIGNAX2"), Some(50));
        assert_eq!(header.get_float("LTV1"), Some(-18.0));
        assert_eq!(header.get_string("ORIGSHA"), Some("a".repeat(64).as_str()));
        assert_eq!(header.get_string("ORIGFILE"), Some("raw.fits"));

        let mut long = fitskit::Header::new();
        stamp_crop_provenance(
            &mut long,
            rect,
            (200, 50),
            0,
            &"c".repeat(64),
            Some(&"n".repeat(61)),
        );
        assert!(long.find("ORIGFILE").is_none());
    }

    #[test]
    fn failed_staged_verification_leaves_existing_product_and_no_stage() {
        let temp = tempfile::tempdir().unwrap();
        let product = temp.path().join("science.fits");
        fs::write(&product, b"previous product").unwrap();
        let result = publish_single_product(&product, |staged| {
            fs::write(staged, b"unverified").unwrap();
            anyhow::bail!("injected verification failure")
        });
        assert!(result.is_err());
        assert_eq!(fs::read(&product).unwrap(), b"previous product");
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_destination_is_never_written_through() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("source.fits");
        fs::write(&target, b"source bytes").unwrap();
        let link = temp.path().join("science.fits");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(publish_single_product(&link, |staged| Ok(fs::write(staged, b"new")?)).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"source bytes");
    }

    #[test]
    fn reason_plane_normalizes_legacy_value_history_without_continue_cards() {
        let temp = tempfile::tempdir().unwrap();
        let reason = temp.path().join("reason.fits");
        let history =
            "legacy generated history record with enough text to require an OGIP continuation card";
        let mut fits = FitsFile::with_primary_image(ImageData::new(
            vec![2, 2],
            PixelData::F64(vec![1.0, 2.0, 3.0, 4.0]),
        ));
        fits.primary_mut().header.push(Keyword::with_value(
            "HISTORY",
            HeaderValue::String(history.into()),
            None,
        ));
        write_reason_mask(&fits.primary().header, &reason, 5, 1, &[0, 1, 2, 3, 4]).unwrap();

        let bytes = fs::read(&reason).unwrap();
        assert!(!bytes.windows(8).any(|window| window == b"CONTINUE"));
        let output = FitsFile::from_file(reason).unwrap();
        let history_text = output
            .primary()
            .header
            .iter()
            .filter(|keyword| keyword.name == "HISTORY")
            .filter_map(|keyword| keyword.comment.as_deref())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(history_text.contains(history));
        output.primary().verify_datasum().unwrap();
    }

    #[test]
    fn existing_blank_is_reused_without_searching_for_another_integer_code() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.fits");
        let output = temp.path().join("masked.fits");
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
        write_masked_product(&output, &image, &mask).unwrap();
        assert_eq!(
            FitsFile::from_file(&output)
                .unwrap()
                .primary()
                .header
                .get_int("BLANK"),
            Some(255)
        );
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 2);
    }

    #[test]
    fn output_uses_the_same_parsed_source_snapshot_as_detection() {
        let t = tempfile::tempdir().unwrap();
        let source = t.path().join("source.fits");
        let output = t.path().join("processed.fits");
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
        write_masked_product(&output, &image, &mask).unwrap();

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
    }
}
