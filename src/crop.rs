//! Deterministic largest axis-aligned rectangle that excludes all proposed artifacts.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CropRect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

impl CropRect {
    pub fn area(self) -> usize {
        self.width * self.height
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CropRegion {
    Rectangle(CropRect),
    Circle {
        bounds: CropRect,
        center_x: usize,
        center_y: usize,
        radius: f64,
        retained_pixels: usize,
    },
}

impl CropRegion {
    pub fn bounds(self) -> CropRect {
        match self {
            Self::Rectangle(rect) => rect,
            Self::Circle { bounds, .. } => bounds,
        }
    }

    pub fn retained_pixels(self) -> usize {
        match self {
            Self::Rectangle(rect) => rect.area(),
            Self::Circle {
                retained_pixels, ..
            } => retained_pixels,
        }
    }

    pub fn shape_name(self) -> &'static str {
        match self {
            Self::Rectangle(_) => "rectangle",
            Self::Circle { .. } => "circle",
        }
    }

    pub fn retains(self, x: usize, y: usize) -> bool {
        match self {
            Self::Rectangle(rect) => {
                x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height
            }
            Self::Circle {
                center_x,
                center_y,
                radius,
                ..
            } => {
                (x as f64 - center_x as f64).powi(2) + (y as f64 - center_y as f64).powi(2)
                    <= radius * radius
            }
        }
    }
}

/// Choose the geometry that retains the most unmasked sample centers.
/// Rectangle wins ties; both candidates are derived exclusively from the pixel mask.
pub fn largest_clean_region(mask: &[bool], width: usize, height: usize) -> Option<CropRegion> {
    let rectangle = largest_unmasked_rectangle(mask, width, height)?;
    let Some(circle) = largest_unmasked_circle(mask, width, height) else {
        return Some(CropRegion::Rectangle(rectangle));
    };
    if circle.retained_pixels > rectangle.area() {
        Some(CropRegion::Circle {
            bounds: circle.bounds,
            center_x: circle.center_x,
            center_y: circle.center_y,
            radius: circle.radius,
            retained_pixels: circle.retained_pixels,
        })
    } else {
        Some(CropRegion::Rectangle(rectangle))
    }
}

#[derive(Clone, Copy, Debug)]
struct CleanCircle {
    bounds: CropRect,
    center_x: usize,
    center_y: usize,
    radius: f64,
    retained_pixels: usize,
}

fn largest_unmasked_circle(mask: &[bool], width: usize, height: usize) -> Option<CleanCircle> {
    if width == 0 || height == 0 || width.checked_mul(height) != Some(mask.len()) {
        return None;
    }
    if !mask.iter().any(|value| *value) || mask.iter().all(|value| *value) {
        return None;
    }

    // Exact squared Euclidean distance transform to the nearest candidate pixel.
    let mut vertical = vec![f64::INFINITY; mask.len()];
    let mut input = vec![f64::INFINITY; height.max(width)];
    let mut output = vec![f64::INFINITY; height.max(width)];
    for x in 0..width {
        for y in 0..height {
            input[y] = if mask[y * width + x] {
                0.0
            } else {
                f64::INFINITY
            };
        }
        squared_distance_transform(&input[..height], &mut output[..height]);
        for y in 0..height {
            vertical[y * width + x] = output[y];
        }
    }
    let mut distances = vec![f64::INFINITY; mask.len()];
    for y in 0..height {
        let start = y * width;
        squared_distance_transform(
            &vertical[start..start + width],
            &mut distances[start..start + width],
        );
    }

    // Pixel centers must stay at least half a pixel from frame edges and
    // conservatively clear the half-diagonal of every masked pixel footprint.
    let half_diagonal = std::f64::consts::FRAC_1_SQRT_2;
    let mut best: Option<(f64, f64, usize, usize)> = None;
    for y in 0..height {
        for x in 0..width {
            if mask[y * width + x] {
                continue;
            }
            let distance = distances[y * width + x].sqrt() - half_diagonal;
            let edge_distance = (x as f64 + 0.5)
                .min(width as f64 - x as f64 - 0.5)
                .min(y as f64 + 0.5)
                .min(height as f64 - y as f64 - 0.5);
            let radius = distance.min(edge_distance);
            let center_distance = (x as f64 - (width as f64 - 1.0) / 2.0).powi(2)
                + (y as f64 - (height as f64 - 1.0) / 2.0).powi(2);
            let candidate = (radius, -center_distance, y, x);
            if best.is_none_or(|old| {
                candidate.0 > old.0
                    || (candidate.0 == old.0
                        && (candidate.1, candidate.2, candidate.3) > (old.1, old.2, old.3))
            }) {
                best = Some(candidate);
            }
        }
    }
    let (radius, _, center_y, center_x) = best?;
    if !radius.is_finite() || radius <= 0.0 {
        return None;
    }
    let x0 = ((center_x as f64 - radius).ceil() as usize).min(width);
    let y0 = ((center_y as f64 - radius).ceil() as usize).min(height);
    let x1 = ((center_x as f64 + radius).floor() as usize + 1).min(width);
    let y1 = ((center_y as f64 + radius).floor() as usize + 1).min(height);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let bounds = CropRect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    };
    let radius_squared = radius * radius;
    let mut retained_pixels = 0usize;
    for y in y0..y1 {
        for x in x0..x1 {
            if (x as f64 - center_x as f64).powi(2) + (y as f64 - center_y as f64).powi(2)
                <= radius_squared
            {
                if mask[y * width + x] {
                    return None;
                }
                retained_pixels += 1;
            }
        }
    }
    Some(CleanCircle {
        bounds,
        center_x,
        center_y,
        radius,
        retained_pixels,
    })
}

