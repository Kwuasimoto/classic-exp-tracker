//! EXP readout reader: `326 [0.10%]` → ExpReading. Font matching, not OCR: Windows
//! OCR misreads this font (`[0.10%]` → `to. 10%)`, verified 2026-10-15). The readout
//! is cut into glyphs here and each is matched against the game's font (font.rs) at
//! whatever scale the screen shows it.

use serde::Serialize;

use super::font::{self, Patch, Placement};
use super::ReadError;
use crate::geom::Crop;

/// Pixels with luminance ≥ this are text; used to find the text rows and the
/// gaps between glyphs (measured on the 2026-10-15 fixture).
pub const TEXT_LUM_MIN: u32 = 150;
/// A digit and the gap after it are 6 font pixels wide; the widest glyph, '%', is 7.
/// A run of text columns wider than MAX_RUN_CELLS font pixels is glyphs that touch
/// (on a friend's screen, 2026-10-21, the decimals of `47.08%` had no dark column
/// between them). It is cut into parts of about one pitch each, as beanguri's
/// Maple-Classic-Exp-Overlay (MIT) does, at the darkest column near each cut.
const PITCH_CELLS: f32 = 6.0;
const MAX_RUN_CELLS: f32 = 7.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpReading {
    pub exp: u64,
    /// Percent in basis points: 0.10% → 10.
    pub pct_bp: u32,
}

/// One glyph of the readout and how it read.
#[derive(Clone, Debug, PartialEq)]
pub struct Glyph {
    /// Its left column in the crop and its width.
    pub x: u32,
    pub width: u32,
    /// Cut from a run of glyphs that touched; logged.
    pub split: bool,
    /// The brightness around it, as matched.
    pub patch: Patch,
    /// The closest font characters with their scores, best first (for the log).
    pub ranked: Vec<(char, f32)>,
    /// What it reads as: `None` when no character is close enough or clearly closest.
    pub ch: Option<char>,
}

/// The readout's glyphs, and the scale they were read at.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub glyphs: Vec<Glyph>,
    /// Rows of text, and the screen pixels per font pixel that makes.
    pub text_height: u32,
}

impl Line {
    pub fn font_px(&self) -> f32 {
        self.text_height as f32 / font::ROWS as f32
    }

    /// The text read, with '?' for each glyph not recognised.
    pub fn text(&self) -> String {
        self.glyphs.iter().map(|g| g.ch.unwrap_or('?')).collect()
    }
}

fn lum(px: [u8; 4]) -> u32 {
    let [r, g, b, _] = px.map(u32::from);
    (299 * r + 587 * g + 114 * b) / 1000
}

/// Find the text rows, cut them into glyphs at columns without text (and runs too wide
/// for one glyph by the font's pitch), and match each glyph against the font.
pub fn read_line(crop: &Crop) -> Result<Line, ReadError> {
    let (w, h) = (crop.width, crop.height);
    let lums: Vec<Vec<u32>> = (0..h).map(|y| (0..w).map(|x| lum(crop.px(x, y))).collect()).collect();
    let is_text = |x: u32, y: u32| lums[y as usize][x as usize] >= TEXT_LUM_MIN;
    let text_rows: Vec<u32> = (0..h).filter(|&y| (0..w).any(|x| is_text(x, y))).collect();
    let (Some(&top), Some(&bottom)) = (text_rows.first(), text_rows.last()) else {
        return Err(ReadError::Blank);
    };
    let text_height = bottom - top + 1;
    let p = text_height as f32 / font::ROWS as f32;
    let column_has_text = |x: u32| (top..=bottom).any(|y| is_text(x, y));
    let peak = |x: u32| (top..=bottom).map(|y| lums[y as usize][x as usize]).max().unwrap_or(0);

    let mut runs = Vec::new();
    let mut x = 0;
    while x < w {
        if !column_has_text(x) {
            x += 1;
            continue;
        }
        let start = x;
        while x < w && column_has_text(x) {
            x += 1;
        }
        let width = (x - start) as f32;
        if width <= MAX_RUN_CELLS * p {
            runs.push((start..x, false));
            continue;
        }
        // Touching glyphs: cut near each multiple of the run's share of the pitch, at
        // the darkest column there. The cut column belongs to neither part.
        let parts = ((width + p) / (PITCH_CELLS * p)).round().max(2.0) as u32;
        let mut from = start;
        for k in 1..parts {
            let target = start as f32 + width * k as f32 / parts as f32;
            let lo = (target - p).floor().max(from as f32 + 1.0) as u32;
            let hi = ((target + p).ceil() as u32).min(x - 2);
            let Some(cut) = (lo..=hi).min_by_key(|&c| peak(c)) else { break };
            runs.push((from..cut, true));
            from = cut + 1;
        }
        runs.push((from..x, true));
    }

    let (y0, y1) = (top.saturating_sub(1), (bottom + 2).min(h));
    let glyphs = runs
        .into_iter()
        .map(|(run, split)| {
            let (x0, x1) = (run.start.saturating_sub(1), (run.end + 1).min(w));
            let patch = Patch {
                width: (x1 - x0) as usize,
                height: (y1 - y0) as usize,
                lums: (y0..y1).flat_map(|y| (x0..x1).map(move |x| (y, x))).map(|(y, x)| lums[y as usize][x as usize] as f32).collect(),
            };
            let at = Placement {
                left: (run.start - x0) as f32,
                width: run.len() as f32,
                text_top: (top - y0) as f32,
                text_height: text_height as f32,
            };
            let ranked = font::rank(&patch, at);
            let ch = font::decide(&ranked);
            Glyph { x: run.start, width: run.len() as u32, split, patch, ranked, ch }
        })
        .collect();
    Ok(Line { glyphs, text_height })
}

