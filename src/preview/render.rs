//! Side-by-side crop-fit previews with color-coded source-panel boundaries.
use crate::{crop::CropRect, durability::sync_parent, preview::zscale::ZScale};
use anyhow::{Context, bail};
use image::{ImageFormat, Rgb, RgbImage};
use std::{fs, path::Path};
use tempfile::NamedTempFile;

const LABEL_HEIGHT: usize = 18;

/// Write a labeled source/crop PNG with an independently fitted ZScale interval
/// for the source and the final crop, as image viewers do for separate images.
pub fn write_crop_preview(
    destination: &Path,
    values: &[f64],
    source_width: usize,
    source_height: usize,
    region: CropRect,
    interior_undefined: Option<&[bool]>,
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
    if interior_undefined.is_some_and(|mask| mask.len() != values.len()) {
        bail!("preview interior mask dimensions do not match the pixel array");
    }
    // Samples set undefined in the crop product render as undefined (black)
    // and are excluded from the crop ZScale, matching the FITS product.
    let product_value = |index: usize| {
        if interior_undefined.is_some_and(|mask| mask[index]) {
            f64::NAN
        } else {
            values[index]
        }
    };
    let source_scale =
        ZScale::from_values(values).context("source has no finite pixels for ZScale")?;
    let crop_scale = ZScale::from_iter_factory(|| {
        (bounds.y..bounds.y + bounds.height).flat_map(|y| {
            let start = y * source_width + bounds.x;
            (start..start + bounds.width).map(product_value)
        })
    })
    .context("final crop has no finite pixels for ZScale")?;
    // Fit the after-panel to the full source-panel height while preserving its
    // aspect ratio. Nearest-neighbor sampling is display-only and invents no
    // intermediate FITS sample values.
    let display_crop_height = source_height;
    let display_crop_width = usize::try_from(
        (bounds.width as u128 * display_crop_height as u128 + bounds.height as u128 / 2)
            / bounds.height as u128,
    )
    .context("preview crop width exceeds addressable dimensions")?;
    let canvas_width = source_width
        .checked_add(display_crop_width)
        .context("preview width overflow")?;
    let canvas_height = source_height
        .checked_add(LABEL_HEIGHT)
        .context("preview height overflow")?;
    let width = u32::try_from(canvas_width).context("preview width exceeds PNG dimensions")?;
    let height = u32::try_from(canvas_height).context("preview height exceeds PNG dimensions")?;
    let mut canvas = RgbImage::from_pixel(width, height, Rgb([0, 0, 0]));
    draw_label(&mut canvas, 8, 5, "BEFORE");
    draw_label(&mut canvas, source_width as u32 + 8, 5, "CROP FIT");

    for y in 0..source_height {
        let row = y * source_width;
        let png_y = LABEL_HEIGHT + source_height - 1 - y;
        for x in 0..source_width {
            let value = values[row + x];
            if value.is_finite() {
                let pixel = source_scale.to_u8(value);
                canvas.put_pixel(x as u32, png_y as u32, Rgb([pixel, pixel, pixel]));
            }
            if let Some(color) = crop_boundary_color(region, x, y, source_width, source_height) {
                canvas.put_pixel(x as u32, png_y as u32, color);
            }
        }
    }
    for display_y in 0..display_crop_height {
        let crop_y = (((2 * display_y + 1) as u128 * bounds.height as u128)
            / (2 * display_crop_height as u128)) as usize;
        let png_y = LABEL_HEIGHT + display_crop_height - 1 - display_y;
        for display_x in 0..display_crop_width {
            let crop_x = (((2 * display_x + 1) as u128 * bounds.width as u128)
                / (2 * display_crop_width as u128)) as usize;
            let source_x = bounds.x + crop_x;
            let source_y = bounds.y + crop_y;
            let value = product_value(source_y * source_width + source_x);
            if value.is_finite() {
                canvas.put_pixel((source_width + display_x) as u32, png_y as u32, {
                    let pixel = crop_scale.to_u8(value);
                    Rgb([pixel, pixel, pixel])
                });
            }
        }
    }

    let parent = destination.parent().context("preview path has no parent")?;
    fs::create_dir_all(parent)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    image::DynamicImage::ImageRgb8(canvas).write_to(&mut temporary, ImageFormat::Png)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(destination)
        .map_err(|error| error.error)?;
    sync_parent(destination)?;
    Ok(())
}

