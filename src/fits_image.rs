//! Rust-native FITS loading, eligibility checks, and detection-value conversion.
use anyhow::{Context, bail};
use fitskit::{FitsFile, HduData, Header, Keyword, PixelData};
use sha2::{Digest, Sha256};
use std::{
    fmt,
    fs::File,
    io::{self, Cursor, Read, Seek, SeekFrom},
    path::Path,
};

const ESTIMATED_WORKSPACE_BYTES_PER_PIXEL: usize = 40;

#[derive(Debug)]
pub struct NoUnusedIntegerCode {
    pub bitpix: i64,
}

impl fmt::Display for NoUnusedIntegerCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "no unused integer pixel value is available for BLANK at BITPIX={}",
            self.bitpix
        )
    }
}

impl std::error::Error for NoUnusedIntegerCode {}

#[derive(Debug)]
pub enum Eligibility {
    MultiHdu(usize),
    Not2d,
    EmptyImage,
    IntegerPrecisionLoss,
}

pub struct Image {
    pub width: usize,
    pub height: usize,
    pub values: Vec<f64>,
    pub bitpix: i64,
    pub blank_value: Option<i64>,
    pub metadata_warnings: Vec<String>,
    pub crop_wcs_unsupported: Option<String>,
    /// Original non-standard/raw cards to restore verbatim in generated products.
    pub raw_header_cards: Vec<RawHeaderCard>,
    /// Parsed source snapshot used by both classification and product writing.
    pub source_fits: FitsFile,
    /// SHA-256 of the exact open source file used to build `source_fits`.
    pub source_sha256: String,
}

#[derive(Clone)]
pub struct RawHeaderCard {
    pub(crate) offset: u64,
    pub(crate) replacement: [u8; 80],
    pub(crate) original: [u8; 80],
    pub(crate) annotation: Option<String>,
}

/// Parse a FITS file using fitskit and validate the single-HDU 2D image contract.
pub fn read_image(
    path: &Path,
    maximum_image_mib: usize,
) -> anyhow::Result<Result<Image, Eligibility>> {
    // Inspect headers and seek over payloads before allocating parsed pixel buffers.
    let mut source =
        File::open(path).with_context(|| format!("opening FITS {}", path.display()))?;
    let source_metadata = source.metadata()?;
    let (hdu_count, axes, bitpix, repairs) = preflight(&mut source, path)?;
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
    let max_pixels =
        maximum_image_mib.saturating_mul(1024 * 1024) / ESTIMATED_WORKSPACE_BYTES_PER_PIXEL;
    if n > max_pixels {
        bail!(
            "image has {n} pixels; estimated workspace exceeds configured maximum_image_mib={maximum_image_mib}"
        );
    }
    source.seek(SeekFrom::Start(0))?;
    let (file, source_sha256) = {
        let mut reader = HashingReader::new(&mut source, repairs.clone());
        let file = FitsFile::from_reader(&mut reader)
            .with_context(|| format!("opening FITS {}", path.display()))?;
        (file, reader.finish())
    };
    let parsed_metadata = source.metadata()?;
    if source_metadata.len() != parsed_metadata.len()
        || source_metadata.modified().ok() != parsed_metadata.modified().ok()
    {
        bail!("source file changed while it was being parsed");
    }
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
    let mut metadata_warnings = repairs
        .iter()
        .filter_map(|repair| repair.annotation.clone())
        .collect::<Vec<_>>();
    if let Some(declared) = declared_blank.filter(|_| existing_blank.is_none()) {
        metadata_warnings.push(format!(
            "ignored out-of-range BLANK={} for BITPIX={bitpix}; pixel statistics use stored image values",
            declared
        ));
    }
    let (values, blank_value) = match &image.pixels {
        PixelData::U8(raw) => {
            if integer_span_exceeds_f64_exactness(raw, existing_blank) {
                return Ok(Err(Eligibility::IntegerPrecisionLoss));
            }
            (integer_values(raw, existing_blank)?, existing_blank)
        }
        PixelData::I16(raw) => {
            if integer_span_exceeds_f64_exactness(raw, existing_blank) {
                return Ok(Err(Eligibility::IntegerPrecisionLoss));
            }
            (integer_values(raw, existing_blank)?, existing_blank)
        }
        PixelData::I32(raw) => {
            if integer_span_exceeds_f64_exactness(raw, existing_blank) {
                return Ok(Err(Eligibility::IntegerPrecisionLoss));
            }
            (integer_values(raw, existing_blank)?, existing_blank)
        }
        PixelData::I64(raw) => {
            if integer_span_exceeds_f64_exactness(raw, existing_blank) {
                return Ok(Err(Eligibility::IntegerPrecisionLoss));
            }
            (integer_values(raw, existing_blank)?, existing_blank)
        }
        PixelData::F32(raw) => (center_float_values(raw.iter().map(|&v| f64::from(v))), None),
        PixelData::F64(raw) => (center_float_values(raw.iter().copied()), None),
    };
    if values.len() != n {
        bail!("FITS array size does not match NAXIS dimensions");
    }
    let crop_wcs_unsupported = crop_wcs_unsupported(&hdu.header);
    if let Some(reason) = &crop_wcs_unsupported {
        metadata_warnings.push(format!(
            "metadata note: {reason}; pixel statistics are still processed"
        ));
    }
    Ok(Ok(Image {
        width,
        height,
        values,
        bitpix,
        blank_value,
        metadata_warnings,
        crop_wcs_unsupported,
        raw_header_cards: repairs,
        source_fits: file,
        source_sha256,
    }))
}

