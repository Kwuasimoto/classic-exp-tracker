//! EXP readout reader: `326 [0.10%]` → ExpReading. Template matching, not OCR:
//! Windows OCR misreads this font (`[0.10%]` → `to. 10%)`, verified 2026-10-15).

use serde::Serialize;

use super::glyphs::{Template, TEMPLATES};
use super::ReadError;
use crate::geom::Crop;

/// Text pixels have luminance ≥ this (measured on the 2026-10-15 fixture).
pub const TEXT_LUM_MIN: u32 = 150;
/// Max share of differing pixels for a glyph to match a template.
const MAX_MISMATCH_RATIO: f32 = 0.15;

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

fn cell(rows: &[&[u8]], col: usize, row: usize) -> bool {
    rows.get(row).and_then(|r| r.get(col)).is_some_and(|&b| b == b'#')
}

/// Differing pixels between two glyphs, top-aligned, trying both horizontal
/// alignments when widths differ by one. `None` when sizes are too different.
fn mismatches(a: &[String], b: &[&str]) -> Option<(usize, usize)> {
    let a: Vec<&[u8]> = a.iter().map(|r| r.as_bytes()).collect();
    let b: Vec<&[u8]> = b.iter().map(|r| r.as_bytes()).collect();
    let (wa, wb) = (a.first()?.len(), b.first()?.len());
    if wa.abs_diff(wb) > 1 || a.len().abs_diff(b.len()) > 2 {
        return None;
    }
    let (width, height) = (wa.max(wb), a.len().max(b.len()));
    let shifts: &[(usize, usize)] = if wa == wb { &[(0, 0)] } else if wa < wb { &[(0, 0), (1, 0)] } else { &[(0, 0), (0, 1)] };
    shifts
        .iter()
        .map(|&(shift_a, shift_b)| {
            (0..height)
                .flat_map(|row| (0..width).map(move |col| (row, col)))
                .filter(|&(row, col)| {
                    let pa = col.checked_sub(shift_a).is_some_and(|c| cell(&a, c, row));
                    let pb = col.checked_sub(shift_b).is_some_and(|c| cell(&b, c, row));
                    pa != pb
                })
                .count()
        })
        .min()
        .map(|n| (n, width * height))
}

/// Nearest template character, if it is close enough and strictly better than
/// the best template of any other character.
pub fn classify(glyph: &Glyph, templates: &[Template]) -> Option<char> {
    let mut best_per_char: Vec<(char, usize, usize)> = Vec::new();
    for t in templates {
        let Some((n, area)) = mismatches(&glyph.rows, t.rows) else { continue };
        match best_per_char.iter_mut().find(|(c, _, _)| *c == t.ch) {
            Some(entry) if n < entry.1 => *entry = (t.ch, n, area),
            Some(_) => {}
            None => best_per_char.push((t.ch, n, area)),
        }
    }
    best_per_char.sort_by_key(|&(_, n, _)| n);
    let &(ch, n, area) = best_per_char.first()?;
    let close_enough = (n as f32) <= MAX_MISMATCH_RATIO * area as f32;
    let unambiguous = best_per_char.get(1).is_none_or(|&(_, second, _)| second > n);
    (close_enough && unambiguous).then_some(ch)
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
    let text = segment(crop)?
        .iter()
        .map(|g| classify(g, TEMPLATES).ok_or(ReadError::UnknownGlyph(g.x)))
        .collect::<Result<String, _>>()?;
    parse(&text)
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
