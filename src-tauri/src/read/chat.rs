//! The chat box → its lines' kinds. Measured on real frames at 1366x768 (2026-11-01): the
//! game draws chat text in one exact colour per kind of line, with no anti-aliasing, on
//! 16 px bands anchored to the box's bottom. Everything here is in native pixels of the
//! chat strip (STRIP), which covers the tallest box, the input bar and the toggle button.

use super::ReadError;
use crate::geom::{Crop, Rect};

/// Where the chat strip is on a 1366x768 screen: the tallest box (20 bands, its scrollbar
/// at y 374) down to the closed chat line's descenders (y 722), past the toggle button.
pub const STRIP: Rect = Rect { x: 283, y: 374, w: 576, h: 349 };

/// Band height; every line, wrapped part or not, takes exactly one band.
pub const BAND: u32 = 16;
/// The most bands the tallest box shows.
pub const MAX_BANDS: u32 = 20;
/// Top of the bottom band (screen y 682).
const BOTTOM_BAND: u32 = 682 - STRIP.y;
/// Text columns (screen x 290–830).
pub const TEXT_X: (u32, u32) = (290 - STRIP.x, 831 - STRIP.x);
/// Where a line's first ink can land: a message at x 292–295, a wrapped part's indent at
/// 312–317 (screen x).
const FIRST_SLOT_X: (u32, u32) = (290 - STRIP.x, 320 - STRIP.x);
/// Rows of a band that hold ink: capitals from +4, descenders to +16 (the next band's
/// first row); the channel badge's frame from +2.
const INK_ROWS: (u32, u32) = (2, 16);
/// Ink pixels a band needs before it counts as a line ('.' alone has 1).
const MIN_INK: u32 = 8;

/// The open/closed button right of the input bar (screen x 818–829, y 709–720): `#` its
/// black outline, `b` its blue face, `w` anything else (its white sign, the corners).
const TOGGLE: Rect = Rect { x: 818 - STRIP.x, y: 709 - STRIP.y, w: 12, h: 12 };
const TOGGLE_OPEN: [&str; 12] = [
    "w##########w", "#bbbbbbbbbb#", "#bbbbbbbbbb#", "#bbbbbbbbbb#", "#bbbbbbbbbb#", "#bbbbbbbbbb#",
    "#bbbbbbbbbb#", "#bbwwwwwwbb#", "#bbwwwwwwbb#", "#bbbbbbbbbb#", "#bbbbbbbbbb#", "w##########w",
];
const TOGGLE_CLOSED: [&str; 12] = [
    "w##########w", "#bbbbbbbbbb#", "#bbbbbbbbbb#", "#bbbbwwbbbb#", "#bbbwwwwbbb#", "#bbwwwwwwbb#",
    "#bbwwwwwwbb#", "#bbbwwwwbbb#", "#bbbbwwbbbb#", "#bbbbbbbbbb#", "#bbbbbbbbbb#", "w##########w",
];

/// Whether the chat box is open: decided by the toggle button alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toggle {
    /// The box shows up to MAX_BANDS bands.
    Open,
    /// Only the newest line shows, in the input bar (band top at screen y 706).
    Closed,
    /// No button: a loading screen, a menu over it, or not the game.
    Hidden,
}

pub fn toggle(strip: &Crop) -> Toggle {
    // The input bar's white top line runs under the whole box, open or closed (constant in
    // every frame): a window over it means the chat can't be trusted.
    if !(INPUT_LINE_X.0..INPUT_LINE_X.1).all(|x| strip.px(x, INPUT_LINE_Y)[..3] == WHITE) {
        return Toggle::Hidden;
    }
    let matches = |mask: &[&str; 12]| {
        mask.iter().enumerate().all(|(dy, row)| {
            row.bytes().enumerate().all(|(dx, want)| {
                let [r, g, b, _] = strip.px(TOGGLE.x + dx as u32, TOGGLE.y + dy as u32);
                let got = match (r, g, b) {
                    (0, 0, 0) => b'#',
                    (r, _, b) if b > 200 && r < 120 => b'b',
                    _ => b'w',
                };
                got == want
            })
        })
    };
    if matches(&TOGGLE_OPEN) {
        Toggle::Open
    } else if matches(&TOGGLE_CLOSED) {
        Toggle::Closed
    } else {
        Toggle::Hidden
    }
}

/// The input bar's white top line (screen y 701, x 290–830 checked).
const INPUT_LINE_Y: u32 = 701 - STRIP.y;
const INPUT_LINE_X: (u32, u32) = (290 - STRIP.x, 831 - STRIP.x);

