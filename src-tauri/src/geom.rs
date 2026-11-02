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
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Boxes {
    pub exp: Rect,
    pub map: Rect,
    /// The "LV." number.
    pub level: Rect,
    /// Where the minimap's collapsed title bar shows its text: the game collapses the
    /// minimap in shops (map_strip::is_collapsed_minimap).
    pub bar: Rect,
    /// The item inventory, pushed into the screen's top-right corner (read::inventory).
    pub inventory: Rect,
    /// The quickslot beside the chat, either layout (read::inventory::read_quickslot).
    pub quickslot: Rect,
    /// The chat box at its tallest, its input bar and toggle button (read::chat).
    pub chat: Rect,
    /// Where the inventory's and the quickslot's native pixels land in their boxes: the
    /// readers measure everything inside them in native pixels.
    pub inventory_at: Scale,
    pub quickslot_at: Scale,
}

/// How a box's native pixels land in its crop: `sx`×`sy` screen pixels per native pixel,
/// the box's native top-left at (`ox`, `oy`) in the crop (under a pixel in, as boxes_for
/// rounds boxes outward). The game stretches its 1366x768 picture to the screen bilinearly
/// (measured on a 1920x1080 frame, 2026-10-23: native item patches stretched that way match
/// the screen within 0.3 per channel), so what's inside is placed by scaling its native
/// position.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scale {
    pub sx: f64,
    pub sy: f64,
    pub ox: f64,
    pub oy: f64,
}

impl Scale {
    /// One screen pixel per native pixel, the box exactly on its crop.
    pub const NATIVE: Scale = Scale { sx: 1.0, sy: 1.0, ox: 0.0, oy: 0.0 };

    /// Screen pixels per font pixel, for the fonts (their pixels are square: rows decide).
    pub fn font_px(&self) -> f32 {
        self.sy as f32
    }

    /// Native `r` (inside the box) in crop pixels: left, top, right, bottom edges.
    fn edges(&self, r: Rect) -> (f64, f64, f64, f64) {
        let x = |n: u32| self.ox + f64::from(n) * self.sx;
        let y = |n: u32| self.oy + f64::from(n) * self.sy;
        (x(r.x), y(r.y), x(r.x + r.w), y(r.y + r.h))
    }

    /// The crop pixels that native `r` touches, and where its top-left falls in them.
    pub fn around(&self, crop: &Crop, r: Rect) -> Option<(Crop, f32, f32)> {
        let (x0, y0, x1, y1) = self.edges(r);
        let (left, top) = (x0.floor(), y0.floor());
        let area = Rect { x: left as u32, y: top as u32, w: (x1.ceil() - left) as u32, h: (y1.ceil() - top) as u32 };
        Some((crop.sub(area)?, (x0 - left) as f32, (y0 - top) as f32))
    }

    /// The crop pixels wholly inside native `r`: none of what's around it smoothed in.
    pub fn inside(&self, crop: &Crop, r: Rect) -> Option<Crop> {
        let (x0, y0, x1, y1) = self.edges(r);
        let (left, top) = (x0.ceil(), y0.ceil());
        crop.sub(Rect { x: left as u32, y: top as u32, w: (x1.floor() - left).max(0.0) as u32, h: (y1.floor() - top).max(0.0) as u32 })
    }

    /// The crop pixel under the centre of native pixel (`x`, `y`).
    pub fn pixel(&self, crop: &Crop, x: u32, y: u32) -> Option<[u8; 4]> {
        let cx = (self.ox + (f64::from(x) + 0.5) * self.sx).floor() as u32;
        let cy = (self.oy + (f64::from(y) + 0.5) * self.sy).floor() as u32;
        (cx < crop.width && cy < crop.height).then(|| crop.px(cx, cy))
    }

