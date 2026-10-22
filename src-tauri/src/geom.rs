//! Pixel geometry shared by capture and readers, and where the readouts are on screen. Crops live only in memory.

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

/// Where the readouts are on the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Boxes {
    pub exp: Rect,
    pub map: Rect,
    /// The "LV." number.
    pub level: Rect,
}

/// The game's own resolution: its EXP font is drawn one screen pixel per font pixel
/// here (measured 2026-10-22). Full screen, the game stretches this to the monitor.
pub const NATIVE: (u32, u32) = (1366, 768);

/// The readouts at NATIVE, measured on the game full screen (2026-10-22). The EXP box
/// reaches about 20 px past the readout measured (`52,718[8.05%]`, x 749–826), so a
/// longer number still fits; nothing else is drawn there.
const NATIVE_BOXES: Boxes = Boxes {
    exp: Rect { x: 747, y: 738, w: 100, h: 10 },
    map: Rect { x: 46, y: 24, w: 243, h: 38 },
    level: Rect { x: 321, y: 740, w: 40, h: 21 },
};

/// How far a screen's shape may be from the game's 16:9 (1366x768 itself is 0.04% off).
const ASPECT_TOLERANCE: f64 = 0.01;

/// The boxes on a `width`×`height` screen with the game full screen, or `None` when the
/// screen isn't 16:9: the game then stretches its text unevenly, and the EXP reader
/// assumes square font pixels.
pub fn boxes_for(width: u32, height: u32) -> Option<Boxes> {
    let (nw, nh) = (f64::from(NATIVE.0), f64::from(NATIVE.1));
    let aspect = f64::from(width) / f64::from(height.max(1));
    if (aspect / (16.0 / 9.0) - 1.0).abs() > ASPECT_TOLERANCE {
        return None;
    }
    let (sx, sy) = (f64::from(width) / nw, f64::from(height) / nh);
    // Rounded outward, so a box never loses an edge of what it was drawn around.
    let scale = |r: Rect| {
        let (x0, y0) = ((f64::from(r.x) * sx).floor() as u32, (f64::from(r.y) * sy).floor() as u32);
        let x1 = ((f64::from(r.x + r.w) * sx).ceil() as u32).min(width);
        let y1 = ((f64::from(r.y + r.h) * sy).ceil() as u32).min(height);
        Rect { x: x0, y: y0, w: x1.saturating_sub(x0), h: y1.saturating_sub(y0) }
    };
    Some(Boxes { exp: scale(NATIVE_BOXES.exp), map: scale(NATIVE_BOXES.map), level: scale(NATIVE_BOXES.level) })
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
    fn boxes_scale_to_16_9_screens_only() {
        assert_eq!(boxes_for(NATIVE.0, NATIVE.1), Some(NATIVE_BOXES));
        // Within a few pixels of the boxes drawn by hand at 1920x1080 (x 1053–1185, y 1037–1053).
        let exp = boxes_for(1920, 1080).expect("16:9").exp;
        assert_eq!((exp.x, exp.x + exp.w, exp.y, exp.y + exp.h), (1049, 1191, 1037, 1052));
        assert_eq!(boxes_for(1280, 960), None);
    }
}