/// Top of band `k` counted up from the box's bottom (k = 0 is the newest line).
pub fn band_top(k: u32) -> u32 {
    BOTTOM_BAND - BAND * k
}

/// Top of the one line the closed chat shows in its input bar.
pub const CLOSED_BAND: u32 = 706 - STRIP.y;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Megaphone {
    Pink,
    Gold,
    Teal,
    /// The cheapest tier: no badge, always from the reader's own channel.
    Channel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    /// White text on the box's tint.
    Normal,
    /// Orange text on the tint (party chat, unconfirmed).
    Party,
    /// Yellow "[Notice] …" on the tint.
    Notice,
    /// "Name [CHxx] : …" on a coloured band.
    Megaphone(Megaphone),
    /// White "Name obtained [Item] from …" on a red band.
    Crate,
}

const WHITE: [u8; 3] = [255, 255, 255];
const PARTY: [u8; 3] = [255, 153, 0];
const NOTICE: [u8; 3] = [255, 255, 0];

impl Megaphone {
    /// Its text colour and its item names' colour (pink's item colour not seen yet).
    pub fn inks(self) -> &'static [[u8; 3]] {
        match self {
            Megaphone::Pink => &[[119, 0, 66]],
            Megaphone::Gold => &[[70, 39, 6], [255, 254, 153]],
            Megaphone::Teal => &[[9, 69, 76], [15, 89, 98]],
            Megaphone::Channel => &[[0, 63, 127]],
        }
    }
}

impl LineKind {
    /// The exact colours its text is drawn in.
    pub fn inks(self) -> &'static [[u8; 3]] {
        match self {
            LineKind::Normal | LineKind::Crate => &[WHITE],
            LineKind::Party => &[PARTY],
            LineKind::Notice => &[NOTICE],
            LineKind::Megaphone(m) => m.inks(),
        }
    }
}

/// Crate announcements' red band: on row +1 of every crate line in the frames, red 128–191
/// with green and blue at least 57 below it; the tint over the reddest scenery measured
/// stays within 30 of its green, and a window's greys and blues are nowhere near.
fn is_crate_red([r, g, b, _]: [u8; 4]) -> bool {
    r >= 120 && r >= g.saturating_add(50) && r >= b.saturating_add(50)
}

impl Megaphone {
    /// Its band's colour: the middle of the medians measured on row +1 of every
    /// megaphone in the frames (they vary by up to 20 with what shows through).
    fn background(self) -> [u8; 3] {
        match self {
            Megaphone::Pink => [218, 168, 190],
            Megaphone::Gold => [228, 180, 7],
            Megaphone::Teal => [92, 218, 200],
            Megaphone::Channel => [175, 188, 194],
        }
    }
}
const BACKGROUND_TOLERANCE: u8 = 25;

/// The box's tint darkens what's behind it: the median of a tinted band's brightest
/// channel was at most 102 in the frames, scenery above the box's at least 119.
const TINT_MAX: u8 = 110;

const KNOWN_INKS: [[u8; 3]; 9] =
    [WHITE, PARTY, NOTICE, [119, 0, 66], [70, 39, 6], [255, 254, 153], [9, 69, 76], [15, 89, 98], [0, 63, 127]];

fn median(mut v: Vec<u8>) -> u8 {
    let mid = v.len() / 2;
    *v.select_nth_unstable(mid).1
}

/// Whether columns [x0, x1) of band `top` lie under the box's tint (judged on everything
/// but text).
fn tinted(strip: &Crop, top: u32, (x0, x1): (u32, u32)) -> bool {
    let mut brightest = Vec::with_capacity(((x1 - x0) * BAND) as usize);
    for y in top..top + BAND {
        for x in x0..x1 {
            let [r, g, b, _] = strip.px(x, y);
            if !KNOWN_INKS.contains(&[r, g, b]) {
                brightest.push(r.max(g).max(b));
            }
        }
    }
    !brightest.is_empty() && median(brightest) <= TINT_MAX
}

/// The median colour of columns [x0, x1) of band `top`'s row +1, which holds no ink.
fn band_colour(strip: &Crop, top: u32, (x0, x1): (u32, u32)) -> [u8; 3] {
    let row: Vec<[u8; 4]> = (x0..x1).map(|x| strip.px(x, top + 1)).collect();
    [0, 1, 2].map(|c| median(row.iter().map(|p| p[c]).collect()))
}

