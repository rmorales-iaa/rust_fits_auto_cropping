//! Product headers: the source primary header copied card for card (original
//! 80-byte images), with only the edits a product requires, plus the MO_
//! provenance group.
//!
//! Edits: NAXIS1/NAXIS2 (crop dimensions), CRPIXn[a] and existing LTV1/LTV2
//! (shifted by the crop origin so the WCS and IRAF physical coordinates stay
//! correct), BLANK (only when integer samples are newly set undefined), and
//! CHECKSUM/DATASUM (recomputed). Earlier MO_ cards are replaced. Every other
//! card, including COMMENT/HISTORY and malformed cards, is written unchanged.
use crate::crop::CropRect;
use anyhow::{Context, bail};

pub type Card = [u8; 80];

/// Keyword of a card (columns 1-8, trailing blanks removed).
pub fn card_key(card: &Card) -> String {
    String::from_utf8_lossy(&card[..8]).trim_end().to_owned()
}

/// A card value for `format_card`.
pub enum Value<'a> {
    Int(i64),
    Float(f64),
    Logical(bool),
    Str(&'a str),
}

/// Fixed-format FITS card: numbers and logicals right-justified to column 30,
/// strings quoted from column 11, then ` / comment` truncated at column 80.
pub fn format_card(key: &str, value: Value<'_>, comment: Option<&str>) -> anyhow::Result<Card> {
    if key.is_empty()
        || key.len() > 8
        || !key
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
    {
        bail!("invalid FITS keyword {key:?}");
    }
    let field = match value {
        Value::Int(v) => format!("{v:>20}"),
        Value::Float(v) => format!("{:>20}", format_float(v)?),
        Value::Logical(v) => format!("{:>20}", if v { "T" } else { "F" }),
        Value::Str(s) => {
            if !s.bytes().all(|b| (0x20..0x7f).contains(&b)) {
                bail!("FITS string for {key} is not printable ASCII");
            }
            format!("'{:<8}'", s.replace('\'', "''"))
        }
    };
    let mut text = format!("{key:<8}= {field}");
    if let Some(comment) = comment {
        if !comment.bytes().all(|b| (0x20..0x7f).contains(&b)) {
            bail!("FITS comment for {key} is not printable ASCII");
        }
        text.push_str(" / ");
        text.push_str(comment);
    }
    if text.len() > 80 {
        if 10 + field.len() > 80 {
            bail!("FITS value for {key} does not fit on one card");
        }
        text.truncate(80);
    }
    let mut card = [b' '; 80];
    card[..text.len()].copy_from_slice(text.as_bytes());
    Ok(card)
}

/// Shortest round-trip decimal form with a decimal point and an `E` exponent
/// when needed, so the value parses back to the same f64.
fn format_float(v: f64) -> anyhow::Result<String> {
    if !v.is_finite() {
        bail!("non-finite FITS real value");
    }
    let s = format!("{v:?}");
    let s = match s.split_once('e') {
        Some((mantissa, exponent)) => {
            let mantissa = if mantissa.contains('.') {
                mantissa.to_owned()
            } else {
                format!("{mantissa}.0")
            };
            format!("{mantissa}E{exponent}")
        }
        None if s.contains('.') => s,
        None => format!("{s}.0"),
    };
    Ok(s)
}

/// Value text of a value card (between `= ` and any comment).
fn value_text(card: &Card) -> Option<&str> {
    if &card[8..10] != b"= " {
        return None;
    }
    let text = std::str::from_utf8(&card[10..]).ok()?;
    if text.trim_start().starts_with('\'') {
        return None; // strings are never edited numerically
    }
    Some(text.split('/').next().unwrap_or("").trim())
}

/// Numeric value of a card (FITS allows a `D` exponent).
pub fn numeric_value(card: &Card) -> Option<f64> {
    value_text(card)?.replace(['D', 'd'], "E").parse().ok()
}

