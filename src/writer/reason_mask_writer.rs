//! FITS serialization and sample verification for the optional action-reason plane.
use super::{
    is_structural_or_data_summary, normalize_history_cards, publish_fits, push_history,
    stamp_processing_metadata,
};
use anyhow::bail;
use fitskit::{FitsFile, HduData, Header, HeaderValue, ImageData, PixelData};
use std::path::Path;

/// Write a uint8 bookkeeping plane; codes are not astrophysical validity labels.
pub fn write_reason_mask(
    source_header: &Header,
    path: &Path,
    width: usize,
    height: usize,
    codes: &[u8],
) -> anyhow::Result<()> {
    if codes.len() != width.saturating_mul(height) || codes.iter().any(|code| *code > 3) {
        bail!("reason-plane dimensions or codes are invalid");
    }
    let mut output = FitsFile::with_primary_image(ImageData::new(
        vec![width, height],
        PixelData::U8(codes.to_vec()),
    ));
    for keyword in source_header.iter() {
        if !is_structural_or_data_summary(&keyword.name) && keyword.name != "BUNIT" {
            output.primary_mut().header.push(keyword.clone());
        }
    }
    normalize_history_cards(&mut output.primary_mut().header);
    stamp_processing_metadata(&mut output.primary_mut().header);
    output.primary_mut().header.set(
        "RSCHEME",
        HeaderValue::String("PIXACT1".into()),
        Some("pixel action reason codes; not science validity"),
    );
    for definition in [
        "Reason code 0: defined input pixel retained by selected action",
        "Reason code 1: excluded by accepted detector artifact mask",
        "Reason code 2: defined pixel sacrificed by crop geometry only",
        "Reason code 3: input sample already undefined before processing",
    ] {
        push_history(
            &mut output.primary_mut().header,
            &format!("rust_fits_auto_cropping: {definition}"),
        );
    }
    publish_fits(&output, path)?;
    verify_reason_mask_file(path, width, height, codes)
}

fn verify_reason_mask_file(
    path: &Path,
    width: usize,
    height: usize,
    expected: &[u8],
) -> anyhow::Result<()> {
    let check = FitsFile::from_file(path)?;
    if check.len() != 1 {
        bail!("written reason plane does not contain exactly one HDU");
    }
    check.primary().verify_datasum()?;
    if check.primary().header.get_string("RSCHEME") != Some("PIXACT1") {
        bail!("written reason plane has no recognized code scheme");
    }
    let actual = match &check.primary().data {
        HduData::Image(img) if img.axes == [width, height] => match &img.pixels {
            PixelData::U8(values) => values,
            _ => bail!("reason-plane BITPIX changed from 8"),
        },
        HduData::Image(_) => bail!("reason-plane dimensions changed during write"),
        _ => bail!("reason plane primary HDU is not an image"),
    };
    if actual != expected {
        bail!("reason-plane samples changed during write");
    }
    Ok(())
}