/// The kind of line band `top` holds, or `None` when no known line starts there: the
/// kind's ink must reach into the first slot, where every line and wrapped part begins,
/// and sit on the kind's background (the box's tint for normal, party and notice lines; a
/// megaphone's band colour or the crate's red in the first slot), so text-coloured scenery
/// above a short box isn't taken for a line. A band's colour is judged at the line's start
/// only: a window over the rest of it is `covered`'s to find, and doesn't hide the line.
pub fn classify(strip: &Crop, top: u32) -> Option<LineKind> {
    let ink_count = |inks: &[[u8; 3]], (x0, x1): (u32, u32)| -> u32 {
        let mut n = 0;
        for y in top + INK_ROWS.0..=top + INK_ROWS.1 {
            for x in x0..x1 {
                let [r, g, b, _] = strip.px(x, y);
                n += u32::from(inks.contains(&[r, g, b]));
            }
        }
        n
    };
    let starts_line = |kind: LineKind| ink_count(kind.inks(), FIRST_SLOT_X) > 0;
    // The badge's white frame sits on megaphones, so megaphone inks are weighed first.
    let (n, megaphone) = [Megaphone::Pink, Megaphone::Gold, Megaphone::Teal, Megaphone::Channel]
        .map(|m| (ink_count(m.inks(), TEXT_X), m))
        .into_iter()
        .max_by_key(|&(n, _)| n)
        .expect("three megaphones");
    if n >= MIN_INK && starts_line(LineKind::Megaphone(megaphone)) {
        let near = band_colour(strip, top, FIRST_SLOT_X).iter().zip(megaphone.background()).all(|(&a, b)| a.abs_diff(b) <= BACKGROUND_TOLERANCE);
        return near.then_some(LineKind::Megaphone(megaphone));
    }
    // Tint lines are judged on the whole row: the tint's median is a steady test there
    // (at most 102 inside the box, at least 119 above it), where a patch of bright scenery
    // can push a 30-column first slot past it.
    let tint = tinted(strip, top, TEXT_X);
    for kind in [LineKind::Notice, LineKind::Party] {
        if ink_count(kind.inks(), TEXT_X) >= MIN_INK && starts_line(kind) {
            return tint.then_some(kind);
        }
    }
    if ink_count(&[WHITE], TEXT_X) >= MIN_INK && starts_line(LineKind::Normal) {
        // The band's own rows above the ink: red on crate announcements.
        let reds = (FIRST_SLOT_X.0..FIRST_SLOT_X.1).filter(|&x| is_crate_red(strip.px(x, top))).count() as u32;
        if reds * 5 >= (FIRST_SLOT_X.1 - FIRST_SLOT_X.0) * 4 {
            return Some(LineKind::Crate);
        }
        return tint.then_some(LineKind::Normal);
    }
    None
}

/// The closed chat's input bar: flat grey behind its one line (2026-11-01).
const CLOSED_GREY: [u8; 3] = [119, 119, 119];

/// The kind of the one line the closed chat shows: by its text colour alone, on the bar's
/// exact grey. A megaphone's line there has no badge or band.
pub fn classify_closed(strip: &Crop) -> Option<LineKind> {
    if band_colour(strip, CLOSED_BAND, TEXT_X) != CLOSED_GREY {
        return None;
    }
    let kinds = [
        LineKind::Megaphone(Megaphone::Pink),
        LineKind::Megaphone(Megaphone::Gold),
        LineKind::Megaphone(Megaphone::Teal),
        LineKind::Megaphone(Megaphone::Channel),
        LineKind::Notice,
        LineKind::Party,
        LineKind::Normal,
    ];
    kinds.into_iter().find(|kind| {
        (CLOSED_BAND + INK_ROWS.0..=CLOSED_BAND + INK_ROWS.1).any(|y| {
            (FIRST_SLOT_X.0..FIRST_SLOT_X.1).any(|x| {
                let [r, g, b, _] = strip.px(x, y);
                kind.inks().contains(&[r, g, b])
            })
        })
    })
}

/// Columns of a band's row +1 allowed to break its background (the box's edge column has
/// shown one black pixel on a clean band).
const STRAY_COLUMNS: usize = 2;
/// A clean megaphone's row +1 stays within 19 of its band colour; a covered one breaks it
/// on hundreds of columns (character info over the box, 2026-11-01).
const BAND_SPREAD: u8 = 20;
/// Behind the tint, no pixel of a clean band's row +1 (bar text) was brighter than this;
/// a window over the box is opaque and bright.
const TINT_BRIGHTEST: u8 = 150;

