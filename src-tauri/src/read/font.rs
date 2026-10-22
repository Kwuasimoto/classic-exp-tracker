//! The EXP readout's font. The game draws the readout in a 5×7 pixel font scaled up
//! to the screen (an idea from xEmPIr3's tracker; no code from it is used), smoothed
//! where the scale isn't whole. So a character is recognised by drawing each font
//! character at the line's scale, at sub-pixel offsets, and comparing that with the
//! screen by normalised correlation, with the score and lead limits of beanguri's
//! Maple-Classic-Exp-Overlay (MIT). Nothing is learned and no resolution is special.
//!
//! The bitmaps were checked against our own 1920x1080 captures (tests/fixtures and
//! the glyphs averaged from a 10-minute recording, 2026-10-16).

/// Rows of the font, on the line's grid: digits fill all 7, '.' and ',' sit at the bottom.
pub const ROWS: usize = 7;
/// The best character must score at least this (true matches score ≈ 0.85–0.95)…
pub const MIN_SCORE: f32 = 0.75;
/// …and lead the next character by this much.
pub const MIN_LEAD: f32 = 0.03;
/// Sub-pixel offsets tried, in screen pixels, either way of the expected position.
const OFFSETS: [f32; 5] = [-1.0, -0.5, 0.0, 0.5, 1.0];

const FONT: &[(char, [&str; ROWS])] = &[
    ('0', [".###.", "#...#", "#...#", "#...#", "#...#", "#...#", ".###."]),
    ('1', [".#", "##", ".#", ".#", ".#", ".#", ".#"]),
    ('2', [".###.", "#...#", "....#", "...#.", "..#..", ".#...", "#####"]),
    ('3', [".###.", "#...#", "....#", "..##.", "....#", "#...#", ".###."]),
    ('4', ["...#.", "..##.", ".#.#.", "#..#.", "#####", "...#.", "...#."]),
    ('5', ["#####", "#....", "#....", ".###.", "....#", "#...#", ".###."]),
    ('6', [".###.", "#...#", "#....", "####.", "#...#", "#...#", ".###."]),
    ('7', ["#####", "....#", "....#", "....#", "...#.", "..#..", "..#.."]),
    ('8', [".###.", "#...#", "#...#", ".###.", "#...#", "#...#", ".###."]),
    ('9', [".###.", "#...#", "#...#", ".####", "....#", "#...#", ".###."]),
    ('[', ["##", "#.", "#.", "#.", "#.", "#.", "##"]),
    (']', ["##", ".#", ".#", ".#", ".#", ".#", "##"]),
    ('%', [".#....#", "#.#..#.", ".#..#..", "...#...", "..#..#.", ".#..#.#", "#....#."]),
    ('.', [".", ".", ".", ".", ".", ".", "#"]),
    (',', ["..", "..", "..", "..", "..", ".#", "#."]),
];

/// Screen brightness around one glyph, row by row.
#[derive(Clone, Debug, PartialEq)]
pub struct Patch {
    pub width: usize,
    pub height: usize,
    pub lums: Vec<f32>,
}

/// Where the glyph sits in its patch: its text columns and the line's text rows.
#[derive(Clone, Copy, Debug)]
pub struct Placement {
    pub left: f32,
    pub width: f32,
    pub text_top: f32,
    /// Rows of the text line, top of the tallest glyph to the bottom of the line.
    pub text_height: f32,
}

/// How much of each of `n` screen pixels each of `cells` font cells covers, the
/// first cell starting at `origin` and each `p` pixels wide: `n × cells`, row-major.
fn overlap(n: usize, origin: f32, p: f32, cells: usize) -> Vec<f32> {
    let mut m = vec![0.0; n * cells];
    for i in 0..n {
        for k in 0..cells {
            let a = origin + k as f32 * p;
            let covered = ((i + 1) as f32).min(a + p) - (i as f32).max(a);
            m[i * cells + k] = covered.clamp(0.0, 1.0);
        }
    }
    m
}