    /// Native `r`'s own pixels (RGB, row by row), taken back out of the stretched screen:
    /// each screen pixel is a known bilinear mix of the native pixels round its centre, so
    /// the native pixels are the least-squares fit to the screen pixels over them. At native
    /// they're the crop's pixels unchanged. `None` when `r` isn't inside the crop.
    pub fn native_rgb(&self, crop: &Crop, r: Rect) -> Option<Vec<u8>> {
        let (xs, from_x) = unstretch(self.sx, self.ox, r.x, r.w, crop.width)?;
        let (ys, from_y) = unstretch(self.sy, self.oy, r.y, r.h, crop.height)?;
        // Across first: each screen row of the samples, as native columns.
        let across: Vec<Vec<[f64; 3]>> = ys
            .iter()
            .map(|&y| {
                (1..=r.w as usize)
                    .map(|nx| {
                        let mut v = [0.0; 3];
                        for (&x, &w) in xs.iter().zip(&from_x[nx]) {
                            let px = crop.px(x, y);
                            (0..3).for_each(|c| v[c] += w * f64::from(px[c]));
                        }
                        v
                    })
                    .collect()
            })
            .collect();
        // Then down, the native pixel round its rows' weights.
        let mut out = Vec::with_capacity(r.w as usize * r.h as usize * 3);
        for weights in &from_y[1..=r.h as usize] {
            for nx in 0..r.w as usize {
                for c in 0..3 {
                    let v: f64 = across.iter().zip(weights).map(|(row, w)| w * row[nx][c]).sum();
                    out.push(v.round().clamp(0.0, 255.0) as u8);
                }
            }
        }
        Some(out)
    }
}

/// One axis of Scale::native_rgb, for native pixels `a`..`a + n` with one more either side
/// (the screen pixels at the run's edges mix those in): the screen pixels sampling only
/// those native pixels, and for each of the n + 2 native pixels the weights that rebuild it
/// from them (least squares). `None` when the crop doesn't hold them, or they don't decide
/// every native pixel.
fn unstretch(s: f64, o: f64, a: u32, n: u32, len: u32) -> Option<(Vec<u32>, Vec<Vec<f64>>)> {
    // Screen pixel j's centre samples native position u, native pixel k's centre being at k.
    let u = |j: u32| (f64::from(j) + 0.5 - o) / s - 0.5;
    let (lo, hi) = (f64::from(a) - 1.0, f64::from(a + n));
    let samples: Vec<u32> = (0..len).filter(|&j| (lo..=hi).contains(&u(j))).collect();
    let k = n as usize + 2;
    // Each sample's mix of the k native pixels.
    let mix: Vec<Vec<f64>> = samples
        .iter()
        .map(|&j| {
            let v = u(j) - lo;
            let (i, t) = (v.floor() as usize, v - v.floor());
            let mut row = vec![0.0; k];
            row[i] += 1.0 - t;
            if t > 0.0 {
                row[i + 1] += t;
            }
            row
        })
        .collect();
    // Normal equations: (MᵀM) W = Mᵀ, solved by elimination with partial pivoting.
    let mut lhs: Vec<Vec<f64>> = (0..k).map(|p| (0..k).map(|q| mix.iter().map(|row| row[p] * row[q]).sum()).collect()).collect();
    let mut rhs: Vec<Vec<f64>> = (0..k).map(|p| mix.iter().map(|row| row[p]).collect()).collect();
    for col in 0..k {
        let pivot = (col..k).max_by(|&x, &y| lhs[x][col].abs().total_cmp(&lhs[y][col].abs()))?;
        if lhs[pivot][col].abs() < 1e-9 {
            return None;
        }
        lhs.swap(col, pivot);
        rhs.swap(col, pivot);
        let (pivot_lhs, pivot_rhs) = (lhs[col].clone(), rhs[col].clone());
        for (row, (l, r)) in lhs.iter_mut().zip(rhs.iter_mut()).enumerate() {
            let f = l[col] / pivot_lhs[col];
            if row == col || f == 0.0 {
                continue;
            }
            l.iter_mut().zip(&pivot_lhs).for_each(|(v, p)| *v -= f * p);
            r.iter_mut().zip(&pivot_rhs).for_each(|(v, p)| *v -= f * p);
        }
    }
    // The left side is diagonal now.
    for (i, row) in rhs.iter_mut().enumerate() {
        let d = lhs[i][i];
        row.iter_mut().for_each(|w| *w /= d);
    }
    Some((samples, rhs))
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
    bar: Rect { x: 46, y: 3, w: 120, h: 13 },
    // The inventory in its default corner (inventory_at); measured 2026-10-23, as is the
    // quickslot, which covers the controller layout's fan and the keyboard layout's 4x2 keys.
    inventory: Rect { x: 1157, y: 0, w: 209, h: 289 },
    quickslot: Rect { x: 860, y: 600, w: 230, h: 100 },
    chat: crate::read::chat::STRIP,
    inventory_at: Scale::NATIVE,
    quickslot_at: Scale::NATIVE,
};

