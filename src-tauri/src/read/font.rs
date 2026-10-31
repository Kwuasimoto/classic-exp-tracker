//! The game's pixel fonts: the EXP readout's, and the inventory's meso total. The game
//! draws them scaled up to the screen (an idea from xEmPIr3's tracker; no code from it is
//! used), smoothed where the scale isn't whole. So a character is recognised by drawing
//! each font character at the line's scale, at sub-pixel offsets, and comparing that with
//! the screen by normalised correlation, with the score and lead limits of beanguri's
//! Maple-Classic-Exp-Overlay (MIT). Nothing is learned and no resolution is special.
//!
//! The EXP bitmaps were checked against our own 1920x1080 captures (tests/fixtures and
//! the glyphs averaged from a 10-minute recording, 2026-10-16); the meso bitmaps were
//! drawn from 1366x768 captures, where the game draws them 1:1 (2026-10-23).

/// A font: each character's bitmap, `rows` tall plus `descent` rows below the line that
/// only some characters reach (the meso total's comma).
pub struct Font {
    pub rows: usize,
    pub descent: usize,
    glyphs: &'static [(char, &'static [&'static str])],
}

impl Font {
    pub fn glyphs(&self) -> &'static [(char, &'static [&'static str])] {
        self.glyphs
    }

    /// The fewest columns any character's ink spans.
    pub fn narrowest(&self) -> usize {
        self.glyphs
            .iter()
            .filter_map(|(_, rows)| {
                let inked: Vec<usize> = (0..rows[0].len()).filter(|&c| rows.iter().any(|r| r.as_bytes()[c] == b'#')).collect();
                Some(inked.last()? - inked.first()? + 1)
            })
            .min()
            .unwrap_or(1)
    }
}
/// The best character must score at least this (true matches score ≈ 0.85–0.95)…
pub const MIN_SCORE: f32 = 0.75;
/// …and lead the next character by this much.
pub const MIN_LEAD: f32 = 0.03;
/// Sub-pixel offsets tried, in screen pixels, either way of the expected position.
const OFFSETS: [f32; 5] = [-1.0, -0.5, 0.0, 0.5, 1.0];

/// The EXP readout: digits fill all 7 rows, '.' and ',' sit at the bottom.
pub const EXP: Font = Font { rows: 7, descent: 0, glyphs: EXP_GLYPHS };

const EXP_GLYPHS: &[(char, &[&str])] = &[
    ('0', &[".###.", "#...#", "#...#", "#...#", "#...#", "#...#", ".###."]),
    ('1', &[".#", "##", ".#", ".#", ".#", ".#", ".#"]),
    ('2', &[".###.", "#...#", "....#", "...#.", "..#..", ".#...", "#####"]),
    ('3', &[".###.", "#...#", "....#", "..##.", "....#", "#...#", ".###."]),
    ('4', &["...#.", "..##.", ".#.#.", "#..#.", "#####", "...#.", "...#."]),
    ('5', &["#####", "#....", "#....", ".###.", "....#", "#...#", ".###."]),
    ('6', &[".###.", "#...#", "#....", "####.", "#...#", "#...#", ".###."]),
    ('7', &["#####", "....#", "....#", "....#", "...#.", "..#..", "..#.."]),
    ('8', &[".###.", "#...#", "#...#", ".###.", "#...#", "#...#", ".###."]),
    ('9', &[".###.", "#...#", "#...#", ".####", "....#", "#...#", ".###."]),
    ('[', &["##", "#.", "#.", "#.", "#.", "#.", "##"]),
    (']', &["##", ".#", ".#", ".#", ".#", ".#", "##"]),
    ('%', &[".#....#", "#.#..#.", ".#..#..", "...#...", "..#..#.", ".#..#.#", "#....#."]),
    ('.', &[".", ".", ".", ".", ".", ".", "#"]),
    (',', &["..", "..", "..", "..", "..", ".#", "#."]),
];

/// The inventory's meso total: black digits 8 rows tall on white; the comma's tail
/// reaches 2 rows below them.
pub const MESO: Font = Font { rows: 8, descent: 2, glyphs: MESO_GLYPHS };

const MESO_GLYPHS: &[(char, &[&str])] = &[
    ('0', &[".###.", "#...#", "#...#", "#...#", "#...#", "#...#", "#...#", ".###.", ".....", "....."]),
    ('1', &[".#.", "##.", ".#.", ".#.", ".#.", ".#.", ".#.", "###", "...", "..."]),
    ('2', &[".###.", "#...#", "....#", "...#.", "..#..", ".#...", "#....", "#####", ".....", "....."]),
    ('3', &[".###.", "#...#", "....#", "..##.", "....#", "....#", "#...#", ".###.", ".....", "....."]),
    ('4', &["...#.", "..##.", ".#.#.", "#..#.", "#####", "...#.", "...#.", "...#.", ".....", "....."]),
    ('5', &["#####", "#....", "#....", "####.", "....#", "....#", "#...#", ".###.", ".....", "....."]),
    ('6', &["..##.", ".#...", "#....", "####.", "#...#", "#...#", "#...#", ".###.", ".....", "....."]),
    ('7', &["#####", "....#", "...#.", "...#.", "..#..", "..#..", ".#...", ".#...", ".....", "....."]),
    ('8', &[".###.", "#...#", "#...#", ".###.", "#...#", "#...#", "#...#", ".###.", ".....", "....."]),
    ('9', &[".###.", "#...#", "#...#", "#...#", ".####", "....#", "...#.", ".##..", ".....", "....."]),
    (',', &["..", "..", "..", "..", "..", "..", ".#", ".#", ".#", "#."]),
];

/// Stack counts on item slots: the 9 rows of each digit's white-to-blue core, which the
/// game outlines with 1 black pixel all round (count.rs compares the outline too).
pub const COUNT: Font = Font { rows: 9, descent: 0, glyphs: COUNT_GLYPHS };

const COUNT_GLYPHS: &[(char, &[&str])] = &[
    ('0', &[".####.", "#....#", "#....#", "#....#", "#....#", "#....#", "#....#", "#....#", ".####."]),
    ('1', &["..#", "###", "..#", "..#", "..#", "..#", "..#", "..#", "..#"]),
    ('2', &[".####.", "#....#", ".....#", ".....#", "....#.", "..##..", ".#....", "#.....", "######"]),
    ('3', &[".####.", "#....#", ".....#", ".....#", "..###.", ".....#", ".....#", "#....#", ".####."]),
    ('4', &["....#.", "...##.", "..#.#.", ".#..#.", "#...#.", "######", "....#.", "....#.", "....#."]),
    ('5', &["######", "#.....", "#.....", "#####.", ".....#", ".....#", ".....#", "#....#", ".####."]),
    ('6', &["..###.", ".#....", "#.....", "#####.", "#....#", "#....#", "#....#", "#....#", ".####."]),
    ('7', &["######", ".....#", "....#.", "....#.", "...#..", "...#..", "..#...", "..#...", ".#...."]),
    ('8', &[".####.", "#....#", "#....#", "#....#", ".####.", "#....#", "#....#", "#....#", ".####."]),
    ('9', &[".####.", "#....#", "#....#", "#....#", "#....#", ".#####", ".....#", ".....#", ".####."]),
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
pub fn render(rows: &[&str], p: f32, ox: f32, oy: f32, width: usize, height: usize) -> Vec<f32> {
    let (cols, n_rows) = (rows[0].len(), rows.len());
    let (my, mx) = (overlap(height, oy, p, n_rows), overlap(width, ox, p, cols));
    let mut out = vec![0.0; width * height];
    for y in 0..height {
        for (fy, row) in rows.iter().enumerate() {
            let wy = my[y * n_rows + fy];
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
pub fn rank(patch: &Patch, at: Placement, font: &Font) -> Vec<(char, f32)> {
    // The line's text rows span the font's rows (or, with a character that reaches
    // below the line, its rows and descent), plus up to a pixel of smoothed edge.
    let h = at.text_height;
    let mut spans = vec![font.rows as f32];
    if font.descent > 0 {
        spans.push((font.rows + font.descent) as f32);
    }
    let sizes: Vec<(f32, f32)> = spans.iter().flat_map(|&span| [h - 1.0, h - 0.5, h].map(|t| (t / span, span))).collect();
    let mut ranked: Vec<(char, f32)> = Vec::new();
    for (ch, rows) in font.glyphs {
        let cols = rows[0].len() as f32;
        let mut best = f32::NEG_INFINITY;
        for &(p, span) in sizes.iter().filter(|&&(p, _)| p > 0.0 && (cols * p - at.width).abs() <= 1.5 * p + 1.0) {
            for dy in OFFSETS {
                let oy = at.text_top + dy - (h - span * p) / 2.0;
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
        for (ch, rows) in EXP.glyphs {
            let cols = rows[0].len() as f32;
            let (width, height) = ((cols * p) as usize + 4, (EXP.rows as f32 * p) as usize + 4);
            let ink = render(rows, p, 2.3, 2.3, width, height);
            let patch = Patch { width, height, lums: ink.iter().map(|v| 100.0 + 120.0 * v).collect() };
            let at = Placement { left: 2.0, width: cols * p, text_top: 2.0, text_height: EXP.rows as f32 * p + 0.5 };
            assert_eq!(decide(&rank(&patch, at, &EXP)), Some(*ch), "{ch:?}");
        }
    }
}