/// The columns of band `top` whose background isn't the kind's own, when there are more than
/// STRAY_COLUMNS of them: something (a window, a menu) is drawn over that part of the line.
pub fn covered(strip: &Crop, top: u32, kind: LineKind) -> Option<(u32, u32)> {
    let y = top + 1;
    let wrong: Vec<u32> = (TEXT_X.0..TEXT_X.1)
        .filter(|&x| {
            let [r, g, b, a] = strip.px(x, y);
            if kind.inks().contains(&[r, g, b]) {
                return false;
            }
            match kind {
                LineKind::Megaphone(m) => [r, g, b].iter().zip(m.background()).any(|(&c, want)| c.abs_diff(want) > BAND_SPREAD),
                LineKind::Crate => !is_crate_red([r, g, b, a]),
                LineKind::Normal | LineKind::Party | LineKind::Notice => !KNOWN_INKS.contains(&[r, g, b]) && r.max(g).max(b) > TINT_BRIGHTEST,
            }
        })
        .collect();
    (wrong.len() > STRAY_COLUMNS).then(|| (wrong[0], wrong[wrong.len() - 1] + 1))
}

/// Why a walk stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    /// No line starts here: the chat ends (above the box, an empty row), or a window covers
    /// the line's start; or MAX_BANDS were read.
    End,
    /// Still under the tint, with a flat bright colour in the first slot: a line of a colour
    /// not known yet, to log.
    Unknown { top: u32 },
}

/// A band holding a line, and the columns [from, to) where something is drawn over the rest
/// of it: a window, or the game's own mouse cursor (drawn into the picture). A covered band
/// isn't read; a later frame reads the line once it's clear.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Band {
    pub top: u32,
    pub kind: LineKind,
    pub covered: Option<(u32, u32)>,
}

/// The open box's lines, newest first: walked up band by band from the bottom until a band
/// has no line starting in it. A covered line doesn't stop the walk: the lines above it are
/// judged on their own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Walk {
    pub bands: Vec<Band>,
    pub stop: Stop,
}

pub fn walk(strip: &Crop) -> Walk {
    let mut bands = Vec::new();
    for k in 0..MAX_BANDS {
        let top = band_top(k);
        match classify(strip, top) {
            Some(kind) => bands.push(Band { top, kind, covered: covered(strip, top, kind) }),
            None => {
                // Text is flat colour: one bright colour repeated in the first slot, which
                // the tint leaves no scenery bright enough to make.
                let mut colours = std::collections::HashMap::new();
                for y in top + INK_ROWS.0..=top + INK_ROWS.1 {
                    for x in FIRST_SLOT_X.0..FIRST_SLOT_X.1 {
                        let [r, g, b, _] = strip.px(x, y);
                        if r.max(g).max(b) >= 200 {
                            *colours.entry([r, g, b]).or_insert(0u32) += 1;
                        }
                    }
                }
                let inked = colours.values().any(|&n| n >= MIN_INK);
                let stop = if tinted(strip, top, TEXT_X) && inked { Stop::Unknown { top } } else { Stop::End };
                return Walk { bands, stop };
            }
        }
    }
    Walk { bands, stop: Stop::End }
}

/// The box's last three rows (screen y 695–697) are faded toward black by these factors.
const FADE_ROWS: (u32, [f32; 3]) = (695 - STRIP.y, [0.984, 0.95, 0.91]);

/// Whether the pixel at (x, y) is `ink`, allowing for the bottom rows' fade.
pub fn is_ink(strip: &Crop, x: u32, y: u32, ink: [u8; 3]) -> bool {
    let [r, g, b, _] = strip.px(x, y);
    if [r, g, b] == ink {
        return true;
    }
    let Some(&f) = y.checked_sub(FADE_ROWS.0).and_then(|i| FADE_ROWS.1.get(i as usize)) else { return false };
    [r, g, b].iter().zip(ink).all(|(&got, want)| (f32::from(got) - f32::from(want) * f).abs() <= 2.0)
}

/// A megaphone's channel badge: a white frame 32 px wide (34 with its sides) on band rows
/// +2–+13, "CH" and two digits in red inside. Positions are strip columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Badge {
    pub channel: u8,
    /// The frame's left side; the sender's name ends 5–6 px before it.
    pub left: u32,
    /// The ':' after the badge (its dots on band rows +6 and +13).
    pub colon_x: u32,
    /// The message's first column: an item name or the text.
    pub message_x: u32,
}