/// Read a generated single-HDU FITS snapshot while treating malformed
/// non-structural header cards as same-width in-memory placeholders.
pub fn read_fits_snapshot(path: &Path) -> anyhow::Result<FitsFile> {
    let mut source =
        File::open(path).with_context(|| format!("opening FITS {}", path.display()))?;
    let (_, _, _, raw_cards) = preflight(&mut source, path)?;
    source.seek(SeekFrom::Start(0))?;
    let mut reader = HashingReader::new(&mut source, raw_cards);
    FitsFile::from_reader(&mut reader).with_context(|| format!("parsing FITS {}", path.display()))
}

/// Hash bytes as fitskit reads them. Forward seeks are implemented by reading
/// the skipped bytes, so FITS block padding is part of the digest too.
struct HashingReader<'a> {
    inner: &'a mut File,
    hasher: Sha256,
    repairs: Vec<RawHeaderCard>,
}

impl<'a> HashingReader<'a> {
    fn new(inner: &'a mut File, repairs: Vec<RawHeaderCard>) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
            repairs,
        }
    }

    fn finish(self) -> String {
        format!("{:x}", self.hasher.finalize())
    }
}

impl Read for HashingReader<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let start = self.inner.stream_position()?;
        let read = self.inner.read(output)?;
        self.hasher.update(&output[..read]);
        let end = start.saturating_add(read as u64);
        for repair in &self.repairs {
            let repair_end = repair
                .offset
                .saturating_add(repair.replacement.len() as u64);
            let overlap_start = start.max(repair.offset);
            let overlap_end = end.min(repair_end);
            if overlap_start < overlap_end {
                let output_start = usize::try_from(overlap_start - start).unwrap();
                let replacement_start = usize::try_from(overlap_start - repair.offset).unwrap();
                let overlap_len = usize::try_from(overlap_end - overlap_start).unwrap();
                output[output_start..output_start + overlap_len].copy_from_slice(
                    &repair.replacement[replacement_start..replacement_start + overlap_len],
                );
            }
        }
        Ok(read)
    }
}

impl Seek for HashingReader<'_> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let current = self.inner.stream_position()?;
        let target = match position {
            SeekFrom::Start(target) => target,
            SeekFrom::Current(delta) => current
                .checked_add_signed(delta)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid seek"))?,
            SeekFrom::End(delta) => self
                .inner
                .metadata()?
                .len()
                .checked_add_signed(delta)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid seek"))?,
        };
        if target < current {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "FITS parser requested a backward seek while hashing",
            ));
        }
        let mut remaining = target - current;
        let mut buffer = [0_u8; 8192];
        while remaining > 0 {
            let chunk = usize::try_from(remaining.min(buffer.len() as u64)).unwrap();
            let read = self.read(&mut buffer[..chunk])?;
            if read == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "FITS parser requested a seek past end of source",
                ));
            }
            remaining -= read as u64;
        }
        Ok(target)
    }
}