/// Comment of a numeric card, if any.
fn numeric_comment(card: &Card) -> Option<String> {
    let text = std::str::from_utf8(&card[10..]).ok()?;
    let (_, comment) = text.split_once('/')?;
    let comment = comment.trim();
    (!comment.is_empty()).then(|| comment.to_owned())
}

fn is_crpix(key: &str) -> bool {
    key.strip_prefix("CRPIX")
        .is_some_and(|rest| matches!(rest.as_bytes(), [b'1' | b'2'] | [b'1' | b'2', b'A'..=b'Z']))
}

/// Axis (1 or 2) of a card shifted by a crop, if any.
fn shifted_axis(key: &str) -> Option<usize> {
    if is_crpix(key) {
        return Some(if key.as_bytes()[5] == b'1' { 1 } else { 2 });
    }
    match key {
        "LTV1" => Some(1),
        "LTV2" => Some(2),
        _ => None,
    }
}

/// What a product changes relative to its source header.
pub struct HeaderEdits {
    pub width: usize,
    pub height: usize,
    /// Crop origin in the source (shift for CRPIX/LTV); `None` for masks.
    pub shift: Option<(usize, usize)>,
    /// BLANK required because integer samples are newly undefined.
    pub blank: Option<i64>,
    pub mo_cards: Vec<Card>,
}

/// Source cards with the product edits applied, MO_ group appended (no
/// checksum cards, no END).
pub fn assemble(source: &[Card], edits: &HeaderEdits) -> anyhow::Result<Vec<Card>> {
    let mut out = Vec::with_capacity(source.len() + edits.mo_cards.len() + 4);
    let mut blank_written = false;
    let mut last_naxis = None;
    for card in source {
        let key = card_key(card);
        if key.starts_with("MO_") || key == "CHECKSUM" || key == "DATASUM" {
            continue;
        }
        let edited = match key.as_str() {
            "NAXIS1" | "NAXIS2" => {
                let value = if key == "NAXIS1" {
                    edits.width
                } else {
                    edits.height
                };
                Some(format_card(
                    &key,
                    Value::Int(value as i64),
                    numeric_comment(card).as_deref(),
                )?)
            }
            "BLANK" => match edits.blank {
                Some(blank) => {
                    blank_written = true;
                    (numeric_value(card) != Some(blank as f64))
                        .then(|| {
                            format_card(&key, Value::Int(blank), numeric_comment(card).as_deref())
                        })
                        .transpose()?
                }
                None => None,
            },
            _ => match (edits.shift, shifted_axis(&key)) {
                (Some((x0, y0)), Some(axis)) => {
                    let value = numeric_value(card)
                        .with_context(|| format!("{key} has no numeric value to shift"))?;
                    let offset = if axis == 1 { x0 } else { y0 } as f64;
                    Some(format_card(
                        &key,
                        Value::Float(value - offset),
                        numeric_comment(card).as_deref(),
                    )?)
                }
                _ => None,
            },
        };
        if key.starts_with("NAXIS") {
            last_naxis = Some(out.len());
        }
        out.push(edited.unwrap_or(*card));
    }
    if let Some(blank) = edits.blank
        && !blank_written
    {
        let at = last_naxis.map_or(out.len(), |i| i + 1);
        out.insert(
            at,
            format_card("BLANK", Value::Int(blank), Some("undefined pixel value"))?,
        );
    }
    out.extend_from_slice(&edits.mo_cards);
    Ok(out)
}