/// The ink `rows` would leave on a `width × height` patch at `p` pixels per font
/// pixel, its top-left font cell at (`ox`, `oy`): each screen pixel gets the share of
/// it that ink covers, as smoothing scales it.
fn render(rows: &[&str; ROWS], p: f32, ox: f32, oy: f32, width: usize, height: usize) -> Vec<f32> {
    let cols = rows[0].len();
    let (my, mx) = (overlap(height, oy, p, ROWS), overlap(width, ox, p, cols));
    let mut out = vec![0.0; width * height];
    for y in 0..height {
        for (fy, row) in rows.iter().enumerate() {
            let wy = my[y * ROWS + fy];
            if wy == 0.0 {
                continue;
            }
            for (fx, _) in row.bytes().enumerate().filter(|&(_, b)| b == b'#') {
                for x in 0..width {
                    out[y * width + x] += wy * mx[x * cols + fx];
                }
            }
        }
    }
    out
}

/// Zero-mean normalised correlation, −1 … 1; brightness and contrast don't matter
/// (the brackets are green, dimmer than the white digits). −1 for a flat image.
pub fn ncc(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len() as f32;
    let (ma, mb) = (a.iter().sum::<f32>() / n, b.iter().sum::<f32>() / n);
    let (mut ab, mut aa, mut bb) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        let (x, y) = (x - ma, y - mb);
        ab += x * y;
        aa += x * x;
        bb += y * y;
    }
    let d = (aa * bb).sqrt();
    if d > 0.0 { ab / d } else { -1.0 }
}

/// Every font character that fits the glyph's width, with its best score over the
/// sizes and offsets tried, best first.
pub fn rank(patch: &Patch, at: Placement) -> Vec<(char, f32)> {
    // The line's text rows span 7 font pixels, plus up to a pixel of smoothed edge.
    let h = at.text_height;
    let sizes = [(h - 1.0) / ROWS as f32, (h - 0.5) / ROWS as f32, h / ROWS as f32];
    let mut ranked: Vec<(char, f32)> = Vec::new();
    for (ch, rows) in FONT {
        let cols = rows[0].len() as f32;
        let mut best = f32::NEG_INFINITY;
        for p in sizes.into_iter().filter(|&p| p > 0.0 && (cols * p - at.width).abs() <= 1.5 * p + 1.0) {
            for dy in OFFSETS {
                let oy = at.text_top + dy - (h - ROWS as f32 * p) / 2.0;
                for dx in OFFSETS {
                    let ox = at.left + at.width / 2.0 - cols * p / 2.0 + dx;
                    best = best.max(ncc(&patch.lums, &render(rows, p, ox, oy, patch.width, patch.height)));
                }
            }
        }
        if best.is_finite() {
            ranked.push((*ch, best));
        }
    }
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
    ranked
}

/// The best character, if it scores well enough and clearly beats the next one.
pub fn decide(ranked: &[(char, f32)]) -> Option<char> {
    let &(ch, score) = ranked.first()?;
    let lead = ranked.get(1).map_or(f32::INFINITY, |&(_, next)| score - next);
    (score >= MIN_SCORE && lead >= MIN_LEAD).then_some(ch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_character_drawn_at_a_fractional_scale_reads_as_itself() {
        let p = 1.5;
        for (ch, rows) in FONT {
            let cols = rows[0].len() as f32;
            let (width, height) = ((cols * p) as usize + 4, (ROWS as f32 * p) as usize + 4);
            let ink = render(rows, p, 2.3, 2.3, width, height);
            let patch = Patch { width, height, lums: ink.iter().map(|v| 100.0 + 120.0 * v).collect() };
            let at = Placement { left: 2.0, width: cols * p, text_top: 2.0, text_height: ROWS as f32 * p + 0.5 };
            assert_eq!(decide(&rank(&patch, at)), Some(*ch), "{ch:?}");
        }
    }
}