/// Parse `<digits/commas>[<int>.<2 digits>%]`.
pub fn parse(text: &str) -> Result<ExpReading, ReadError> {
    let bad = || ReadError::Parse(text.to_string());
    let (number, rest) = text.split_once('[').ok_or_else(bad)?;
    let pct = rest.strip_suffix("%]").ok_or_else(bad)?;
    let (whole, frac) = pct.split_once('.').ok_or_else(bad)?;
    let digits: String = number.chars().filter(|&c| c != ',').collect();
    let all_digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !all_digits(&digits) || !all_digits(whole) || frac.len() != 2 || !all_digits(frac) {
        return Err(bad());
    }
    let exp = digits.parse::<u64>().map_err(|_| bad())?;
    let whole = whole.parse::<u32>().map_err(|_| bad())?;
    let frac = frac.parse::<u32>().map_err(|_| bad())?;
    let pct_bp = whole.checked_mul(100).and_then(|v| v.checked_add(frac)).ok_or_else(bad)?;
    if pct_bp > 10_000 {
        return Err(bad());
    }
    Ok(ExpReading { exp, pct_bp })
}

/// The reading a line spells, if every glyph is recognised.
pub fn reading(line: &Line) -> Result<ExpReading, ReadError> {
    if let Some(glyph) = line.glyphs.iter().find(|g| g.ch.is_none()) {
        return Err(ReadError::UnknownGlyph { column: glyph.x, line: line.text() });
    }
    parse(&line.text())
}

pub fn read_exp(crop: &Crop) -> Result<ExpReading, ReadError> {
    reading(&read_line(crop)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::fixture;
    use image::imageops::{resize, FilterType};

    #[test]
    fn reads_the_fixture() {
        let reading = read_exp(&fixture("exp_326_0.10.png")).expect("fixture is readable");
        assert_eq!(reading, ExpReading { exp: 326, pct_bp: 10 });
    }

    #[test]
    fn reads_a_stretched_screen() {
        // A monitor stretching the game: 1080p shown at 1440p (smoothed), and ×2 (blocky).
        let base = fixture("exp_326_0.10.png").to_image();
        for (num, den, filter) in [(4, 3, FilterType::Triangle), (2, 1, FilterType::Nearest)] {
            let stretched = Crop::from_image(&resize(&base, base.width() * num / den, base.height() * num / den, filter));
            assert_eq!(read_exp(&stretched), Ok(ExpReading { exp: 326, pct_bp: 10 }), "×{num}/{den}");
        }
    }

    #[test]
    fn glyphs_that_touch_are_split() {
        // Bridge the gap between '3' and '2' with a dim pixel, as the game drew the
        // decimals of `47.08%` on a friend's screen: no dark column between them.
        let mut crop = fixture("exp_326_0.10.png");
        let text_rows: Vec<u32> = (0..crop.height).filter(|&y| (0..crop.width).any(|x| lum(crop.px(x, y)) >= TEXT_LUM_MIN)).collect();
        let middle = text_rows[text_rows.len() / 2];
        let gap = read_line(&crop).expect("text found").glyphs[1].x - 1;
        let i = (middle * crop.width + gap) as usize * 4;
        crop.rgba[i..i + 3].copy_from_slice(&[170, 170, 170]);
        let line = read_line(&crop).expect("text found");
        assert_eq!(line.glyphs.len(), 10, "split back into 10 glyphs");
        assert!(line.glyphs[0].split);
        assert_eq!(reading(&line), Ok(ExpReading { exp: 326, pct_bp: 10 }));
    }

    #[test]
    fn parse_readouts() {
        assert_eq!(parse("1,234,567[45.67%]"), Ok(ExpReading { exp: 1_234_567, pct_bp: 4567 }));
        assert!(parse("326[0.10]").is_err());
    }
}
