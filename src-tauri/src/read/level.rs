//! Level readout reader: the number after "LV." in the status bar, white digits
//! on orange tiles. There are no built-in references, because only the digits of
//! the current level are ever on screen. Instead the app learns: when the level
//! box shows a digit it doesn't know while the level is known from the EXP
//! readings, that digit is labelled from the level and stored (as ASCII art, see
//! `to_art`) for next time. `learn` checks the shapes; pipeline.rs only
//! calls it once the box and the level have been steady across several accepted
//! EXP readings, so a box that changed before the tracker caught up isn't learned.

use super::{LearnedGlyph, ReadError};
use crate::geom::Crop;

/// Digit pixels are white: every channel at least this bright (the tiles are orange).
const WHITE_MIN: u8 = 200;
/// Ink 0 at this whiteness (the tiles' blue channel is ≈ 0x10–0x20), 1 at 255.
const WHITE_FLOOR: f32 = 32.0;

/// Glyphs are compared over the centre of brightness −6 … +6 columns.
const HALF_WIDTH: i32 = 6;
/// The nearest learned digit is accepted only if it is at least this many times
/// closer than the nearest learned digit of another character…
const MIN_MARGIN: f32 = 1.5;
/// …and closer than this (sum of squared ink differences).
const MAX_DISTANCE: f32 = 8.0;

/// One digit cut from the level box: per row, the ink of each pixel (0.0 = background,
/// 1.0 = full text brightness).
#[derive(Clone, Debug, PartialEq)]
pub struct Glyph {
    ink: Vec<Vec<f32>>,
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
        glyphs.push(Glyph { ink });
    }
    Ok(glyphs)
}

/// The glyph resampled at its centre of brightness −HALF_WIDTH … +HALF_WIDTH
/// columns (linear interpolation), so glyphs drawn at different fractional
/// positions line up with the references.
fn centred(glyph: &Glyph) -> Vec<Vec<f32>> {
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

fn art_level(b: u8) -> f32 {
    ART_LEVELS.iter().position(|&l| l == b).unwrap_or(0) as f32 / 9.0
}

/// Centred ink as ASCII art, the format of every stored reference.
fn to_art(centred: &[Vec<f32>]) -> Vec<String> {
    centred
        .iter()
        .map(|row| row.iter().map(|v| ART_LEVELS[(v * 9.0).round().clamp(0.0, 9.0) as usize] as char).collect())
        .collect()
}

/// Sum of squared ink differences; `None` if the line heights differ.
fn distance(centred: &[Vec<f32>], art: &[String]) -> Option<f32> {
    if centred.len() != art.len() {
        return None;
    }
    Some(
        centred
            .iter()
            .zip(art)
            .flat_map(|(a, r)| a.iter().zip(r.bytes()).map(move |(a, r)| (a - art_level(r)) * (a - art_level(r))))
            .sum(),
    )
}

/// The digit `glyph` reads as with the digits learned at this resolution: the nearest
/// learned shape, if it is close and clearly closer than any other character's.
fn classify(glyph: &Glyph, learned: &[LearnedGlyph]) -> Option<char> {
    let centred = centred(glyph);
    let mut ranked: Vec<(f32, char)> = learned.iter().filter_map(|l| distance(&centred, &l.rows).map(|d| (d, l.ch))).collect();
    ranked.sort_by(|a, b| a.0.total_cmp(&b.0));
    let &(best, ch) = ranked.first()?;
    let runner_up = ranked.iter().find(|(_, c)| *c != ch).map_or(f32::INFINITY, |&(d, _)| d);
    (best <= MAX_DISTANCE && runner_up >= MIN_MARGIN * best).then_some(ch)
}

/// The level, if every digit matches a learned one.
pub fn read_level(glyphs: &[Glyph], learned: &[LearnedGlyph]) -> Option<u32> {
    let digits: Option<String> = glyphs.iter().map(|g| classify(g, learned)).collect();
    digits?.parse().ok()
}

/// New digits to store, labelled from `level` (known from the EXP readings).
/// Learns nothing if the digit count differs, a glyph's height differs from the
/// digits already learned (something else is in the box), or a glyph already
/// reads as a different digit (the box isn't showing `level`).
pub fn learn(glyphs: &[Glyph], learned: &[LearnedGlyph], level: u32) -> Result<Vec<LearnedGlyph>, String> {
    let digits: Vec<char> = level.to_string().chars().collect();
    if digits.len() != glyphs.len() {
        return Err(format!("level box has {} glyph(s), level {level} has {} digit(s)", glyphs.len(), digits.len()));
    }
    if let Some(known) = learned.first()
        && glyphs.iter().any(|g| g.ink.len() != known.rows.len())
    {
        return Err(format!("glyphs aren't {} px tall like the digits already learned", known.rows.len()));
    }
    let mut new = Vec::new();
    for (glyph, &ch) in glyphs.iter().zip(&digits) {
        match classify(glyph, learned) {
            Some(read) if read == ch => {}
            Some(read) => return Err(format!("a glyph reads as {read:?}, but level {level} has {ch:?} there")),
            None => new.push(LearnedGlyph { ch, rows: to_art(&centred(glyph)) }),
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