/// Complete HDU bytes: header cards, DATASUM, CHECKSUM, END, block padding,
/// then the big-endian data padded with zeros. Verifies the FITS checksum
/// invariant before returning.
pub fn finish_hdu(mut cards: Vec<Card>, mut data: Vec<u8>) -> anyhow::Result<Vec<u8>> {
    let data_len = data.len();
    data.resize(data_len.div_ceil(2880) * 2880, 0);
    let datasum = fitskit::checksum::checksum(&data);
    cards.push(format_card(
        "DATASUM",
        Value::Str(&datasum.to_string()),
        Some("data unit checksum"),
    )?);
    let checksum_card = cards.len();
    // Conventional layout (value padded to column 30, comment slash in column
    // 32): readers such as Astropy re-render this card when verifying, so any
    // other layout would make their recomputed checksum differ.
    let mut checksum = [b' '; 80];
    let text = b"CHECKSUM= '0000000000000000'   / HDU checksum";
    checksum[..text.len()].copy_from_slice(text);
    cards.push(checksum);
    let mut end = [b' '; 80];
    end[..3].copy_from_slice(b"END");
    cards.push(end);
    let mut bytes = Vec::with_capacity(cards.len().div_ceil(36) * 2880 + data.len());
    for card in &cards {
        bytes.extend_from_slice(card);
    }
    bytes.resize(bytes.len().div_ceil(2880) * 2880, b' ');
    let header_len = bytes.len();
    bytes.extend_from_slice(&data);
    drop(data);
    let mut sum = fitskit::checksum::Checksum::new();
    sum.update(&bytes[..header_len]);
    let header_sum = sum.finish();
    // Ones-complement addition of the header and data sums (end-around carry).
    let total = {
        let s = u64::from(header_sum) + u64::from(datasum);
        ((s & 0xffff_ffff) + (s >> 32)) as u32
    };
    let encoded = fitskit::checksum::encode_checksum(total, true);
    if encoded.len() != 16 {
        bail!("FITS checksum encoder returned an invalid value");
    }
    let at = checksum_card * 80 + 11;
    bytes[at..at + 16].copy_from_slice(encoded.as_bytes());
    let verified = fitskit::checksum::checksum(&bytes);
    if verified != 0xffff_ffff && verified != 0 {
        bail!("product FITS CHECKSUM invariant failed: {verified:#010x}");
    }
    Ok(bytes)
}

/// Keys a product may change or add; all other source cards must be copied
/// byte for byte and in order.
fn product_may_edit(key: &str, edits: &HeaderEdits) -> bool {
    key.starts_with("MO_")
        || matches!(key, "NAXIS1" | "NAXIS2" | "CHECKSUM" | "DATASUM")
        || (key == "BLANK" && edits.blank.is_some())
        || (edits.shift.is_some() && shifted_axis(key).is_some())
}

/// Check the on-disk product header against its source cards: preserved
/// cards identical and in order, and every edited value as expected.
pub fn verify(source: &[Card], written: &[Card], edits: &HeaderEdits) -> anyhow::Result<()> {
    let preserved = |cards: &[Card]| -> Vec<Card> {
        cards
            .iter()
            .filter(|card| !product_may_edit(&card_key(card), edits))
            .copied()
            .collect()
    };
    if preserved(source) != preserved(written) {
        bail!("product header does not preserve the source cards verbatim");
    }
    let find = |cards: &[Card], key: &str| -> Vec<Card> {
        cards
            .iter()
            .filter(|c| card_key(c) == key)
            .copied()
            .collect()
    };
    for (key, value) in [("NAXIS1", edits.width), ("NAXIS2", edits.height)] {
        let cards = find(written, key);
        if cards.len() != 1 || numeric_value(&cards[0]) != Some(value as f64) {
            bail!("product {key} is not {value}");
        }
    }
    if let Some((x0, y0)) = edits.shift {
        for card in source {
            let key = card_key(card);
            let Some(axis) = shifted_axis(&key) else {
                continue;
            };
            let offset = if axis == 1 { x0 } else { y0 } as f64;
            let expected = numeric_value(card).map(|v| v - offset);
            let got = find(written, &key);
            if got.len() != find(source, &key).len()
                || got.iter().any(|c| numeric_value(c) != expected)
            {
                bail!("product {key} was not shifted by the crop origin");
            }
        }
    }
    if let Some(blank) = edits.blank {
        let cards = find(written, "BLANK");
        if cards.len() != 1 || numeric_value(&cards[0]) != Some(blank as f64) {
            bail!("product BLANK is not {blank}");
        }
    }
    let mo_written: Vec<Card> = written
        .iter()
        .filter(|c| card_key(c).starts_with("MO_"))
        .copied()
        .collect();
    if mo_written != edits.mo_cards {
        bail!("product MO_ provenance cards differ from those intended");
    }
    Ok(())
}

