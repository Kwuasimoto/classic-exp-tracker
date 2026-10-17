//! EXP readout reader: `326 [0.10%]` → ExpReading. Template matching, not OCR:
//! Windows OCR misreads this font (`[0.10%]` → `to. 10%)`, verified 2026-10-15).

use serde::Serialize;

use super::glyphs::REFERENCES;
use super::ReadError;
use crate::geom::Crop;

/// Pixels with luminance ≥ this are text; used to find the text rows and the
/// gaps between glyphs (measured on the 2026-10-15 fixture).
pub const TEXT_LUM_MIN: u32 = 150;
/// Ink 1.0 = this much brighter than the row's background (text ≈ 225, bar ≈ 100).
const INK_RANGE: f32 = 120.0;
/// Glyphs are compared over the centre of brightness −6 … +6 columns.
pub const HALF_WIDTH: i32 = 6;
/// The nearest reference is accepted only if it is at least this many times
/// closer than the nearest reference of another character…
const MIN_MARGIN: f32 = 1.5;
/// …and closer than this (sum of squared ink differences).
const MAX_DISTANCE: f32 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpReading {
    pub exp: u64,
    /// Percent in basis points: 0.10% → 10.
    pub pct_bp: u32,
}

/// One glyph cut from the text line: its left column in the crop and, per row,
/// the ink of each pixel (0.0 = background, 1.0 = full text brightness).
#[derive(Clone, Debug, PartialEq)]
pub struct Glyph {
    pub x: u32,
    pub ink: Vec<Vec<f32>>,
}

fn lum(px: [u8; 4]) -> u32 {
    let [r, g, b, _] = px.map(u32::from);
    (299 * r + 587 * g + 114 * b) / 1000
}

/// Trim the line to its text rows, split it at columns without text, and keep
/// one extra column on each side of a glyph for its anti-aliased edge.
pub fn segment(crop: &Crop) -> Result<Vec<Glyph>, ReadError> {
    let (w, h) = (crop.width, crop.height);
    let lums: Vec<Vec<u32>> = (0..h).map(|y| (0..w).map(|x| lum(crop.px(x, y))).collect()).collect();
    let is_text = |x: u32, y: u32| lums[y as usize][x as usize] >= TEXT_LUM_MIN;
    let text_rows: Vec<u32> = (0..h).filter(|&y| (0..w).any(|x| is_text(x, y))).collect();
    let (Some(&top), Some(&bottom)) = (text_rows.first(), text_rows.last()) else {
        return Err(ReadError::Blank);
    };
    let column_has_text = |x: u32| (top..=bottom).any(|y| is_text(x, y));
    // Most of each row is background, so its median is the background brightness.
    let background: Vec<u32> = lums
        .iter()
        .map(|row| {
            let mut sorted = row.clone();
            sorted.sort_unstable();
            sorted[sorted.len() / 2]
        })
        .collect();
    let ink = |x: u32, y: u32| {
        let above = lums[y as usize][x as usize] as f32 - background[y as usize] as f32;
        (above / INK_RANGE).clamp(0.0, 1.0)
    };

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
        let columns = start.saturating_sub(1)..(x + 1).min(w);
        let ink = (top..=bottom).map(|y| columns.clone().map(|gx| ink(gx, y)).collect()).collect();
        glyphs.push(Glyph { x: start, ink });
    }
    Ok(glyphs)
}

/// The glyph resampled at its centre of brightness −HALF_WIDTH … +HALF_WIDTH
/// columns (linear interpolation), so glyphs drawn at different fractional
/// positions line up with the references.
pub fn centred(glyph: &Glyph) -> Vec<Vec<f32>> {
    let (mut sum, mut weighted) = (0.0, 0.0);
    for row in &glyph.ink {
        for (x, v) in row.iter().enumerate() {
            sum += v;
            weighted += v * (x as f32 + 0.5);
        }
    }
    let width = glyph.ink.first().map_or(0, Vec::len);
    let centre = if sum > 0.0 { weighted / sum } else { width as f32 / 2.0 };
    glyph
        .ink
        .iter()
        .map(|row| {
            let at = |i: i32| usize::try_from(i).ok().and_then(|i| row.get(i)).copied().unwrap_or(0.0);
            (-HALF_WIDTH..=HALF_WIDTH)
                .map(|u| {
                    let p = centre + u as f32 - 0.5;
                    let (x0, f) = (p.floor(), p - p.floor());
                    at(x0 as i32) * (1.0 - f) + at(x0 as i32 + 1) * f
                })
                .collect()
        })
        .collect()
}

/// Ink levels of the ASCII art used for references: ' ' = 0/9 … '@' = 9/9.
const ART_LEVELS: &[u8; 10] = b" .:-=+*#%@";

/// Centred ink as ASCII art, the format of every stored reference.
pub fn to_art(centred: &[Vec<f32>]) -> Vec<String> {
    centred
        .iter()
        .map(|row| row.iter().map(|v| ART_LEVELS[(v * 9.0).round().clamp(0.0, 9.0) as usize] as char).collect())
        .collect()
}

/// Sum of squared ink differences; `None` if the line heights differ.
fn distance(centred: &[Vec<f32>], art: &[impl AsRef<str>]) -> Option<f32> {
    if centred.len() != art.len() {
        return None;
    }
    let level = |b: u8| ART_LEVELS.iter().position(|&l| l == b).unwrap_or(0) as f32 / 9.0;
    Some(
        centred
            .iter()
            .zip(art)
            .flat_map(|(a, r)| a.iter().zip(r.as_ref().bytes()).map(move |(a, r)| (a - level(r)) * (a - level(r))))
            .sum(),
    )
}

/// The nearest reference's character, if it is close and clearly closer than
/// any other character's. Otherwise `None`, and the read fails.
pub fn nearest<'a, R: AsRef<str> + 'a>(glyph: &Glyph, references: impl IntoIterator<Item = (char, &'a [R])>) -> Option<char> {
    let centred = centred(glyph);
    let mut ranked: Vec<(f32, char)> =
        references.into_iter().filter_map(|(ch, art)| distance(&centred, art).map(|d| (d, ch))).collect();
    ranked.sort_by(|a, b| a.0.total_cmp(&b.0));
    let &(best, ch) = ranked.first()?;
    let runner_up = ranked.iter().find(|(_, c)| *c != ch).map_or(f32::INFINITY, |&(d, _)| d);
    (best <= MAX_DISTANCE && runner_up >= MIN_MARGIN * best).then_some(ch)
}

pub fn classify(glyph: &Glyph) -> Option<char> {
    nearest(glyph, REFERENCES.iter().map(|r| (r.ch, r.rows)))
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
    let chars: Vec<Option<char>> = glyphs.iter().map(classify).collect();
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
