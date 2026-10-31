//! EXP readout reader: `326 [0.10%]` → ExpReading. Font matching, not OCR: Windows
//! OCR misreads this font (`[0.10%]` → `to. 10%)`, verified 2026-10-15). The readout
//! is cut into glyphs here and each is matched against the game's font (font.rs) at
//! whatever scale the screen shows it.


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

/// What the readout shows. Below 1,000,000 EXP into a level the game writes the counter and
/// the percent, `326 [0.10%]`; from 1,000,000 on it drops the counter and writes only the
/// percent, `[84.36%]`. Percents are in basis points: 0.10% → 10.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExpReading {
    Counter { exp: u64, pct_bp: u32 },
    Percent { pct_bp: u32 },
}

impl ExpReading {
    pub fn pct_bp(self) -> u32 {
        match self {
            Self::Counter { pct_bp, .. } | Self::Percent { pct_bp } => pct_bp,
        }
    }

    /// The EXP into the level: the counter as read, or the percent of the level's `total`
    /// (rounded down, so gains between two percents add up exactly). `None` for a percent
    /// while the total isn't known.
    pub fn exp_in_level(self, total: Option<u64>) -> Option<u64> {
        match self {
            Self::Counter { exp, .. } => Some(exp),
            Self::Percent { pct_bp } => total.map(|t| u64::try_from(u128::from(t) * u128::from(pct_bp) / 10_000).unwrap_or(u64::MAX)),
        }
    }
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
    rows: usize,
}

impl Line {
    pub fn font_px(&self) -> f32 {
        self.text_height as f32 / self.rows as f32
    }

    /// Whether any glyph was recognised. A line with none isn't the readout: something
    /// else is drawn over the box.
    pub fn is_readout(&self) -> bool {
        self.glyphs.iter().any(|g| g.ch.is_some())
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

/// The EXP readout's line (bright text on the dark bar).
pub fn read_line(crop: &Crop) -> Result<Line, ReadError> {
    read_text_line(crop, &font::EXP, Ink::Light)
}

/// Whether a line's text is brighter or darker than what's behind it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    Light,
    /// Read with brightness inverted, so the text is the bright part.
    Dark,
}

/// Find the text rows, cut them into glyphs at columns without text (and runs too wide
/// for one glyph by the font's pitch), and match each glyph against `font`.
pub fn read_text_line(crop: &Crop, font: &font::Font, ink: Ink) -> Result<Line, ReadError> {
    let (w, h) = (crop.width, crop.height);
    let lums: Vec<Vec<u32>> = (0..h)
        .map(|y| (0..w).map(|x| if ink == Ink::Dark { 255 - lum(crop.px(x, y)) } else { lum(crop.px(x, y)) }).collect())
        .collect();
    let is_text = |x: u32, y: u32| lums[y as usize][x as usize] >= TEXT_LUM_MIN;
    let text_rows: Vec<u32> = (0..h).filter(|&y| (0..w).any(|x| is_text(x, y))).collect();
    let (Some(&top), Some(&bottom)) = (text_rows.first(), text_rows.last()) else {
        return Err(ReadError::Blank);
    };
    let text_height = bottom - top + 1;
    // Screen pixels per font pixel, for cutting glyphs apart. A font with a descent (the
    // meso comma's tail) makes a line its rows tall, or its rows and descent: sized by
    // the taller, a cut never falls inside a glyph. (The EXP font has none.)
    let p = text_height as f32 / (font.rows + font.descent) as f32;
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
            // A run too short to hold a cut (a line only a row or two tall makes `p` tiny,
            // so one column counts as too wide) leaves `lo..=hi` empty and stays whole.
            let hi = ((target + p).ceil() as u32).min(x.saturating_sub(2));
            let Some(cut) = (lo..=hi).min_by_key(|&c| peak(c)) else { break };
            runs.push((from..cut, true));
            from = cut + 1;
        }
        runs.push((from..x, true));
    }
    join_slivers(&mut runs, (font.narrowest() as f32 - 1.0) * p, peak);

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
            let ranked = font::rank(&patch, at, font);
            let ch = font::decide(&ranked);
            Glyph { x: run.start, width: run.len() as u32, split, patch, ranked, ch }
        })
        .collect();
    Ok(Line { glyphs, text_height, rows: font.rows })
}