fn crop_wcs_unsupported(header: &Header) -> Option<String> {
    for axis in 1..=2 {
        for suffix in std::iter::once(String::new())
            .chain((b'A'..=b'Z').map(|letter| char::from(letter).to_string()))
        {
            let key = format!("CTYPE{axis}{suffix}");
            if header
                .get_string(&key)
                .is_some_and(|value| value.to_ascii_uppercase().contains("-TAB"))
            {
                return Some(format!(
                    "{key} uses table-lookup WCS, which crop mode cannot update safely"
                ));
            }
        }
    }
    for keyword in header.iter() {
        let name = keyword.name.as_str();
        if [
            "CPDIS", "CPERR", "D2IMDIS", "D2IMERR", "DP1.", "DP2.", "D2IM1.", "D2IM2.",
        ]
        .iter()
        .any(|prefix| name.starts_with(prefix))
        {
            return Some(format!(
                "{} declares lookup-table distortion, which crop mode cannot update safely",
                keyword.name
            ));
        }
    }
    None
}

/// Detect gzip by file content so a compressed stream with a misleading FITS
/// suffix is ignored without asking the FITS parser to read it.
pub fn is_gzip_compressed(path: &Path) -> anyhow::Result<bool> {
    let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut signature = [0_u8; 2];
    let count = file.read(&mut signature)?;
    Ok(count == signature.len() && signature == [0x1f, 0x8b])
}