/// MO_ provenance of a product (all products; crop cards when the product,
/// or the source it was made from, is a crop of an original frame).
pub struct Provenance<'a> {
    pub action: &'a str,
    pub algorithm: &'a str,
    pub undefined_added: usize,
    /// Crop rectangle in the immediate source; `None` for masks.
    pub crop: Option<CropRect>,
    pub source_size: (usize, usize),
    pub source_sha256: &'a str,
    pub source_file_name: Option<&'a str>,
    /// UTC creation time, ISO 8601.
    pub created_utc: &'a str,
}

/// Crop recorded in a source that is itself a product of this program.
pub struct EarlierCrop {
    pub section: CropRect,
    pub original_size: (usize, usize),
    pub sha256: Option<String>,
    pub file_name: Option<String>,
}

/// Parse MO_CROP = T, MO_SEC, MO_ONAX1/2 (and identity) from source cards.
pub fn earlier_crop(source: &[Card]) -> Option<EarlierCrop> {
    let get = |key: &str| source.iter().find(|c| card_key(c) == key);
    let string = |key: &str| -> Option<String> {
        let card = get(key)?;
        let text = std::str::from_utf8(&card[10..]).ok()?.trim_start();
        let inner = text.strip_prefix('\'')?;
        let mut value = String::new();
        let mut chars = inner.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\'' {
                if chars.peek() == Some(&'\'') {
                    chars.next();
                    value.push('\'');
                } else {
                    return Some(value.trim_end().to_owned());
                }
            } else {
                value.push(c);
            }
        }
        None
    };
    let logical = get("MO_CROP").and_then(value_text)?;
    if logical != "T" {
        return None;
    }
    let section = parse_section(&string("MO_SEC")?)?;
    let width = get("MO_ONAX1").and_then(numeric_value)?;
    let height = get("MO_ONAX2").and_then(numeric_value)?;
    Some(EarlierCrop {
        section,
        original_size: (width as usize, height as usize),
        sha256: string("MO_SHA"),
        file_name: string("MO_FILE"),
    })
}