const BADGE_W: u32 = 32;
const BADGE_RED: [u8; 3] = [204, 0, 0];
/// The digits' columns inside the frame (after "CH" at +1 and +5) and their 5 rows.
const DIGIT_COLS: (u32, u32) = (8, 20);
const DIGIT_ROWS: u32 = 6;
/// Every digit in the badge's 5 px font, seen in real badges (CH01–CH30, 2026-11-01).
const DIGITS: [(char, [&str; 5]); 10] = [
    ('0', [".##.", "#..#", "#..#", "#..#", ".##."]),
    ('1', ["##", ".#", ".#", ".#", ".#"]),
    ('2', ["###.", "...#", ".###", "#...", "####"]),
    ('3', ["###.", "...#", ".###", "...#", "###."]),
    ('4', ["#..#", "#..#", "####", "...#", "...#"]),
    ('5', ["####", "#...", "###.", "...#", "###."]),
    ('6', [".##.", "#...", "###.", "#..#", ".##."]),
    ('7', ["###", "..#", "..#", ".#.", ".#."]),
    ('8', [".##.", "#..#", ".##.", "#..#", ".##."]),
    ('9', [".##.", "#..#", ".###", "...#", ".##."]),
];

/// The channel badge on band `top`, if it has one. A frame whose digits aren't all known
/// is an error, so a new shape is logged instead of guessed.
pub fn badge(strip: &Crop, top: u32) -> Result<Option<Badge>, ReadError> {
    let white = |x: u32, y: u32| is_ink(strip, x, y, WHITE);
    let frame_at = |x: u32| {
        !white(x - 1, top + 2)
            && (x..x + BADGE_W).all(|i| white(i, top + 2) && white(i, top + 13))
            && !white(x + BADGE_W, top + 2)
            && (top + 4..top + 13).all(|y| white(x - 1, y) && white(x + BADGE_W, y))
    };
    let Some(x) = (TEXT_X.0 + 1..TEXT_X.1 - BADGE_W).find(|&x| frame_at(x)) else { return Ok(None) };
    // The digits' ink, column by column, cut into glyphs at empty columns.
    let column = |c: u32| -> String {
        (0..5).map(|r| if is_ink(strip, x + c, top + DIGIT_ROWS + r, BADGE_RED) { '#' } else { '.' }).collect()
    };
    let mut glyphs: Vec<(u32, Vec<String>)> = Vec::new();
    let mut open = false;
    for c in DIGIT_COLS.0..DIGIT_COLS.1 {
        let col = column(c);
        let inked = col.contains('#');
        match (inked, open) {
            (true, true) => glyphs.last_mut().expect("open glyph").1.push(col),
            (true, false) => glyphs.push((c, vec![col])),
            (false, _) => {}
        }
        open = inked;
    }
    let mut channel = 0u8;
    let mut line = String::new();
    let mut unknown = None;
    for (c, cols) in &glyphs {
        let rows: Vec<String> = (0..5).map(|r| cols.iter().map(|col| col.as_bytes()[r] as char).collect()).collect();
        match DIGITS.iter().find(|(_, shape)| shape.iter().eq(rows.iter())) {
            Some(&(d, _)) => {
                line.push(d);
                channel = channel.saturating_mul(10).saturating_add(d as u8 - b'0');
            }
            None => {
                line.push('?');
                unknown.get_or_insert(*c);
            }
        }
    }
    if let Some(c) = unknown.or((glyphs.len() != 2).then_some(DIGIT_COLS.0)) {
        return Err(ReadError::UnknownGlyph { column: x + c, line: format!("CH{line}") });
    }
    let colon_x = x + BADGE_W + 7;
    Ok(Some(Badge { channel, left: x - 1, colon_x, message_x: colon_x + 8 }))
}

/// A wrapped part's indent: its first ink lands at screen x 312–317, a line's at 292–295.
const WRAP_X: u32 = 310 - STRIP.x;
/// The ':' after a name: one column, its dots on band rows +6 and +13 (measured on every
/// normal and party line in the frames, 2026-11-01).
const COLON_ROWS: [u32; 2] = [6, 13];

/// Where a line's parts are, as strip column ranges [start, end).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spans {
    /// The sender: `None` on notices, crate announcements, wrapped parts and lines
    /// without a ':'.
    pub name: Option<(u32, u32)>,
    pub message: (u32, u32),
    /// A wrapped part of the line above.
    pub continued: bool,
}

