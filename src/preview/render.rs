//! Side-by-side native-scale grayscale panels for accepted crops.
use crate::{crop::CropRegion, preview::zscale::ZScale};
use anyhow::{Context, bail};
use image::{GrayImage, ImageFormat, Luma};
use std::{fs, path::Path};
use tempfile::NamedTempFile;

const LABEL_HEIGHT: usize = 18;

/// Write a labeled source/crop PNG using one source-derived ZScale interval.
pub fn write_crop_preview(
    destination: &Path,
    values: &[f64],
    source_width: usize,
    source_height: usize,
    region: CropRegion,
) -> anyhow::Result<()> {
    if source_width.checked_mul(source_height) != Some(values.len()) {
        bail!("preview source dimensions do not match the pixel array");
    }
    let bounds = region.bounds();
    if bounds.width == 0
        || bounds.height == 0
        || bounds
            .x
            .checked_add(bounds.width)
            .is_none_or(|x| x > source_width)
        || bounds
            .y
            .checked_add(bounds.height)
            .is_none_or(|y| y > source_height)
    {
        bail!("preview crop bounds exceed the source image");
    }
    let scale = ZScale::from_values(values).context("source has no finite pixels for ZScale")?;
    let canvas_width = source_width
        .checked_add(bounds.width)
        .context("preview width overflow")?;
    let content_height = source_height.max(bounds.height);
    let canvas_height = content_height
        .checked_add(LABEL_HEIGHT)
        .context("preview height overflow")?;
    let width = u32::try_from(canvas_width).context("preview width exceeds PNG dimensions")?;
    let height = u32::try_from(canvas_height).context("preview height exceeds PNG dimensions")?;
    let mut canvas = GrayImage::from_pixel(width, height, Luma([0]));
    draw_label(&mut canvas, 8, 5, "BEFORE");
    draw_label(&mut canvas, source_width as u32 + 8, 5, "CROP");

    for y in 0..source_height {
        let row = y * source_width;
        let png_y = LABEL_HEIGHT + source_height - 1 - y;
        for x in 0..source_width {
            let value = values[row + x];
            if value.is_finite() {
                canvas.put_pixel(x as u32, png_y as u32, Luma([scale.to_u8(value)]));
            }
        }
    }
    for crop_y in 0..bounds.height {
        let source_y = bounds.y + crop_y;
        let png_y = LABEL_HEIGHT + bounds.height - 1 - crop_y;
        let row = source_y * source_width;
        for crop_x in 0..bounds.width {
            let source_x = bounds.x + crop_x;
            if !region.retains(source_x, source_y) {
                continue;
            }
            let value = values[row + source_x];
            if value.is_finite() {
                canvas.put_pixel(
                    (source_width + crop_x) as u32,
                    png_y as u32,
                    Luma([scale.to_u8(value)]),
                );
            }
        }
    }

    let parent = destination.parent().context("preview path has no parent")?;
    fs::create_dir_all(parent)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    image::DynamicImage::ImageLuma8(canvas).write_to(&mut temporary, ImageFormat::Png)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(destination)
        .map_err(|error| error.error)?;
    Ok(())
}

fn draw_label(image: &mut GrayImage, start_x: u32, start_y: u32, text: &str) {
    for (index, character) in text.chars().enumerate() {
        let x0 = start_x + (index as u32) * 6;
        for (y, row) in glyph(character).iter().enumerate() {
            for x in 0..5 {
                if row & (1 << (4 - x)) != 0 {
                    let x = x0 + x;
                    let y = start_y + y as u32;
                    if x < image.width() && y < image.height() {
                        image.put_pixel(x, y, Luma([255]));
                    }
                }
            }
        }
    }
}

fn glyph(character: char) -> [u8; 7] {
    match character {
        'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'C' => [
            0b01111, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b01111,
        ],
        'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        _ => [0; 7],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crop::CropRect;
    use tempfile::tempdir;
    #[test]
    fn creates_labeled_before_and_after_png_with_lower_origin() {
        let temp = tempdir().unwrap();
        let output = temp.path().join("subdir/frame.fits.png");
        let width = 24;
        let height = 12;
        let values: Vec<f64> = (0..width * height).map(|value| value as f64).collect();
        let region = CropRegion::Rectangle(CropRect {
            x: 4,
            y: 2,
            width: 12,
            height: 6,
        });
        write_crop_preview(&output, &values, width, height, region).unwrap();

        let rendered = image::open(&output).unwrap().to_luma8();
        assert_eq!(rendered.dimensions(), (36, 30));
        assert_eq!(rendered.get_pixel(0, 29).0[0], 0); // FITS bottom-left is PNG bottom-left.
        assert!(rendered.get_pixel(8, 5).0[0] > 0); // BEFORE label.
        assert!(rendered.get_pixel(width as u32 + 9, 5).0[0] > 0); // CROP label.
        assert_eq!(
            rendered.get_pixel(4, 27).0[0],
            rendered.get_pixel(24, 23).0[0]
        );
        assert_eq!(rendered.get_pixel(24, 29).0[0], 0); // outside the selected crop.
    }
}