fn crop_boundary_color(
    region: CropRect,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
) -> Option<Rgb<u8>> {
    let retained = region.retains(x, y);
    let mut retained_neighbor = false;
    let mut cropped_neighbor = false;
    for dy in -1_isize..=1 {
        for dx in -1_isize..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let neighbor_x = x as isize + dx;
            let neighbor_y = y as isize + dy;
            if neighbor_x < 0
                || neighbor_y < 0
                || neighbor_x >= width as isize
                || neighbor_y >= height as isize
            {
                continue;
            }
            if region.retains(neighbor_x as usize, neighbor_y as usize) {
                retained_neighbor = true;
            } else {
                cropped_neighbor = true;
            }
        }
    }
    match (retained, retained_neighbor, cropped_neighbor) {
        (true, _, true) => Some(Rgb([0, 255, 0])),
        (false, true, _) => Some(Rgb([255, 0, 0])),
        _ => None,
    }
}

fn draw_label(image: &mut RgbImage, start_x: u32, start_y: u32, text: &str) {
    for (index, character) in text.chars().enumerate() {
        let x0 = start_x + (index as u32) * 6;
        for (y, row) in glyph(character).iter().enumerate() {
            for x in 0..5 {
                if row & (1 << (4 - x)) != 0 {
                    let x = x0 + x;
                    let y = start_y + y as u32;
                    if x < image.width() && y < image.height() {
                        image.put_pixel(x, y, Rgb([255, 255, 255]));
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
        'I' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111,
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
        'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
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
    fn creates_labeled_preview_with_green_kept_and_red_cropped_edges() {
        let temp = tempdir().unwrap();
        let output = temp.path().join("subdir/frame.fits.png");
        let width = 24;
        let height = 12;
        let values: Vec<f64> = (0..width * height).map(|value| value as f64).collect();
        let region = CropRect {
            x: 4,
            y: 2,
            width: 12,
            height: 6,
        };
        write_crop_preview(&output, &values, width, height, region, None).unwrap();

        let rendered = image::open(&output).unwrap().to_rgb8();
        assert_eq!(rendered.dimensions(), (48, 30));
        assert_eq!(rendered.get_pixel(0, 29).0, [0, 0, 0]); // FITS bottom-left is PNG bottom-left.
        assert_eq!(rendered.get_pixel(8, 5).0, [255, 255, 255]); // BEFORE label.
        assert_eq!(rendered.get_pixel(width as u32 + 9, 5).0, [255, 255, 255]); // CROP label.
        let green_y = LABEL_HEIGHT as u32 + height as u32 - 1 - 4;
        assert_eq!(rendered.get_pixel(4, green_y).0, [0, 255, 0]);
        assert_eq!(rendered.get_pixel(3, green_y).0, [255, 0, 0]);
        let shared_sample = values[4 * width + 8];
        let crop_values: Vec<f64> = (2..8)
            .flat_map(|y| values[y * width + 4..y * width + 16].iter().copied())
            .collect();
        let crop_scale = ZScale::from_values(&crop_values).unwrap();
        let crop_gray = crop_scale.to_u8(shared_sample);
        assert_eq!(
            rendered.get_pixel(24 + 9, LABEL_HEIGHT as u32 + 11 - 5).0,
            [crop_gray; 3]
        );
    }

    #[test]
    fn crop_panel_uses_its_own_zscale_interval() {
        let temp = tempdir().unwrap();
        let output = temp.path().join("crop.png");
        let (width, height) = (20, 10);
        let mut values: Vec<f64> = (0..width * height).map(|index| index as f64).collect();
        let region = CropRect {
            x: 10,
            y: 0,
            width: 10,
            height: 10,
        };
        // The selected crop samples occupy a distinct range from the source
        // extrema, making shared and crop-derived stretches observably differ.
        for y in 0..height {
            for x in 10..20 {
                values[y * width + x] = 10.0 + (y * 10 + x - 10) as f64;
            }
        }
        write_crop_preview(&output, &values, width, height, region, None).unwrap();
        let rendered = image::open(&output).unwrap().to_rgb8();
        let source_scale = ZScale::from_values(&values).unwrap();
        let mut crop_samples = Vec::new();
        for y in 0..height {
            for x in 10..20 {
                crop_samples.push(values[y * width + x]);
            }
        }
        let crop_scale = ZScale::from_values(&crop_samples).unwrap();
        let crop_center_x = width as u32 + 5;
        let crop_center_y = LABEL_HEIGHT as u32 + height as u32 - 1 - 5;
        let actual_crop = rendered.get_pixel(crop_center_x, crop_center_y).0[0];
        let expected_crop = crop_scale.to_u8(values[5 * width + 15]);
        let shared_scale_value = source_scale.to_u8(values[5 * width + 15]);
        assert_eq!(actual_crop, expected_crop);
        assert_ne!(actual_crop, shared_scale_value);
    }
}
