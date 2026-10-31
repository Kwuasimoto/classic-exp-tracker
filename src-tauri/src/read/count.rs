//! Stack counts on item slots, read like the EXP readout (font.rs): the count font drawn
//! at the screen's scale and compared with the screen by normalised correlation, with the
//! same score and lead limits. A count is drawn over its item's icon, so only the digit
//! and its 1 px black outline are compared: the icon's pixels around it are left out, and
//! white ones matching the digit (a bone, a highlight, seen on the quickslot) don't count
//! against it. Counts are left-aligned in the slot.

use super::font::{self, decide, ncc, Font, COUNT};
use crate::geom::Crop;

/// The best character at a column scoring this much, without being clearly one character,
/// is a digit something covers (a tooltip): the count can't be trusted.
const COVERED: f32 = 0.6;
/// Columns before the first digit (a '1' is drawn with blank columns first).
const MAX_LEAD: f32 = 3.0;
/// Columns between one digit and the next.
const NEXT_WITHIN: f32 = 4.0;

/// A slot's count rows, as brightness: the outline row above the digits, their 9 rows and
/// the outline row below, the slot's left edge at `left` and the outline row at `top` (in
/// screen pixels: 0 at native, under a pixel where the screen scales the game).
pub struct Band {
    width: usize,
    height: usize,
    left: f32,
    top: f32,
    lums: Vec<f32>,
}

impl Band {
    pub fn new(crop: &Crop, left: f32, top: f32) -> Self {
        let lums = (0..crop.height)
            .flat_map(|y| (0..crop.width).map(move |x| (x, y)))
            .map(|(x, y)| {
                let [r, g, b, _] = crop.px(x, y).map(f32::from);
                0.299 * r + 0.587 * g + 0.114 * b
            })
            .collect();
        Self { width: crop.width as usize, height: crop.height as usize, left, top, lums }
    }
}

/// `rows` with the 1-cell outline round them: one more row and column on every side.
fn outline(rows: &[&str]) -> Vec<String> {
    let (h, w) = (rows.len() as i32, rows[0].len() as i32);
    let ink = |x: i32, y: i32| (0..h).contains(&y) && (0..w).contains(&x) && rows[y as usize].as_bytes()[x as usize] == b'#';
    (-1..=h)
        .map(|y| {
            (-1..=w)
                .map(|x| if !ink(x, y) && [(-1, 0), (1, 0), (0, -1), (0, 1)].iter().any(|(dx, dy)| ink(x + dx, y + dy)) { '#' } else { '.' })
                .collect()
        })
        .collect()
}

/// How well the glyph `rows`, drawn at `p` screen pixels per font pixel from column `x`,
/// matches the band: correlation over the glyph's pixels and its outline's only.
fn score(band: &Band, rows: &[&str], p: f32, x: f32) -> f32 {
    let ring = outline(rows);
    let ring: Vec<&str> = ring.iter().map(String::as_str).collect();
    let ink = font::render(rows, p, x, band.top + p, band.width, band.height);
    let edge = font::render(&ring, p, x - p, band.top, band.width, band.height);
    let (mut screen, mut drawn) = (Vec::new(), Vec::new());
    for i in 0..band.lums.len() {
        if ink[i] > 0.5 || edge[i] > 0.5 {
            screen.push(band.lums[i]);
            drawn.push(ink[i]);
        }
    }
    ncc(&screen, &drawn)
}

/// Every count digit at column `x`, best first.
fn rank_at(band: &Band, font: &Font, p: f32, x: f32) -> Vec<(char, f32, f32)> {
    let mut ranked: Vec<(char, f32, f32)> = font
        .glyphs()
        .iter()
        .filter(|(_, rows)| x + rows[0].len() as f32 * p <= band.width as f32)
        .map(|&(ch, rows)| (ch, score(band, rows, p, x), rows[0].len() as f32 * p))
        .collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
    ranked
}

/// The count in the band at `p` screen pixels per font pixel (1 at native resolution).
/// `Ok(None)` when no digit is there (an empty slot, a skill); an error when something
/// digit-like is there but doesn't read, or the count ends at a covered digit, so it
/// can't be trusted.
pub fn read_count(band: &Band, p: f32) -> Result<Option<u32>, CountUnreadable> {
    let mut text = String::new();
    let mut from = band.left;
    let mut covered = false;
    loop {
        let window = if text.is_empty() { MAX_LEAD } else { NEXT_WITHIN } * p;
        // The column in the window where a digit reads best.
        let mut best: Option<(f32, char, f32, f32)> = None;
        let mut x = from;
        while x <= from + window {
            let ranked = rank_at(band, &COUNT, p, x);
            let scores: Vec<(char, f32)> = ranked.iter().map(|&(ch, s, _)| (ch, s)).collect();
            if let (Some(ch), Some(&(_, s, w))) = (decide(&scores), ranked.first()) {
                if best.is_none_or(|b| s > b.2) {
                    best = Some((x, ch, s, w));
                }
            } else if ranked.first().is_some_and(|&(_, s, _)| s >= COVERED) {
                covered = true;
            }
            x += p;
        }
        match best {
            Some((x, ch, _, w)) => {
                text.push(ch);
                from = x + w;
                covered = false;
            }
            None => break,
        }
    }
    match (text.is_empty(), covered) {
        (_, true) => Err(CountUnreadable),
        (true, false) => Ok(None),
        (false, false) => text.parse().map(Some).map_err(|_| CountUnreadable),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("a count is there but doesn't read")]
pub struct CountUnreadable;

/// The scores behind a read, for tuning the limits on real frames (dev tool).
pub fn best_scores(band: &Band, p: f32) -> Vec<(f32, char, f32)> {
    let mut out = Vec::new();
    let mut x = band.left;
    while x < band.width as f32 {
        if let Some(&(ch, s, _)) = rank_at(band, &COUNT, p, x).first() {
            out.push((x, ch, s));
        }
        x += p;
    }
    out
}