/// Runs narrower than `sliver` (less than any glyph of the font) are pieces of a glyph:
/// smoothing can leave a column inside one just under TEXT_LUM_MIN (at 1920x1080 the meso
/// total's 8 lost its left stroke so, column 147 against 150, and read as "18"). Each joins
/// the neighbour across the inkier gap; a real gap between glyphs is blank. The EXP font has
/// a 1-column glyph ('.'), so nothing in it is a sliver.
fn join_slivers(runs: &mut Vec<(std::ops::Range<u32>, bool)>, sliver: f32, peak: impl Fn(u32) -> u32) {
    let gap_peak = |from: u32, to: u32| (from..to).map(&peak).max().unwrap_or(0);
    let mut i = 0;
    while i < runs.len() {
        let (run, split) = runs[i].clone();
        if split || run.len() as f32 >= sliver {
            i += 1;
            continue;
        }
        let left = i.checked_sub(1).map(|j| gap_peak(runs[j].0.end, run.start));
        let right = runs.get(i + 1).map(|(next, _)| gap_peak(run.end, next.start));
        let join_left = match (left, right) {
            (None, None) => {
                i += 1;
                continue;
            }
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (Some(l), Some(r)) => l > r,
        };
        if join_left {
            runs[i - 1].0.end = run.end;
        } else {
            runs[i + 1].0.start = run.start;
        }
        runs.remove(i);
    }
}

/// Parse `<digits/commas>[<int>.<2 digits>%]`, or `[<int>.<2 digits>%]` alone (see ExpReading).
pub fn parse(text: &str) -> Result<ExpReading, ReadError> {
    let bad = || ReadError::Parse(text.to_string());
    let (number, rest) = text.split_once('[').ok_or_else(bad)?;
    let pct = rest.strip_suffix("%]").ok_or_else(bad)?;
    let (whole, frac) = pct.split_once('.').ok_or_else(bad)?;
    let digits: String = number.chars().filter(|&c| c != ',').collect();
    let all_digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !(number.is_empty() || all_digits(&digits)) || !all_digits(whole) || frac.len() != 2 || !all_digits(frac) {
        return Err(bad());
    }
    let whole = whole.parse::<u32>().map_err(|_| bad())?;
    let frac = frac.parse::<u32>().map_err(|_| bad())?;
    let pct_bp = whole.checked_mul(100).and_then(|v| v.checked_add(frac)).ok_or_else(bad)?;
    if pct_bp > 10_000 {
        return Err(bad());
    }
    if number.is_empty() {
        return Ok(ExpReading::Percent { pct_bp });
    }
    let exp = digits.parse::<u64>().map_err(|_| bad())?;
    Ok(ExpReading::Counter { exp, pct_bp })
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
        assert_eq!(reading, ExpReading::Counter { exp: 326, pct_bp: 10 });
    }

    #[test]
    fn reads_a_stretched_screen() {
        // A monitor stretching the game: 1080p shown at 1440p (smoothed), and ×2 (blocky).
        let base = fixture("exp_326_0.10.png").to_image();
        for (num, den, filter) in [(4, 3, FilterType::Triangle), (2, 1, FilterType::Nearest)] {
            let stretched = Crop::from_image(&resize(&base, base.width() * num / den, base.height() * num / den, filter));
            assert_eq!(read_exp(&stretched), Ok(ExpReading::Counter { exp: 326, pct_bp: 10 }), "×{num}/{den}");
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
        assert_eq!(reading(&line), Ok(ExpReading::Counter { exp: 326, pct_bp: 10 }));
    }

    #[test]
    fn parse_readouts() {
        assert_eq!(parse("1,234,567[45.67%]"), Ok(ExpReading::Counter { exp: 1_234_567, pct_bp: 4567 }));
        // From 1,000,000 EXP into a level the game shows only the percent.
        assert_eq!(parse("[84.36%]"), Ok(ExpReading::Percent { pct_bp: 8436 }));
        assert!(parse("326[0.10]").is_err());
    }
}
