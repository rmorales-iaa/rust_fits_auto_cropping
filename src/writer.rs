//! FITS products. Science products copy the source primary header card for
//! card and add MO_ provenance (see `product_header`); the transient reason
//! plane is serialized with fitskit.
use crate::fits_image::{Image, read_fits_snapshot, read_fits_snapshot_with_cards};
use crate::{crop::CropRect, durability::sync_parent};
use anyhow::{Context, bail};
use chrono::{SecondsFormat, Utc};
use fitskit::{FitsFile, HduData, Header, HeaderValue, Keyword, PixelData};
use rayon::prelude::*;
use std::{fs, io::Cursor, path::Path};
use tempfile::{NamedTempFile, TempDir};

pub mod product_header;
mod reason_mask_writer;
use product_header::{HeaderEdits, Provenance};
pub use reason_mask_writer::write_reason_mask;

/// Product action and detector identity recorded in MO_ACT / MO_ALGO.
#[derive(Clone, Copy)]
pub struct ProductContext<'a> {
    pub action: &'a str,
    pub algorithm: &'a str,
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
    context: ProductContext<'_>,
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
    let full = CropRect {
        x: 0,
        y: 0,
        width: image.width,
        height: image.height,
    };
    publish_single_product(image_path, |staged| {
        write_region_product(
            staged,
            image,
            full,
            false,
            Some(data_mask),
            blank_value,
            context,
        )
    })
}

