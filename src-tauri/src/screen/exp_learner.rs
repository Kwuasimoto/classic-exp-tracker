//! Learns EXP glyphs the reader doesn't know yet, when only one character can fit:
//! the readout's format (`<number>[<int>.<2 digits>%]`) forces `[ ] . %`, and the
//! tracker's earlier readings rule out digits whose reading doesn't fit the level.
//! A line where several characters fit teaches nothing; its shapes wait on the
//! Glyphs page, or for a line where they appear in a position only one fits.

use crate::read::exp::{centred, parse, to_art, ExpReading, Glyph};
use crate::read::{GlyphKind, LearnedGlyph};

/// More distinct unknown shapes than this: too many combinations to tell apart.
const MAX_UNKNOWN_SHAPES: u32 = 2;

/// The characters of `line`'s unknown glyphs, if exactly one assignment of them
/// makes a reading that parses and that `plausible` accepts. Identical shapes on the
/// line get the same character.
pub fn learn(line: &[(Glyph, Option<char>)], plausible: impl Fn(ExpReading) -> bool) -> Option<Vec<LearnedGlyph>> {
    let mut shapes: Vec<Vec<String>> = Vec::new();
    let slots: Vec<Result<char, usize>> = line
        .iter()
        .map(|(glyph, ch)| {
            ch.ok_or_else(|| {
                let art = to_art(&centred(glyph));
                shapes.iter().position(|s| *s == art).unwrap_or_else(|| {
                    shapes.push(art);
                    shapes.len() - 1
                })
            })
        })
        .collect();
    let unknown = u32::try_from(shapes.len()).ok().filter(|&n| (1..=MAX_UNKNOWN_SHAPES).contains(&n))?;

    let alphabet: Vec<char> = GlyphKind::Exp.alphabet().chars().collect();
    let pick = |combo: usize, shape: usize| alphabet[combo / alphabet.len().pow(shape as u32) % alphabet.len()];
    let mut found = None;
    for combo in 0..alphabet.len().pow(unknown) {
        let text: String = slots.iter().map(|slot| slot.unwrap_or_else(|shape| pick(combo, shape))).collect();
        if parse(&text).is_ok_and(&plausible) {
            if found.is_some() {
                return None;
            }
            found = Some(combo);
        }
    }
    let combo = found?;
    Some(shapes.into_iter().enumerate().map(|(shape, rows)| LearnedGlyph { ch: pick(combo, shape), rows }).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::fixture;
    use crate::read::exp::read_line;

    /// The fixture's line (`326[0.10%]`) with the glyph at `unknown` made unrecognised.
    fn line_missing(unknown: usize) -> Vec<(Glyph, Option<char>)> {
        let mut line = read_line(&fixture("exp_326_0.10.png"), &[]).expect("text found");
        line[unknown].1 = None;
        line
    }

    #[test]
    fn the_format_alone_teaches_a_bracket() {
        let learned = learn(&line_missing(3), |_| true).expect("only '[' fits");
        assert_eq!(learned.iter().map(|g| g.ch).collect::<String>(), "[");
    }

    #[test]
    fn a_digit_that_several_readings_fit_teaches_nothing() {
        // The last digit of 326 could be anything: 320 … 329 all parse.
        assert_eq!(learn(&line_missing(2), |_| true), None);
        // …unless the earlier readings rule all but one out.
        let learned = learn(&line_missing(2), |r| r.exp == 326).expect("only 6 fits");
        assert_eq!(learned[0].ch, '6');
    }
}