fn preflight(
    file: &mut File,
    path: &Path,
) -> anyhow::Result<(usize, Vec<usize>, i64, Vec<RawHeaderCard>)> {
    file.seek(SeekFrom::Start(0))?;
    let file_len = file.metadata()?.len();
    let mut count = 0usize;
    let mut first_axes = Vec::new();
    let mut first_bitpix = 0i64;
    let mut repairs = Vec::new();
    loop {
        let position = file.stream_position()?;
        if position == file_len {
            break;
        }
        if position > file_len {
            bail!("FITS HDU exceeds file length");
        }
        let header = read_header_ignoring_nonstructural_card_errors(file, path, &mut repairs)
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
    file.seek(SeekFrom::Start(0))?;
    Ok((count, first_axes, first_bitpix, repairs))
}

/// Parse every header card while replacing malformed non-layout cards with
/// COMMENT cards of the same width. The primary image dimensions and payload
/// layout still have to be valid so pixel bytes can be located safely.
fn read_header_ignoring_nonstructural_card_errors(
    file: &mut File,
    path: &Path,
    repairs: &mut Vec<RawHeaderCard>,
) -> anyhow::Result<Header> {
    const BLOCK_BYTES: usize = 2880;
    const CARD_BYTES: usize = 80;

    let header_start = file.stream_position()?;
    let mut bytes = Vec::new();
    let mut found_end = false;
    while !found_end {
        let mut block = [0u8; BLOCK_BYTES];
        file.read_exact(&mut block)
            .with_context(|| format!("reading FITS header block in {}", path.display()))?;
        for card in block.chunks_exact(CARD_BYTES) {
            if card[..3] == *b"END" && card[3..8].iter().all(|&byte| byte == b' ') {
                found_end = true;
                break;
            }
        }
        bytes.extend_from_slice(&block);
        if bytes.len() > 64 * 1024 * 1024 {
            bail!("FITS header exceeds the 64 MiB safety limit");
        }
    }

    for index in 0..bytes.len() / CARD_BYTES {
        let card = &bytes[index * CARD_BYTES..(index + 1) * CARD_BYTES];
        if card[..3] == *b"END" && card[3..8].iter().all(|&byte| byte == b' ') {
            break;
        }
        let record: [u8; CARD_BYTES] = card.try_into().expect("fixed-size FITS card");
        let is_hierarch = record.starts_with(b"HIERARCH ");
        let parse_result = Keyword::parse(&record);
        if parse_result.is_ok() && !is_hierarch {
            continue;
        }
        let parse_error = match parse_result {
            Ok(_) => None,
            Err(error) => Some(error.to_string()),
        };
        let key = if is_hierarch {
            record[9..]
                .split(|&byte| byte == b'=')
                .next()
                .map(|bytes| String::from_utf8_lossy(bytes).trim().to_owned())
                .unwrap_or_else(|| "HIERARCH".to_owned())
        } else {
            std::str::from_utf8(&record[..8])
                .unwrap_or("")
                .trim()
                .to_ascii_uppercase()
        };
        if !is_hierarch && is_layout_keyword(&key) {
            bail!(
                "malformed structural FITS card {key:?}: {}",
                parse_error.as_deref().unwrap_or("invalid card")
            );
        }
        let offset = header_start + (index * CARD_BYTES) as u64;
        let replacement = raw_card_marker(offset);
        bytes[index * CARD_BYTES..(index + 1) * CARD_BYTES].copy_from_slice(&replacement);
        let annotation = if is_hierarch {
            None
        } else {
            Some(format!(
                "ignored malformed non-structural FITS header card at byte {offset} ({key}): {}; original header card preserved unchanged",
                parse_error.as_deref().unwrap_or("invalid card")
            ))
        };
        repairs.push(RawHeaderCard {
            offset,
            replacement,
            original: record,
            annotation,
        });
    }

    let mut cursor = Cursor::new(bytes);
    Header::read_from(&mut cursor).map_err(Into::into)
}

fn raw_card_marker(offset: u64) -> [u8; 80] {
    let marker = format!("COMMENT RUST_FITS_RAW_CARD_{offset:016X}");
    let mut card = [b' '; 80];
    card[..marker.len()].copy_from_slice(marker.as_bytes());
    card
}

fn is_layout_keyword(key: &str) -> bool {
    matches!(
        key,
        "SIMPLE" | "XTENSION" | "BITPIX" | "NAXIS" | "PCOUNT" | "GCOUNT"
    ) || key.strip_prefix("NAXIS").is_some_and(|suffix| {
        !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
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

fn integer_span_exceeds_f64_exactness<T>(raw: &[T], blank: Option<i64>) -> bool
where
    T: Copy + Into<i64>,
{
    let Some((minimum, maximum)) = raw
        .iter()
        .copied()
        .map(Into::into)
        .filter(|value| Some(*value) != blank)
        .fold(None, |range: Option<(i64, i64)>, value| {
            Some(range.map_or((value, value), |(min, max)| {
                (min.min(value), max.max(value))
            }))
        })
    else {
        return false;
    };
    i128::from(maximum) - i128::from(minimum) > (1_i128 << 53)
}

fn integer_values<T>(raw: &[T], existing_blank: Option<i64>) -> anyhow::Result<Vec<f64>>
where
    T: Copy + Into<i64>,
{
    let raw: Vec<i64> = raw.iter().copied().map(Into::into).collect();
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
    Ok(raw
        .iter()
        .map(|&v| {
            if Some(v) == existing_blank {
                f64::NAN
            } else {
                (v as i128 - anchor as i128) as f64
            }
        })
        .collect())
}

fn center_float_values(values: impl Iterator<Item = f64>) -> Vec<f64> {
    let mut values: Vec<f64> = values.collect();
    let mut finite: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    if finite.is_empty() {
        return values;
    }
    finite.sort_by(f64::total_cmp);
    let middle = finite.len() / 2;
    let center = if finite.len().is_multiple_of(2) {
        finite[middle - 1] * 0.5 + finite[middle] * 0.5
    } else {
        finite[middle]
    };
    for value in &mut values {
        if value.is_finite() {
            let difference = *value - center;
            *value = if difference.is_finite() {
                difference
            } else {
                *value * 0.5 - center * 0.5
            };
        }
    }
    values
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
            return Err(anyhow::Error::new(NoUnusedIntegerCode { bitpix }));
        }
        candidate += 1;
    }
    Ok(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_centering_avoids_overflow_across_opposite_extremes() {
        let values = center_float_values([f64::MAX, -f64::MAX].into_iter());
        assert_eq!(values, [f64::MAX, -f64::MAX]);
        assert!(values.iter().all(|value| value.is_finite()));
    }

    #[test]
    fn integer_conversion_abstains_when_span_exceeds_exact_f64_integers() {
        assert!(!integer_span_exceeds_f64_exactness(
            &[0_i64, 1_i64 << 53],
            None
        ));
        assert!(integer_span_exceeds_f64_exactness(
            &[0_i64, (1_i64 << 53) + 1],
            None
        ));
        assert!(integer_span_exceeds_f64_exactness(
            &[i64::MIN, i64::MAX],
            None
        ));
        assert!(!integer_span_exceeds_f64_exactness(
            &[i64::MIN, i64::MAX],
            Some(i64::MIN)
        ));
    }
}
