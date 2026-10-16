//! Pixel geometry shared by capture, readers and calibration. Crops live only in memory.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    /// True when the rect is non-empty and lies entirely inside a `width`×`height` image.
    pub fn fits_in(&self, width: u32, height: u32) -> bool {
        self.w > 0
            && self.h > 0
            && self.x.checked_add(self.w).is_some_and(|right| right <= width)
            && self.y.checked_add(self.h).is_some_and(|bottom| bottom <= height)
    }
}

/// Calibrated regions for one monitor resolution.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Calibration {
    pub exp: Rect,
    pub map: Rect,
}

pub fn resolution_key(width: u32, height: u32) -> String {
    format!("{width}x{height}")
}

/// Owned RGBA8 pixels, row-major, no row padding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Crop {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Crop {
    /// `None` when `rgba` doesn't hold exactly `width * height` RGBA pixels.
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Option<Self> {
        let expected = (width as usize).checked_mul(height as usize)?.checked_mul(4)?;
        (rgba.len() == expected).then_some(Self { width, height, rgba })
    }

    /// Pixel at (x, y). Callers iterate within `width`/`height`; out of bounds is a programmer error.
    pub fn px(&self, x: u32, y: u32) -> [u8; 4] {
        assert!(x < self.width && y < self.height, "pixel ({x},{y}) outside {}x{}", self.width, self.height);
        let i = (y as usize * self.width as usize + x as usize) * 4;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2], self.rgba[i + 3]]
    }

    /// Copy of the sub-rectangle, or `None` when it doesn't fit.
    pub fn sub(&self, r: Rect) -> Option<Crop> {
        if !r.fits_in(self.width, self.height) {
            return None;
        }
        let mut rgba = Vec::with_capacity(r.w as usize * r.h as usize * 4);
        for y in r.y..r.y + r.h {
            let start = (y as usize * self.width as usize + r.x as usize) * 4;
            rgba.extend_from_slice(&self.rgba[start..start + r.w as usize * 4]);
        }
        Some(Crop { width: r.w, height: r.h, rgba })
    }

    pub fn to_image(&self) -> image::RgbaImage {
        image::RgbaImage::from_raw(self.width, self.height, self.rgba.clone())
            .expect("Crop invariant: rgba length matches dimensions")
    }

    pub fn from_image(img: &image::RgbaImage) -> Self {
        Self { width: img.width(), height: img.height(), rgba: img.as_raw().clone() }
    }
}

#[cfg(test)]
pub fn fixture(name: &str) -> Crop {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    let img = image::open(&path)
        .unwrap_or_else(|e| panic!("fixture {}: {e}", path.display()))
        .to_rgba8();
    Crop::from_image(&img)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_in_rejects_empty_and_overflowing_rects() {
        assert!(Rect { x: 0, y: 0, w: 10, h: 10 }.fits_in(10, 10));
        assert!(!Rect { x: 1, y: 0, w: 10, h: 10 }.fits_in(10, 10));
        assert!(!Rect { x: 0, y: 0, w: 0, h: 10 }.fits_in(10, 10));
        assert!(!Rect { x: u32::MAX, y: 0, w: 2, h: 1 }.fits_in(10, 10));
    }

    #[test]
    fn new_rejects_wrong_buffer_length() {
        assert!(Crop::new(2, 2, vec![0; 16]).is_some());
        assert!(Crop::new(2, 2, vec![0; 15]).is_none());
    }

    #[test]
    fn sub_copies_the_right_pixels() {
        // 3x2 image where each pixel's red channel is its index.
        let rgba: Vec<u8> = (0..6u8).flat_map(|i| [i, 0, 0, 255]).collect();
        let crop = Crop::new(3, 2, rgba).expect("valid buffer");
        let sub = crop.sub(Rect { x: 1, y: 0, w: 2, h: 2 }).expect("fits");
        assert_eq!((sub.width, sub.height), (2, 2));
        assert_eq!([sub.px(0, 0)[0], sub.px(1, 0)[0], sub.px(0, 1)[0], sub.px(1, 1)[0]], [1, 2, 4, 5]);
        assert!(crop.sub(Rect { x: 2, y: 0, w: 2, h: 1 }).is_none());
    }

    #[test]
    fn resolution_key_format() {
        assert_eq!(resolution_key(1920, 1080), "1920x1080");
    }
}