/// The corner the user keeps the item inventory in (the top-left one is the minimap's).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Corner {
    #[default]
    TopRight,
    BottomRight,
    BottomLeft,
}

impl Corner {
    pub fn as_i64(self) -> i64 {
        match self {
            Self::TopRight => 0,
            Self::BottomRight => 1,
            Self::BottomLeft => 2,
        }
    }

    pub fn from_i64(v: i64) -> Option<Self> {
        match v {
            0 => Some(Self::TopRight),
            1 => Some(Self::BottomRight),
            2 => Some(Self::BottomLeft),
            _ => None,
        }
    }
}

/// The inventory window at NATIVE, border to border. In each corner the game puts it flush
/// against both screen edges (measured in all three, 2026-10-23).
const INVENTORY: (u32, u32) = (209, 289);

fn inventory_at(corner: Corner) -> Rect {
    let (w, h) = INVENTORY;
    let (right, bottom) = (NATIVE.0 - w, NATIVE.1 - h);
    let (x, y) = match corner {
        Corner::TopRight => (right, 0),
        Corner::BottomRight => (right, bottom),
        Corner::BottomLeft => (0, bottom),
    };
    Rect { x, y, w, h }
}

/// How far a screen's shape may be from the game's 16:9 (1366x768 itself is 0.04% off).
const ASPECT_TOLERANCE: f64 = 0.01;

/// The boxes on a `width`×`height` screen with the game full screen and the inventory in
/// `corner`, or `None` when the screen isn't 16:9: the game then stretches its text
/// unevenly, and the EXP reader assumes square font pixels.
pub fn boxes_for(width: u32, height: u32, corner: Corner) -> Option<Boxes> {
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
    // How far past its box's (floored) corner a native box's corner lands.
    let place = |r: Rect| {
        let (x, y) = (f64::from(r.x) * sx, f64::from(r.y) * sy);
        Scale { sx, sy, ox: x - x.floor(), oy: y - y.floor() }
    };
    let inventory = inventory_at(corner);
    Some(Boxes {
        exp: scale(NATIVE_BOXES.exp),
        map: scale(NATIVE_BOXES.map),
        level: scale(NATIVE_BOXES.level),
        bar: scale(NATIVE_BOXES.bar),
        inventory: scale(inventory),
        quickslot: scale(NATIVE_BOXES.quickslot),
        chat: scale(NATIVE_BOXES.chat),
        inventory_at: place(inventory),
        quickslot_at: place(NATIVE_BOXES.quickslot),
    })
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
        assert_eq!(boxes_for(NATIVE.0, NATIVE.1, Corner::TopRight), Some(NATIVE_BOXES));
        // Within a few pixels of the boxes drawn by hand at 1920x1080 (x 1053–1185, y 1037–1053).
        let exp = boxes_for(1920, 1080, Corner::TopRight).expect("16:9").exp;
        assert_eq!((exp.x, exp.x + exp.w, exp.y, exp.y + exp.h), (1049, 1191, 1037, 1052));
        assert_eq!(boxes_for(1280, 960, Corner::TopRight), None);
        // The inventory where it was measured in the bottom corners.
        let at = |corner| boxes_for(NATIVE.0, NATIVE.1, corner).map(|b| (b.inventory.x, b.inventory.y));
        assert_eq!((at(Corner::BottomRight), at(Corner::BottomLeft)), (Some((1157, 479)), Some((0, 479))));
    }
}