/// Parse a 1-based inclusive `[x1:x2,y1:y2]` section into a zero-based rectangle.
pub fn parse_section(text: &str) -> Option<CropRect> {
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

/// IRAF/FITS image section of a zero-based rectangle: `[x1:x2,y1:y2]`, 1-based, inclusive.
pub fn crop_section(rect: CropRect) -> String {
    format!(
        "[{}:{},{}:{}]",
        rect.x + 1,
        rect.x + rect.width,
        rect.y + 1,
        rect.y + rect.height
    )
}

fn safe_name(name: &str) -> bool {
    name.len() <= 60
        && name
            .bytes()
            .all(|b| (0x20..0x7f).contains(&b) && b != b'\'')
}

/// The MO_ cards for a product.
pub fn mo_cards(source: &[Card], p: &Provenance<'_>) -> anyhow::Result<Vec<Card>> {
    let earlier = earlier_crop(source);
    let local = p.crop.unwrap_or(CropRect {
        x: 0,
        y: 0,
        width: p.source_size.0,
        height: p.source_size.1,
    });
    // Section in the coordinates of the original frame.
    let crop = match &earlier {
        Some(prior) => Some((
            CropRect {
                x: prior.section.x + local.x,
                y: prior.section.y + local.y,
                ..local
            },
            prior.original_size,
        )),
        None => p.crop.map(|rect| (rect, p.source_size)),
    };
    let sha = earlier
        .as_ref()
        .and_then(|prior| prior.sha256.clone())
        .unwrap_or_else(|| p.source_sha256.to_owned());
    let name = match &earlier {
        Some(prior) => prior.file_name.clone(),
        None => p
            .source_file_name
            .filter(|n| safe_name(n))
            .map(str::to_owned),
    };
    let mut cards = vec![
        format_card(
            "MO_ACT",
            Value::Str(p.action),
            Some("rust_fits_auto_cropping product action"),
        )?,
        format_card(
            "MO_CROP",
            Value::Logical(crop.is_some()),
            Some("T: cropped from the original; see MO_SEC"),
        )?,
    ];
    if let Some((rect, (width, height))) = crop {
        cards.push(format_card(
            "MO_SEC",
            Value::Str(&crop_section(rect)),
            Some("kept section of original image (1-based)"),
        )?);
        cards.push(format_card(
            "MO_ONAX1",
            Value::Int(width as i64),
            Some("NAXIS1 of the original image"),
        )?);
        cards.push(format_card(
            "MO_ONAX2",
            Value::Int(height as i64),
            Some("NAXIS2 of the original image"),
        )?);
        cards.push(format_card(
            "MO_X0",
            Value::Int(rect.x as i64),
            Some("zero-based crop origin in original image"),
        )?);
        cards.push(format_card(
            "MO_Y0",
            Value::Int(rect.y as i64),
            Some("zero-based crop origin in original image"),
        )?);
    }
    cards.push(format_card(
        "MO_UNDF",
        Value::Int(p.undefined_added as i64),
        Some("samples set undefined by this product"),
    )?);
    cards.push(format_card(
        "MO_ALGO",
        Value::Str(p.algorithm),
        Some("detector algorithm"),
    )?);
    let program = format!(
        "{} v{}",
        env!("CARGO_PKG_NAME"),
        env!("RUST_FITS_AUTO_CROPPING_VERSION")
    );
    cards.push(format_card(
        "MO_PROG",
        Value::Str(&program),
        Some("software that created this product"),
    )?);
    cards.push(format_card(
        "MO_DATE",
        Value::Str(p.created_utc),
        Some("UTC product creation time"),
    )?);
    // 64 hex digits fill the card: no comment.
    cards.push(format_card("MO_SHA", Value::Str(&sha), None)?);
    if let Some(name) = name {
        cards.push(format_card("MO_FILE", Value::Str(&name), None)?);
    }
    Ok(cards)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(text: &str) -> Card {
        let mut c = [b' '; 80];
        c[..text.len()].copy_from_slice(text.as_bytes());
        c
    }

    #[test]
    fn cards_round_trip_and_keep_comments() {
        let c = format_card("CRPIX1", Value::Float(-18.0), Some("reference pixel")).unwrap();
        assert_eq!(&c[..31], b"CRPIX1  =                -18.0 ");
        assert_eq!(numeric_value(&c), Some(-18.0));
        assert_eq!(numeric_comment(&c).as_deref(), Some("reference pixel"));
        for v in [
            1e-7,
            123_456_789.123_456_78,
            -0.5,
            1e300,
            2.5e-310,
            0.1 + 0.2,
        ] {
            let c = format_card("X", Value::Float(v), None).unwrap();
            assert_eq!(numeric_value(&c), Some(v), "{v}");
        }
        let s = format_card("MO_SEC", Value::Str("[1:2,3:4]"), Some("c")).unwrap();
        assert_eq!(&s[..25], b"MO_SEC  = '[1:2,3:4]' / c");
        assert!(format_card("bad", Value::Int(1), None).is_err());
        assert_eq!(numeric_value(&card("CRPIX2  =   1.5D2 / x")), Some(150.0));
    }

    #[test]
    fn assemble_edits_only_allowed_cards_and_verify_accepts_it() {
        let source = vec![
            card("SIMPLE  =                    T"),
            card("BITPIX  =                   16"),
            card("NAXIS   =                    2"),
            card("NAXIS1  =                  100 / length of axis 1"),
            card("NAXIS2  =                   50"),
            card("CRPIX1  =  50.5 / odd formatting kept unless shifted"),
            card("CRPIX2A =                 25.0"),
            card("DATE    = '2001-01-01'"),
            card("HISTORY original history"),
            card("CHECKSUM= 'abc'"),
            card("MO_ACT  = 'old'"),
        ];
        let edits = HeaderEdits {
            width: 40,
            height: 30,
            shift: Some((10, 5)),
            blank: Some(-32768),
            mo_cards: vec![card("MO_ACT  = 'crop'")],
        };
        let out = assemble(&source, &edits).unwrap();
        let keys: Vec<String> = out.iter().map(card_key).collect();
        assert_eq!(
            keys,
            [
                "SIMPLE", "BITPIX", "NAXIS", "NAXIS1", "NAXIS2", "BLANK", "CRPIX1", "CRPIX2A",
                "DATE", "HISTORY", "MO_ACT"
            ]
        );
        assert_eq!(numeric_value(&out[3]), Some(40.0));
        assert_eq!(
            numeric_comment(&out[3]).as_deref(),
            Some("length of axis 1")
        );
        assert_eq!(numeric_value(&out[6]), Some(40.5));
        assert_eq!(numeric_value(&out[7]), Some(20.0));
        assert_eq!(out[8], source[7]);
        assert_eq!(out[9], source[8]);
        verify(&source, &out, &edits).unwrap();
        // A changed preserved card is caught.
        let mut tampered = out.clone();
        tampered[8] = card("DATE    = '2002-01-01'");
        assert!(verify(&source, &tampered, &edits).is_err());
        // Masks do not shift CRPIX.
        let mask = HeaderEdits {
            width: 100,
            height: 50,
            shift: None,
            blank: None,
            mo_cards: vec![],
        };
        let out = assemble(&source, &mask).unwrap();
        assert_eq!(out[5], source[5]);
        verify(&source, &out, &mask).unwrap();
    }

    #[test]
    fn finished_hdu_has_valid_checksums_and_parses() {
        let cards = vec![
            card("SIMPLE  =                    T"),
            card("BITPIX  =                   16"),
            card("NAXIS   =                    2"),
            card("NAXIS1  =                    3"),
            card("NAXIS2  =                    2"),
        ];
        let data: Vec<u8> = [1i16, -2, 3, 4, 5, -6]
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .collect();
        let bytes = finish_hdu(cards, data).unwrap();
        let file = fitskit::FitsFile::from_reader(&mut std::io::Cursor::new(bytes)).unwrap();
        file.primary().verify_datasum().unwrap();
    }

    #[test]
    fn mo_cards_compose_recrops_with_the_original() {
        let p = |crop| Provenance {
            action: "crop",
            algorithm: "alg",
            undefined_added: 0,
            crop,
            source_size: (90, 40),
            source_sha256: "b",
            source_file_name: Some("x.fits"),
            created_utc: "2026-01-01T00:00:00",
        };
        let first = mo_cards(
            &[],
            &p(Some(CropRect {
                x: 10,
                y: 0,
                width: 90,
                height: 40,
            })),
        )
        .unwrap();
        let again = mo_cards(
            &first,
            &p(Some(CropRect {
                x: 5,
                y: 2,
                width: 20,
                height: 30,
            })),
        )
        .unwrap();
        let text: Vec<String> = again
            .iter()
            .map(|c| String::from_utf8_lossy(c).into_owned())
            .collect();
        assert!(
            text.iter()
                .any(|t| t.starts_with("MO_SEC  = '[16:35,3:32]'"))
        );
        assert!(
            text.iter()
                .any(|t| t.starts_with("MO_ONAX1=                   90"))
        );
        // A mask of a cropped product stays identified as a crop of the original.
        let masked = mo_cards(
            &first,
            &Provenance {
                action: "mask",
                ..p(None)
            },
        )
        .unwrap();
        let earlier = earlier_crop(&masked).unwrap();
        assert_eq!(
            earlier.section,
            CropRect {
                x: 10,
                y: 0,
                width: 90,
                height: 40
            }
        );
        assert_eq!(earlier.file_name.as_deref(), Some("x.fits"));
        assert!(earlier_crop(&mo_cards(&[], &p(None)).unwrap()).is_none());
    }
}