/// Publish a verified rectangular crop of the parsed source snapshot.
pub fn write_cropped_product(
    image_path: &Path,
    image: &Image,
    region: CropRect,
    context: ProductContext<'_>,
) -> anyhow::Result<()> {
    publish_single_product(image_path, |staged| {
        write_region_product(staged, image, region.bounds(), true, None, None, context)
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
    context: ProductContext<'_>,
) -> anyhow::Result<()> {
    if interior_mask.len() != image.width.saturating_mul(image.height) {
        bail!("interior mask dimensions do not match the image");
    }
    let rect = region.bounds();
    let inside_flagged = (rect.y..rect.y + rect.height)
        .any(|y| (rect.x..rect.x + rect.width).any(|x| interior_mask[y * image.width + x]));
    if !inside_flagged {
        return write_cropped_product(image_path, image, region, context);
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
    publish_single_product(image_path, |staged| {
        write_region_product(
            staged,
            image,
            rect,
            true,
            Some(interior_mask),
            blank_value,
            context,
        )
    })
}

fn source_pixels(image: &Image) -> anyhow::Result<&PixelData> {
    if image.source_fits.len() != 1 {
        bail!("source changed: expected exactly one HDU");
    }
    match &image.source_fits.primary().data {
        HduData::Image(data)
            if data.axes == [image.width, image.height]
                && data.bitpix().to_i64() == image.bitpix =>
        {
            Ok(&data.pixels)
        }
        HduData::Image(_) => bail!("source image dimensions or BITPIX changed during processing"),
        _ => bail!("source primary HDU is not an image"),
    }
}

/// Big-endian samples of `rect`, with full-frame `undefined` samples written
/// as BLANK (integer) or NaN (floating). Returns the bytes and the number of
/// samples set undefined. Rows are encoded in parallel into fixed slots.
fn encode_region(
    pixels: &PixelData,
    source_width: usize,
    rect: CropRect,
    undefined: Option<&[bool]>,
    blank: Option<i64>,
) -> anyhow::Result<(Vec<u8>, usize)> {
    macro_rules! encode {
        ($values:expr, $t:ty, $replacement:expr) => {{
            const BYTES: usize = std::mem::size_of::<$t>();
            let replacement: Option<$t> = $replacement;
            let mut out = vec![0u8; rect.area() * BYTES];
            let count: usize = out
                .par_chunks_mut(rect.width * BYTES)
                .enumerate()
                .map(|(cy, row)| {
                    let mut count = 0usize;
                    let start = (rect.y + cy) * source_width + rect.x;
                    for (cx, slot) in row.chunks_exact_mut(BYTES).enumerate() {
                        let s = start + cx;
                        let value = match (undefined, replacement) {
                            (Some(mask), Some(r)) if mask[s] => {
                                count += 1;
                                r
                            }
                            _ => $values[s],
                        };
                        slot.copy_from_slice(&value.to_be_bytes());
                    }
                    count
                })
                .sum();
            (out, count)
        }};
    }
    let needs = undefined.is_some_and(|mask| {
        (rect.y..rect.y + rect.height).any(|y| {
            mask[y * source_width + rect.x..y * source_width + rect.x + rect.width]
                .iter()
                .any(|m| *m)
        })
    });
    let blank_for = |bits: &str| -> anyhow::Result<i64> {
        blank.with_context(|| format!("integer image requires a BLANK sentinel ({bits})"))
    };
    Ok(match pixels {
        PixelData::U8(v) => encode!(
            v,
            u8,
            needs
                .then(|| blank_for("BITPIX=8")
                    .and_then(|b| u8::try_from(b).context("BLANK does not fit BITPIX=8")))
                .transpose()?
        ),
        PixelData::I16(v) => encode!(
            v,
            i16,
            needs
                .then(|| blank_for("BITPIX=16")
                    .and_then(|b| i16::try_from(b).context("BLANK does not fit BITPIX=16")))
                .transpose()?
        ),
        PixelData::I32(v) => encode!(
            v,
            i32,
            needs
                .then(|| blank_for("BITPIX=32")
                    .and_then(|b| i32::try_from(b).context("BLANK does not fit BITPIX=32")))
                .transpose()?
        ),
        PixelData::I64(v) => encode!(v, i64, needs.then(|| blank_for("BITPIX=64")).transpose()?),
        PixelData::F32(v) => encode!(v, f32, Some(f32::NAN)),
        PixelData::F64(v) => encode!(v, f64, Some(f64::NAN)),
    })
}

/// Write one science product for `rect` of the source (the full frame for a
/// mask), verify it on disk, and leave it at `destination`.
fn write_region_product(
    destination: &Path,
    image: &Image,
    rect: CropRect,
    cropped: bool,
    undefined: Option<&[bool]>,
    blank: Option<i64>,
    context: ProductContext<'_>,
) -> anyhow::Result<()> {
    let pixels = source_pixels(image)?;
    let (width, height) = (image.width, image.height);
    if rect.width == 0
        || rect.height == 0
        || rect.x.checked_add(rect.width).is_none_or(|x| x > width)
        || rect.y.checked_add(rect.height).is_none_or(|y| y > height)
    {
        bail!("crop rectangle is outside the source image");
    }
    let full_frame = rect.x == 0 && rect.y == 0 && rect.width == width && rect.height == height;
    if cropped && full_frame {
        bail!("a full-frame rectangle is not a crop; it must be written as a mask product");
    }
    if !cropped && !full_frame {
        bail!("a mask product must cover the full frame");
    }
    if undefined.is_some_and(|mask| mask.len() != width * height) {
        bail!("undefined-sample mask dimensions do not match the source");
    }
    let (data, undefined_count) = encode_region(pixels, width, rect, undefined, blank)?;
    let blank_edit = (image.bitpix > 0 && undefined_count > 0)
        .then_some(blank)
        .flatten();
    let created = Utc::now().format("%Y-%m-%dT%H:%M:%S%.3f").to_string();
    let mo_cards = product_header::mo_cards(
        &image.source_header_cards,
        &Provenance {
            action: context.action,
            algorithm: context.algorithm,
            undefined_added: undefined_count,
            crop: cropped.then_some(rect),
            source_size: (width, height),
            source_sha256: &image.source_sha256,
            source_file_name: image.source_file_name.as_deref(),
            created_utc: &created,
        },
    )?;
    let edits = HeaderEdits {
        width: rect.width,
        height: rect.height,
        shift: cropped.then_some((rect.x, rect.y)),
        blank: blank_edit,
        mo_cards,
    };
    let cards = product_header::assemble(&image.source_header_cards, &edits)?;
    let bytes = product_header::finish_hdu(cards, data)?;
    fs::write(destination, bytes)?;
    fs::File::open(destination)?.sync_all()?;
    // Verify the file as it is on disk: DATASUM, header policy and samples.
    let (written, written_cards) = read_fits_snapshot_with_cards(destination)?;
    if written.len() != 1 {
        bail!("written science output does not contain exactly one HDU");
    }
    written.primary().verify_datasum()?;
    product_header::verify(&image.source_header_cards, &written_cards, &edits)?;
    let written_pixels = match &written.primary().data {
        HduData::Image(data) if data.axes == [rect.width, rect.height] => &data.pixels,
        HduData::Image(_) => bail!("written product dimensions differ from the selected region"),
        _ => bail!("written primary HDU is not an image"),
    };
    verify_region_samples(pixels, width, written_pixels, rect, undefined, blank_edit)
}

/// Every written sample equals its source sample, except samples flagged in
/// `undefined`, which must be BLANK (integer) or NaN (floating). Compared in
/// the stored type; the first violating index is reported deterministically.
fn verify_region_samples(
    source: &PixelData,
    source_width: usize,
    written: &PixelData,
    rect: CropRect,
    undefined: Option<&[bool]>,
    blank: Option<i64>,
) -> anyhow::Result<()> {
    if written.len() != rect.area() {
        bail!("written sample count differs from the selected region");
    }
    let source_index =
        |i: usize| (rect.y + i / rect.width) * source_width + rect.x + i % rect.width;
    let flagged = |s: usize| undefined.is_some_and(|mask| mask[s]);
    macro_rules! check_integer {
        ($after:expr, $before:expr) => {{
            let blank = blank;
            $after.par_iter().enumerate().position_first(|(i, &got)| {
                let s = source_index(i);
                if flagged(s) {
                    Some(i64::from(got)) != blank
                } else {
                    got != $before[s]
                }
            })
        }};
    }
    macro_rules! check_float {
        ($after:expr, $before:expr) => {{
            $after.par_iter().enumerate().position_first(|(i, &got)| {
                let s = source_index(i);
                let old = $before[s];
                if flagged(s) {
                    !got.is_nan()
                } else {
                    got != old && !(got.is_nan() && old.is_nan())
                }
            })
        }};
    }
    let found = match (written, source) {
        (PixelData::U8(a), PixelData::U8(b)) => check_integer!(a, b),
        (PixelData::I16(a), PixelData::I16(b)) => check_integer!(a, b),
        (PixelData::I32(a), PixelData::I32(b)) => check_integer!(a, b),
        (PixelData::I64(a), PixelData::I64(b)) => check_integer!(a, b),
        (PixelData::F32(a), PixelData::F32(b)) => check_float!(a, b),
        (PixelData::F64(a), PixelData::F64(b)) => check_float!(a, b),
        _ => bail!("written BITPIX differs from the source"),
    };
    if let Some(i) = found {
        bail!("written pixel {i} violates the retained/undefined sample invariant");
    }
    Ok(())
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

fn choose_mask_blank(source: &FitsFile, bitpix: i64, mask: &[bool]) -> anyhow::Result<i64> {
    let image = match &source.primary().data {
        HduData::Image(image) => image,
        _ => bail!("source primary HDU is not an image"),
    };
    if image.pixels.len() != mask.len() {
        bail!("source dimensions changed before BLANK selection");
    }
    smallest_unused_code(&image.pixels, mask, bitpix)
}

/// Smallest stored code (from the type minimum upward) that no retained
/// sample uses; same result as `unused_integer_value` over the retained
/// values. 8/16-bit images use a presence table (no copy); 32/64-bit images
/// sort a copy in their native width.
fn smallest_unused_code(pixels: &PixelData, excluded: &[bool], bitpix: i64) -> anyhow::Result<i64> {
    fn first_absent(present: &[bool], min: i64, bitpix: i64) -> anyhow::Result<i64> {
        present
            .iter()
            .position(|p| !p)
            .map(|i| min + i as i64)
            .ok_or_else(|| anyhow::Error::new(crate::fits_image::NoUnusedIntegerCode { bitpix }))
    }
    match pixels {
        PixelData::U8(values) => {
            let mut present = [false; 256];
            for (&value, &x) in values.iter().zip(excluded) {
                if !x {
                    present[usize::from(value)] = true;
                }
            }
            first_absent(&present, 0, bitpix)
        }
        PixelData::I16(values) => {
            let mut present = vec![false; 65_536];
            for (&value, &x) in values.iter().zip(excluded) {
                if !x {
                    present[(i32::from(value) + 32_768) as usize] = true;
                }
            }
            first_absent(&present, i64::from(i16::MIN), bitpix)
        }
        PixelData::I32(values) => {
            let mut retained: Vec<i32> = values
                .iter()
                .zip(excluded)
                .filter_map(|(&value, &x)| (!x).then_some(value))
                .collect();
            retained.sort_unstable();
            retained.dedup();
            next_after_sorted(
                retained.into_iter().map(i64::from),
                i64::from(i32::MIN),
                i64::from(i32::MAX),
                bitpix,
            )
        }
        PixelData::I64(values) => {
            let mut retained: Vec<i64> = values
                .iter()
                .zip(excluded)
                .filter_map(|(&value, &x)| (!x).then_some(value))
                .collect();
            retained.sort_unstable();
            retained.dedup();
            next_after_sorted(retained.into_iter(), i64::MIN, i64::MAX, bitpix)
        }
        _ => bail!("integer BITPIX does not match source pixel storage"),
    }
}

/// First code from `min` not present in the sorted, deduplicated `values`.
fn next_after_sorted(
    values: impl Iterator<Item = i64>,
    min: i64,
    max: i64,
    bitpix: i64,
) -> anyhow::Result<i64> {
    let mut candidate = min;
    for value in values {
        if value < candidate {
            continue;
        }
        if value > candidate {
            break;
        }
        if candidate == max {
            return Err(anyhow::Error::new(crate::fits_image::NoUnusedIntegerCode {
                bitpix,
            }));
        }
        candidate += 1;
    }
    Ok(candidate)
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

pub(super) fn publish_fits(file: &FitsFile, destination: &Path) -> anyhow::Result<()> {
    let parent = destination
        .parent()
        .context("FITS destination has no parent")?;
    let temp = NamedTempFile::new_in(parent)?;
    fs::write(temp.path(), bytes_with_valid_checksum(file.clone())?)?;
    temp.as_file().sync_all()?;
    read_fits_snapshot(temp.path())?
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

#[cfg(test)]
mod tests {
    use super::{ProductContext, publish_single_product, write_masked_product, write_reason_mask};
    use crate::crop::CropRect;
    use crate::fits_image::read_image;
    use fitskit::{FitsFile, HduData, HeaderValue, ImageData, Keyword, PixelData};
    use sha2::{Digest, Sha256};
    use std::fs;

    const CONTEXT: ProductContext<'static> = ProductContext {
        action: "mask",
        algorithm: "test-algorithm",
    };

    /// FITS bytes from literal header cards (END and padding added) and data.
    fn raw_fits(cards: &[&str], data: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for card in cards.iter().chain(std::iter::once(&"END")) {
            let mut c = [b' '; 80];
            c[..card.len()].copy_from_slice(card.as_bytes());
            bytes.extend_from_slice(&c);
        }
        bytes.resize(bytes.len().div_ceil(2880) * 2880, b' ');
        bytes.extend_from_slice(data);
        bytes.resize(bytes.len().div_ceil(2880) * 2880, 0);
        bytes
    }

    fn header_cards(bytes: &[u8]) -> Vec<String> {
        bytes
            .chunks_exact(80)
            .map(|c| String::from_utf8_lossy(c).into_owned())
            .take_while(|c| !c.starts_with("END "))
            .collect()
    }

    #[test]
    fn products_copy_source_cards_verbatim_and_add_mo_cards() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("frame.fits");
        let (width, height) = (64usize, 48usize);
        let mut values = vec![1000.0f32; width * height];
        for y in 0..height {
            for x in 0..6 {
                values[y * width + x] = 0.0;
            }
        }
        let data: Vec<u8> = values.iter().flat_map(|v| v.to_be_bytes()).collect();
        let cards = [
            "SIMPLE  =                    T / conforms",
            "BITPIX  =                  -32",
            "NAXIS   =                    2",
            "NAXIS1  =                   64 / axis 1",
            "NAXIS2  =                   48",
            "DATE    = '2007-01-11T20:23:31' / original file date",
            "CREATOR = 'acquisition system'",
            "CRPIX1  =   32.5   / odd spacing preserved in masks",
            "OBJECT  = 'it''s a target'",
            "HISTORY = 'value-style history kept as written'",
            "HISTORY plain commentary",
            "DATAMIN =                  0.0",
        ];
        fs::write(&source, raw_fits(&cards, &data)).unwrap();
        let image = read_image(&source, 256, None).unwrap().unwrap();
        let mut mask = vec![false; width * height];
        for y in 0..height {
            for x in 0..6 {
                mask[y * width + x] = true;
            }
        }
        let masked = temp.path().join("masked.fits");
        write_masked_product(&masked, &image, &mask, CONTEXT).unwrap();
        let got = header_cards(&fs::read(&masked).unwrap());
        let source_cards = header_cards(&fs::read(&source).unwrap());
        // Every source card is copied unchanged and in order (mask: no edits).
        assert_eq!(&got[..source_cards.len()], &source_cards[..]);
        let added: Vec<&str> = got[source_cards.len()..].iter().map(|c| &c[..8]).collect();
        assert_eq!(&added[..2], ["MO_ACT  ", "MO_CROP "]);
        assert!(
            added
                .iter()
                .all(|k| k.starts_with("MO_") || *k == "DATASUM " || *k == "CHECKSUM")
        );
        assert!(
            got.iter()
                .any(|c| c.starts_with("MO_CROP =                    F"))
        );

        let cropped = temp.path().join("cropped.fits");
        let rect = CropRect {
            x: 6,
            y: 0,
            width: 58,
            height: 48,
        };
        super::write_cropped_product(
            &cropped,
            &image,
            rect,
            ProductContext {
                action: "crop",
                ..CONTEXT
            },
        )
        .unwrap();
        let got = header_cards(&fs::read(&cropped).unwrap());
        for (old, new) in source_cards.iter().zip(&got) {
            match &old[..8] {
                "NAXIS1  " => assert!(new.starts_with("NAXIS1  =                   58 / axis 1")),
                "CRPIX1  " => {
                    assert!(new.starts_with("CRPIX1  =                 26.5 / odd spacing"))
                }
                _ => assert_eq!(old, new),
            }
        }
        for expected in [
            "MO_CROP =                    T",
            "MO_SEC  = '[7:64,1:48]'",
            "MO_ONAX1=                   64",
            "MO_X0   =                    6",
            "MO_FILE = 'frame.fits'",
        ] {
            assert!(got.iter().any(|c| c.starts_with(expected)), "{expected}");
        }
        let file = FitsFile::from_file(&cropped).unwrap();
        file.primary().verify_datasum().unwrap();
    }

    #[test]
    fn presence_table_blank_search_matches_the_sorting_reference() {
        use crate::fits_image::unused_integer_value;
        let mut state = 1u64;
        let mut next = move || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            state >> 33
        };
        for trial in 0..300 {
            let n = 1 + (next() % 400) as usize;
            let excluded: Vec<bool> = (0..n).map(|_| next() % 5 == 0).collect();
            let pick = |v: &[i64]| -> Vec<i64> {
                v.iter()
                    .zip(&excluded)
                    .filter(|(_, x)| !**x)
                    .map(|(v, _)| *v)
                    .collect()
            };
            // Values concentrated near the type minimum exercise the gaps.
            let u8s: Vec<u8> = (0..n).map(|_| (next() % 40) as u8).collect();
            let wide: Vec<i64> = u8s.iter().map(|&v| i64::from(v)).collect();
            assert_eq!(
                super::smallest_unused_code(&PixelData::U8(u8s.clone()), &excluded, 8).unwrap(),
                unused_integer_value(8, &pick(&wide)).unwrap(),
                "u8 trial {trial}"
            );
            let i16s: Vec<i16> = (0..n).map(|_| i16::MIN + (next() % 60) as i16).collect();
            let wide: Vec<i64> = i16s.iter().map(|&v| i64::from(v)).collect();
            assert_eq!(
                super::smallest_unused_code(&PixelData::I16(i16s), &excluded, 16).unwrap(),
                unused_integer_value(16, &pick(&wide)).unwrap()
            );
            let i32s: Vec<i32> = (0..n).map(|_| i32::MIN + (next() % 60) as i32).collect();
            let wide: Vec<i64> = i32s.iter().map(|&v| i64::from(v)).collect();
            assert_eq!(
                super::smallest_unused_code(&PixelData::I32(i32s), &excluded, 32).unwrap(),
                unused_integer_value(32, &pick(&wide)).unwrap()
            );
        }
        // Full occupancy has no free code, exactly like the reference.
        let all: Vec<u8> = (0..=255).collect();
        assert!(super::smallest_unused_code(&PixelData::U8(all), &[false; 256], 8).is_err());
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

        let image = read_image(&source, 256, None).unwrap().unwrap();
        let mut mask = vec![false; width * height];
        for y in 0..16 {
            for x in 0..32 {
                mask[y * width + x] = true;
            }
        }
        write_masked_product(&output, &image, &mask, CONTEXT).unwrap();
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
        let image = read_image(&source, 256, None).unwrap().unwrap();
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
        write_masked_product(&output, &image, &mask, CONTEXT).unwrap();

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
