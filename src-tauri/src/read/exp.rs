//! EXP readout reader: `326 [0.10%]` → ExpReading. Template matching, not OCR:
//! Windows OCR misreads this font (`[0.10%]` → `to. 10%)`, verified 2026-10-15).

use serde::Serialize;

use super::glyphs::{Template, TEMPLATES};
use super::ReadError;
use crate::geom::Crop;

/// Text pixels have luminance ≥ this (measured on the 2026-10-15 fixture).
pub const TEXT_LUM_MIN: u32 = 150;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpReading {
    pub exp: u64,
    /// Percent in basis points: 0.10% → 10.
    pub pct_bp: u32,
}

/// One glyph cut from the text line: rows of '#'/'.', and its left column in the crop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Glyph {
    pub x: u32,
    pub rows: Vec<String>,
}

fn is_text(px: [u8; 4]) -> bool {
    let [r, g, b, _] = px.map(u32::from);
    (299 * r + 587 * g + 114 * b) / 1000 >= TEXT_LUM_MIN
}

/// Trim the line to its text rows, then split at empty columns.
pub fn segment(crop: &Crop) -> Result<Vec<Glyph>, ReadError> {
    let (w, h) = (crop.width, crop.height);
    let mask: Vec<bool> = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).map(|(x, y)| is_text(crop.px(x, y))).collect();
    let at = |x: u32, y: u32| mask[(y * w + x) as usize];
    let text_rows: Vec<u32> = (0..h).filter(|&y| (0..w).any(|x| at(x, y))).collect();
    let (Some(&top), Some(&bottom)) = (text_rows.first(), text_rows.last()) else {
        return Err(ReadError::Blank);
    };
    let column_has_text = |x: u32| (top..=bottom).any(|y| at(x, y));

    let mut glyphs = Vec::new();
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
        let rows = (top..=bottom)
            .map(|y| (start..x).map(|gx| if at(gx, y) { '#' } else { '.' }).collect())
            .collect();
        glyphs.push(Glyph { x: start, rows });
    }
    Ok(glyphs)
}

/// The template whose rows equal the glyph's exactly. No near matches: a shape
/// the templates don't have fails the read instead of becoming a wrong digit.
pub fn classify(glyph: &Glyph, templates: &[Template]) -> Option<char> {
    templates.iter().find(|t| t.rows.iter().eq(glyph.rows.iter())).map(|t| t.ch)
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

pub fn read_exp(crop: &Crop) -> Result<ExpReading, ReadError> {
    let glyphs = segment(crop)?;
    let chars: Vec<Option<char>> = glyphs.iter().map(|g| classify(g, TEMPLATES)).collect();
    if let Some(i) = chars.iter().position(Option::is_none) {
        let line = chars.iter().map(|c| c.unwrap_or('?')).collect();
        return Err(ReadError::UnknownGlyph { column: glyphs[i].x, line });
    }
    parse(&chars.into_iter().flatten().collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::fixture;

    #[test]
    fn reads_the_fixture() {
        let reading = read_exp(&fixture("exp_326_0.10.png")).expect("fixture is readable");
        assert_eq!(reading, ExpReading { exp: 326, pct_bp: 10 });
    }

    #[test]
    fn parse_readouts() {
        assert_eq!(parse("1,234,567[45.67%]"), Ok(ExpReading { exp: 1_234_567, pct_bp: 4567 }));
        assert!(parse("326[0.10]").is_err());
    }
}
