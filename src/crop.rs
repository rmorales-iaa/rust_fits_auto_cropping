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

    pub fn bounds(self) -> CropRect {
        self
    }

    pub fn retained_pixels(self) -> usize {
        self.area()
    }

    pub fn shape_name(self) -> &'static str {
        "rectangle"
    }

    pub fn retains(self, x: usize, y: usize) -> bool {
        let rect = self.bounds();
        x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height
    }
}

/// Choose the maximum-area axis-aligned rectangle containing only defined,
/// unmasked samples. Optimality is exact for this mask and geometry family;
/// it does not certify that the detector mask matches scientific ground truth.
pub fn largest_clean_region(mask: &[bool], width: usize, height: usize) -> Option<CropRect> {
    largest_unmasked_rectangle(mask, width, height)
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
    fn maximal_rectangle_matches_exhaustive_small_grid_oracle() {
        let (width, height) = (3, 3);
        for pattern in 0_u16..(1 << (width * height)) {
            let mask: Vec<bool> = (0..width * height)
                .map(|index| pattern & (1 << index) != 0)
                .collect();
            let mut expected = 0;
            for y0 in 0..height {
                for x0 in 0..width {
                    for h in 1..=height - y0 {
                        for w in 1..=width - x0 {
                            let clean =
                                (y0..y0 + h).all(|y| (x0..x0 + w).all(|x| !mask[y * width + x]));
                            if clean {
                                expected = expected.max(w * h);
                            }
                        }
                    }
                }
            }
            let actual = largest_unmasked_rectangle(&mask, width, height)
                .map(CropRect::area)
                .unwrap_or(0);
            assert_eq!(actual, expected, "pattern={pattern:09b}");
        }
    }

    #[test]
    fn production_crop_selector_returns_only_the_maximal_clean_rectangle() {
        let (width, height) = (11, 11);
        let mut mask = vec![true; width * height];
        for y in 0..height {
            for x in 0..width {
                if (x as f64 - 5.0).powi(2) + (y as f64 - 5.0).powi(2) <= 25.0 {
                    mask[y * width + x] = false;
                }
            }
        }
        let selected = largest_clean_region(&mask, width, height).unwrap();
        let rect = selected;
        assert!(rect.area() > 0);
        assert!(
            (rect.y..rect.y + rect.height)
                .all(|y| (rect.x..rect.x + rect.width).all(|x| !mask[y * width + x]))
        );
        assert_eq!(
            rect.area(),
            largest_unmasked_rectangle(&mask, width, height)
                .unwrap()
                .area()
        );
    }
}