fn squared_distance_transform(input: &[f64], output: &mut [f64]) {
    let sites: Vec<usize> = input
        .iter()
        .enumerate()
        .filter_map(|(index, value)| value.is_finite().then_some(index))
        .collect();
    if sites.is_empty() {
        output.fill(f64::INFINITY);
        return;
    }
    let mut vertices = vec![0usize; sites.len()];
    let mut boundaries = vec![0.0f64; sites.len() + 1];
    let mut k = 0usize;
    vertices[0] = sites[0];
    boundaries[0] = f64::NEG_INFINITY;
    boundaries[1] = f64::INFINITY;
    for &q in sites.iter().skip(1) {
        let mut intersection = parabola_intersection(input, q, vertices[k]);
        while intersection <= boundaries[k] && k > 0 {
            k -= 1;
            intersection = parabola_intersection(input, q, vertices[k]);
        }
        if intersection <= boundaries[k] {
            vertices[0] = q;
            boundaries[0] = f64::NEG_INFINITY;
            boundaries[1] = f64::INFINITY;
            k = 0;
        } else {
            k += 1;
            vertices[k] = q;
            boundaries[k] = intersection;
            boundaries[k + 1] = f64::INFINITY;
        }
    }
    let last = k;
    k = 0;
    for (q, result) in output.iter_mut().enumerate() {
        while k < last && boundaries[k + 1] < q as f64 {
            k += 1;
        }
        let delta = q as f64 - vertices[k] as f64;
        *result = delta * delta + input[vertices[k]];
    }
}

fn parabola_intersection(input: &[f64], q: usize, p: usize) -> f64 {
    let q = q as f64;
    let p = p as f64;
    (input[q as usize] + q * q - input[p as usize] - p * p) / (2.0 * (q - p))
}

/// Find the maximum-area rectangle containing no `true` pixels.
/// Ties choose the topmost then leftmost rectangle for reproducible output.
pub fn largest_unmasked_rectangle(mask: &[bool], width: usize, height: usize) -> Option<CropRect> {
    if width == 0 || height == 0 || width.checked_mul(height) != Some(mask.len()) {
        return None;
    }
    let mut heights = vec![0usize; width];
    let mut best: Option<(usize, usize, usize, usize, usize)> = None;
    for y in 0..height {
        for x in 0..width {
            heights[x] = if mask[y * width + x] {
                0
            } else {
                heights[x] + 1
            };
        }
        let mut stack: Vec<(usize, usize)> = Vec::new();
        for x in 0..=width {
            let current = heights.get(x).copied().unwrap_or(0);
            let mut start = x;
            while stack.last().is_some_and(|(_, h)| *h > current) {
                let (left, h) = stack.pop().expect("stack checked above");
                let area = h * (x - left);
                let top = y + 1 - h;
                let candidate = (area, top, left, x, y + 1);
                if best.is_none_or(|old| {
                    candidate.0 > old.0
                        || (candidate.0 == old.0 && (candidate.1, candidate.2) < (old.1, old.2))
                }) {
                    best = Some(candidate);
                }
                start = left;
            }
            if stack.last().is_none_or(|(_, h)| *h < current) {
                stack.push((start, current));
            }
        }
    }
    let (area, y, x, right, bottom) = best?;
    (area > 0).then_some(CropRect {
        x,
        y,
        width: right - x,
        height: bottom - y,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_expected_rectangle_around_four_corner_pattern() {
        let (width, height) = (8, 6);
        let mut mask = vec![false; width * height];
        for y in 0..2 {
            for x in 0..2 {
                mask[y * width + x] = true;
                mask[y * width + width - 1 - x] = true;
                mask[(height - 1 - y) * width + x] = true;
                mask[(height - 1 - y) * width + width - 1 - x] = true;
            }
        }
        assert_eq!(
            largest_unmasked_rectangle(&mask, width, height),
            Some(CropRect {
                x: 2,
                y: 0,
                width: 4,
                height: 6
            })
        );
    }

    #[test]
    fn preserves_entire_image_when_there_are_no_artifacts() {
        assert_eq!(
            largest_unmasked_rectangle(&[false; 12], 4, 3),
            Some(CropRect {
                x: 0,
                y: 0,
                width: 4,
                height: 3
            })
        );
    }

    #[test]
    fn rejects_empty_or_mismatched_geometry() {
        assert_eq!(largest_unmasked_rectangle(&[], 0, 0), None);
        assert_eq!(largest_unmasked_rectangle(&[false], 2, 2), None);
    }

    #[test]
    fn circular_field_retains_more_pixels_than_its_clean_rectangle() {
        let (width, height) = (128, 128);
        let mut mask = vec![true; width * height];
        for y in 0..height {
            for x in 0..width {
                if (x as f64 - 63.5).powi(2) + (y as f64 - 63.5).powi(2) <= 56.0_f64.powi(2) {
                    mask[y * width + x] = false;
                }
            }
        }
        let region = largest_clean_region(&mask, width, height).unwrap();
        assert!(matches!(region, CropRegion::Circle { .. }));
        let CropRegion::Circle {
            center_x,
            center_y,
            radius,
            retained_pixels,
            ..
        } = region
        else {
            unreachable!()
        };
        assert!(
            retained_pixels
                > largest_unmasked_rectangle(&mask, width, height)
                    .unwrap()
                    .area()
        );
        assert!(!mask[center_y * width + center_x]);
        assert!(radius > 50.0);
    }
}
