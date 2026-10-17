//! Level readout reader: the number after "LV." in the status bar, white digits
//! on orange tiles. There are no built-in references, because only the digits of
//! the current level are ever on screen. Instead the app learns: when the level
//! box shows a digit it doesn't know while the level is already known from the
//! EXP readings, that digit is labelled from the level and stored (as ASCII art,
//! like the EXP references) for next time. See `learn` and pipeline.rs.

use super::exp::{centred, nearest, to_art, Glyph};
use super::ReadError;
use crate::geom::Crop;

/// Digit pixels are white: every channel at least this bright (the tiles are orange).
const WHITE_MIN: u8 = 200;
/// Ink 0 at this whiteness (the tiles' blue channel is ≈ 0x10–0x20), 1 at 255.
const WHITE_FLOOR: f32 = 32.0;

/// A learned digit: its character and centred ink as ASCII art.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LevelGlyph {
    pub ch: char,
    pub rows: Vec<String>,
}

fn whiteness(px: [u8; 4]) -> u8 {
    px[0].min(px[1]).min(px[2])
}

/// Cut the white digits out of the level box, left to right.
pub fn segment(crop: &Crop) -> Result<Vec<Glyph>, ReadError> {
    let (w, h) = (crop.width, crop.height);
    let is_text = |x: u32, y: u32| whiteness(crop.px(x, y)) >= WHITE_MIN;
    let text_rows: Vec<u32> = (0..h).filter(|&y| (0..w).any(|x| is_text(x, y))).collect();
    let (Some(&top), Some(&bottom)) = (text_rows.first(), text_rows.last()) else {
        return Err(ReadError::Blank);
    };
    let column_has_text = |x: u32| (top..=bottom).any(|y| is_text(x, y));
    let ink = |x: u32, y: u32| ((f32::from(whiteness(crop.px(x, y))) - WHITE_FLOOR) / (255.0 - WHITE_FLOOR)).clamp(0.0, 1.0);

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

/// The level, if every digit matches a learned one.
pub fn read_level(glyphs: &[Glyph], learned: &[LevelGlyph]) -> Option<u32> {
    let digits: Option<String> =
        glyphs.iter().map(|g| nearest(g, learned.iter().map(|l| (l.ch, l.rows.as_slice())))).collect();
    digits?.parse().ok()
}

/// New digits to store, labelled from `level` (known from the EXP readings).
/// Learns nothing if the digit count differs or a glyph already reads as a
/// different digit, since then the box isn't showing `level`.
pub fn learn(glyphs: &[Glyph], learned: &[LevelGlyph], level: u32) -> Result<Vec<LevelGlyph>, String> {
    let digits: Vec<char> = level.to_string().chars().collect();
    if digits.len() != glyphs.len() {
        return Err(format!("level box has {} glyph(s), level {level} has {} digit(s)", glyphs.len(), digits.len()));
    }
    let mut new = Vec::new();
    for (glyph, &ch) in glyphs.iter().zip(&digits) {
        match nearest(glyph, learned.iter().map(|l| (l.ch, l.rows.as_slice()))) {
            Some(read) if read == ch => {}
            Some(read) => return Err(format!("a glyph reads as {read:?}, but level {level} has {ch:?} there")),
            None => new.push(LevelGlyph { ch, rows: to_art(&centred(glyph)) }),
        }
    }
    Ok(new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::fixture;

    #[test]
    fn learns_digits_from_the_known_level_then_reads_them() {
        let glyphs = segment(&fixture("level_41.png")).expect("digits found");
        assert_eq!(read_level(&glyphs, &[]), None);
        assert!(learn(&glyphs, &[], 7).is_err(), "two glyphs can't be level 7");
        let learned = learn(&glyphs, &[], 41).expect("learns 4 and 1");
        assert_eq!(learned.iter().map(|g| g.ch).collect::<String>(), "41");
        assert_eq!(read_level(&glyphs, &learned), Some(41));
        assert!(learn(&glyphs, &learned, 14).is_err(), "glyphs that read as 41 aren't level 14");
    }
}