/// Splits band `top`, holding a line of `kind`, into sender and message: by the badge on
/// megaphones, by the first ':' on other lines and on badgeless megaphones (the closed chat's).
/// `None` when the band has no ink of its kind.
pub fn split(strip: &Crop, top: u32, kind: LineKind) -> Result<Option<Spans>, ReadError> {
    let column = |x: u32| -> Vec<u32> {
        (INK_ROWS.0..=INK_ROWS.1).filter(|&r| kind.inks().iter().any(|&ink| is_ink(strip, x, top + r, ink))).collect()
    };
    let cols: Vec<(u32, Vec<u32>)> = (TEXT_X.0..TEXT_X.1).map(|x| (x, column(x))).filter(|(_, c)| !c.is_empty()).collect();
    let (Some((first, _)), Some((last, _))) = (cols.first(), cols.last()) else { return Ok(None) };
    let (first, end) = (*first, last + 1);
    let extent = |from: u32, to: u32| {
        let xs: Vec<u32> = cols.iter().map(|&(x, _)| x).filter(|&x| x >= from && x < to).collect();
        Some((*xs.first()?, xs.last()? + 1))
    };
    let whole = Spans { name: None, message: (first, end), continued: false };
    if first >= WRAP_X {
        return Ok(Some(Spans { continued: true, ..whole }));
    }
    // The first lone column with just the colon's two dots; names have no spaces.
    let by_colon = || {
        let colon = cols.iter().enumerate().find(|&(i, (x, c))| {
            c.as_slice() == COLON_ROWS
                && i > 0
                && cols[i - 1].0 + 1 < *x
                && cols.get(i + 1).is_some_and(|(next, _)| next - 1 > *x)
        });
        match colon {
            None => Some(whole),
            Some((_, &(x, _))) => extent(x + 1, end).map(|message| Spans { name: extent(first, x), message, continued: false }),
        }
    };
    Ok(match kind {
        // The closed chat shows megaphones without their badge: split those at the colon.
        LineKind::Megaphone(_) => match badge(strip, top)? {
            None => by_colon(),
            Some(b) => extent(b.colon_x + 1, end).map(|message| Spans { name: extent(first, b.left), message, continued: false }),
        },
        LineKind::Normal | LineKind::Party => by_colon(),
        LineKind::Notice | LineKind::Crate => Some(whole),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A full 1366x768 frame from the CHAT_FRAMES folder (frames hold other players'
    /// chat, so they stay out of the repo), cut to the chat strip.
    fn strip(name: &str) -> Crop {
        let dir = std::env::var("CHAT_FRAMES").expect("set CHAT_FRAMES to the chat frames folder");
        let img = image::open(std::path::Path::new(&dir).join(name)).unwrap_or_else(|e| panic!("{name}: {e}")).to_rgba8();
        Crop::from_image(&img).sub(STRIP).expect("1366x768 frame")
    }

    /// One letter per band, bottom band first: N normal, O party (orange), Y notice,
    /// P/G/T megaphones, R crate, `-` none.
    fn kinds(strip: &Crop, bands: u32) -> String {
        (0..bands)
            .map(|k| match classify(strip, band_top(k)) {
                Some(LineKind::Normal) => 'N',
                Some(LineKind::Party) => 'O',
                Some(LineKind::Notice) => 'Y',
                Some(LineKind::Megaphone(Megaphone::Pink)) => 'P',
                Some(LineKind::Megaphone(Megaphone::Gold)) => 'G',
                Some(LineKind::Megaphone(Megaphone::Teal)) => 'T',
                Some(LineKind::Megaphone(Megaphone::Channel)) => 'C',
                Some(LineKind::Crate) => 'R',
                None => '-',
            })
            .collect()
    }

    #[test]
    fn reads_a_channel_badge() {
        // A CH05 badge (frame and digits only) cut from a real frame, put back on band 3.
        let crop = crate::geom::fixture("chat_badge_ch05.png");
        let (left, top) = (100, band_top(3));
        let mut rgba = vec![0; (STRIP.w * STRIP.h * 4) as usize];
        for y in 0..crop.height {
            for x in 0..crop.width {
                let i = (((top + 2 + y) * STRIP.w + left + x) * 4) as usize;
                rgba[i..i + 4].copy_from_slice(&crop.px(x, y));
            }
        }
        let strip = Crop::new(STRIP.w, STRIP.h, rgba).expect("sized");
        assert_eq!(badge(&strip, top), Ok(Some(Badge { channel: 5, left, colon_x: left + 40, message_x: left + 48 })));
        assert_eq!(badge(&strip, band_top(2)), Ok(None));
    }

    #[test]
    #[ignore = "needs CHAT_FRAMES"]
    fn chat_badges() {
        let mut frames: Vec<(String, u32)> = (1..=30).map(|i| (format!("seq/f{i:02}.png"), 9)).collect();
        frames.extend([("chat1.png".into(), 9), ("chat2.png".into(), 9), ("max.png".into(), 20)]);
        let mut found = 0;
        for (name, bands) in &frames {
            let s = strip(name);
            let channels: String = (0..*bands)
                .map(|k| {
                    let top = band_top(k);
                    match badge(&s, top).unwrap_or_else(|e| panic!("{name} band {k}: {e}")) {
                        None => "-".to_string(),
                        Some(b) => {
                            found += 1;
                            let Some(LineKind::Megaphone(m)) = classify(&s, top) else { panic!("{name} band {k}: badge off a megaphone") };
                            let ink = m.inks()[0];
                            assert!(is_ink(&s, b.colon_x, top + 6, ink) && is_ink(&s, b.colon_x, top + 13, ink), "{name} band {k}: no ':'");
                            format!("{:02}", b.channel)
                        }
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
            match name.as_str() {
                "chat1.png" => assert_eq!(channels, "10 16 06 - - 13 18 02 17"),
                "chat2.png" => assert_eq!(channels, "30 10 06 - 27 01 - - -"),
                _ => {}
            }
        }
        // Every badge in the frames (the bottom band's included, its last row faded).
        assert_eq!(found, 137);
    }

    #[test]
    #[ignore = "needs CHAT_FRAMES"]
    fn chat_splits() {
        let mut frames: Vec<(String, u32)> = (1..=30).map(|i| (format!("seq/f{i:02}.png"), 9)).collect();
        frames.extend([("chat1.png".into(), 9), ("chat2.png".into(), 9), ("max.png".into(), 20)]);
        let (mut named, mut colonless) = (0, Vec::new());
        for (name, bands) in &frames {
            let s = strip(name);
            for k in 0..*bands {
                let top = band_top(k);
                let Some(kind) = classify(&s, top) else { continue };
                let spans = split(&s, top, kind).unwrap_or_else(|e| panic!("{name} band {k}: {e}")).unwrap_or_else(|| panic!("{name} band {k}: no spans"));
                assert!(spans.message.0 < spans.message.1, "{name} band {k}");
                if let Some((a, b)) = spans.name {
                    assert!(a < b && b < spans.message.0, "{name} band {k}: {spans:?}");
                    named += 1;
                } else if matches!(kind, LineKind::Normal | LineKind::Party | LineKind::Megaphone(_)) && !spans.continued {
                    colonless.push(format!("{name} band {k}"));
                }
            }
        }
        // Every normal, party and megaphone line has its sender split off.
        assert_eq!(colonless, Vec::<String>::new());
        // "Amala : glgl": the name's ink, then "glgl" after the colon at 53 (strip columns).
        let s = strip("chat2.png");
        let amala = split(&s, band_top(6), LineKind::Normal).expect("readable").expect("a line");
        assert_eq!(amala, Spans { name: Some((9, 47)), message: (61, 81), continued: false });
        assert!(named > 200, "{named}");
    }

    #[test]
    #[ignore = "needs CHAT_FRAMES"]
    fn chat_walk_finds_the_box_height() {
        // The default box holds 9 lines and the tallest 20; the walk ends at each box's top,
        // with untinted scenery above the default box.
        let mut frames: Vec<(String, usize)> = (1..=30).map(|i| (format!("seq/f{i:02}.png"), 9)).collect();
        frames.extend([("chat1.png".into(), 9), ("chat2.png".into(), 9), ("max.png".into(), 20)]);
        for (name, want) in frames {
            let w = walk(&strip(&name));
            assert_eq!((w.bands.len(), w.stop), (want, Stop::End), "{name}");
            // f07: the game's own mouse cursor sits over band 4's right end.
            let covered: Vec<u32> = w.bands.iter().filter(|b| b.covered.is_some()).map(|b| (682 - STRIP.y - b.top) / BAND).collect();
            assert_eq!(covered, if name == "seq/f07.png" { vec![4] } else { vec![] }, "{name}");
        }
    }

    /// One frame every 3 s while windows were moved over the chat (2026-11-01): each band
    /// up from the bottom as its kind (C channel megaphone) and, when something covers it, the
    /// screen columns covered; `-` where the walk stops. Checked against the pictures.
    #[test]
    #[ignore = "needs CHAT_FRAMES"]
    fn chat_walk_marks_covered_lines() {
        let want = [
            "Teal Teal Teal[475-793] Teal[475-793] Teal[475-793] Channel[442-814] Gold[475-793] Pink[475-793] -",
            "Pink Teal Teal[475-793] Teal[475-793] Teal[475-793] Teal[475-793] Channel[461-831] Gold[475-793] Pink[475-831] -",
            "Pink Teal Teal Teal Teal Teal Channel[461-831] Gold[611-831] Pink[611-831] -",
            "Pink Teal Teal Teal[519-831] Teal[309-831] Teal[309-831] Channel[309-831] Gold[309-831] Pink[309-831] -",
            "Pink Teal[312-515] -",
            "Pink Teal[616-819] Teal[613-822] Teal[613-822] Teal[613-822] Teal[613-822] -",
            "Pink Teal Teal Teal Teal[353-668] Teal[353-668] -",
            "Pink[490-808] Teal[490-808] Teal[490-808] Teal[490-808] Teal[490-808] Teal[490-808] -",
            "Pink[606-831] Crate[606-831] Crate[606-831] Pink[606-831] Teal[606-831] Teal[606-831] -",
            "Pink Crate Crate Pink[647-831] Teal[647-831] Teal[647-831] -",
            "-", "-", "-", "-", "-", "-",
            "Pink[729-831] Notice[731-831] Notice[731-831] Pink[729-831] Pink[729-831] Crate[729-831] -",
            "Pink[813-831] Notice[815-831] Notice[815-831] Pink[813-831] Pink[813-831] Crate[813-831] -",
            "Pink Notice Notice Pink Pink Crate -",
            "Notice Normal Pink Notice Notice Pink -",
        ];
        // A window over the input bar or the toggle button hides the chat (c11–c18).
        let hidden = [11, 12, 13, 14, 15, 16, 17, 18];
        for (i, want) in want.iter().enumerate() {
            let name = format!("cover/c{:02}.png", i + 1);
            let s = strip(&name);
            assert_eq!(toggle(&s) == Toggle::Hidden, hidden.contains(&(i + 1)), "{name}");
            let w = walk(&s);
            let mut got: Vec<String> = w
                .bands
                .iter()
                .map(|b| {
                    let kind = match b.kind {
                        LineKind::Megaphone(m) => format!("{m:?}"),
                        k => format!("{k:?}"),
                    };
                    match b.covered {
                        Some((from, to)) => format!("{kind}[{}-{}]", from + STRIP.x, to + STRIP.x),
                        None => kind,
                    }
                })
                .collect();
            got.push("-".into());
            assert_eq!(got.join(" "), *want, "{name}");
        }
    }

    #[test]
    #[ignore = "needs CHAT_FRAMES"]
    fn chat_toggle_and_kinds() {
        for (name, toggle_state, bands, want) in [
            ("chat1.png", Toggle::Open, 9, "PPPRRPPPP"),
            ("chat2.png", Toggle::Open, 9, "PPGNPGNYY"),
            ("max.png", Toggle::Open, 20, "PPPPPTTGGPPGGPPTTPGG"),
        ] {
            let s = strip(name);
            assert_eq!(toggle(&s), toggle_state, "{name}");
            assert_eq!(kinds(&s, bands), want, "{name}");
        }
        // 30 frames a second apart while chat scrolled by (and was scrolled back up).
        let seq = "PPOONNNNT OPPOONNNN OOPPOONNN OOOPPOONN OOOPPOONN OOOPPOONN OOOPPOONN TTTTPPYYT YTTTTNNGN GNNRRGNPP NPPPPPGNP NPGNYYPPP NPGNYYPPP NPGNYYPPP PPPPPGNPG GNNRRGNPP NGNNRRGNP NGNNRRGNP TTNNGNNRR PPYYTTTTN TTTPPYYTT TTNNPTTTT NNNNTTTTN OOOOPPOON POOOOPPOO PPOOOOPPO PPOOOOPPO PPPOOOOPP GPPPOOOOP GPPPOOOOP";
        for (i, want) in seq.split(' ').enumerate() {
            let name = format!("seq/f{:02}.png", i + 1);
            let s = strip(&name);
            assert_eq!(toggle(&s), Toggle::Open, "{name}");
            assert_eq!(kinds(&s, 9), want, "{name}");
        }
        // The character info window over the box's right half, its bottom edge on band 2:
        // the two lines below it are whole, the ones it covers aren't.
        let w = walk(&strip("covered1.png"));
        let covered: Vec<Option<u32>> = w.bands.iter().map(|b| b.covered.map(|(from, _)| from + STRIP.x)).collect();
        assert_eq!(covered[..3], [None, None, Some(475)], "{w:?}");
        let closed = strip("closed.png");
        assert_eq!(toggle(&closed), Toggle::Closed);
        assert_eq!(classify_closed(&closed), Some(LineKind::Megaphone(Megaphone::Pink)));
        let spans = split(&closed, CLOSED_BAND, LineKind::Megaphone(Megaphone::Pink)).expect("readable").expect("a line");
        assert!(spans.name.is_some(), "clairy : B> 5x pet speed…");
        assert_eq!(classify_closed(&strip("chat1.png")), None, "an open box has no grey bar");
    }
}


