//! What the screen shows, frame by frame: reads the readout boxes, debounces map
//! changes, follows the level box, and logs reader failures. No I/O apart from log
//! lines; pipeline.rs saves what this module finds.

mod level_box;

use log::{info, warn};

use crate::geom::Crop;
use crate::read::exp::{read_line, reading, ExpReading, Glyph, Line};
use crate::read::font::{ncc, MIN_LEAD, MIN_SCORE};
use crate::read::map_strip::{is_collapsed_minimap, read_strip, Signature};
use crate::read::{LearnedGlyph, ReadError, ART_LEVELS};
use crate::tracker::{ConfirmOutcome, MapConfirmer, Tracker};
use level_box::LevelBox;

/// Unrecognised EXP glyphs logged per run (each distinct look once: one that
/// correlates this well with a logged one, at the same size, counts as the same), so
/// a user's log shows exactly what failed without flooding it.
const MAX_LOGGED_UNKNOWN: usize = 40;
const SAME_LOOK: f32 = 0.9;
/// Both the EXP box and the minimap strip blank this long: the game isn't full screen on the
/// main display (windowed, on another monitor, or behind something). A loading screen or a
/// map change is over well before.
const GAME_HIDDEN_SECS: i64 = 60;

/// The EXP box's pixels on the last frame, what they read as, and whether touching
/// glyphs were split apart in them.
struct LastExp {
    crop: Crop,
    read: Result<ExpReading, ReadError>,
    split: bool,
}

/// A glyph's brightness for the log, one line per row (see ART_LEVELS).
fn brightness_art(glyph: &Glyph) -> String {
    let lums = &glyph.patch.lums;
    let (lo, hi) = lums.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &v| (lo.min(v), hi.max(v)));
    let level = |v: f32| if hi > lo { ((v - lo) / (hi - lo) * 9.0).round() as usize } else { 0 };
    lums.chunks(glyph.patch.width.max(1))
        .map(|row| format!("\n    |{}|", row.iter().map(|&v| ART_LEVELS[level(v)] as char).collect::<String>()))
        .collect()
}

/// One frame's regions, read.
pub struct FrameRead {
    pub exp: Option<ExpReading>,
    pub map_readable: bool,
    /// The minimap is collapsed to its title bar: the player is in a shop.
    pub shopping: bool,
    /// A map confirmed on this frame, with the strips that confirmed it (for OCR).
    pub new_map: Option<(Signature, Vec<Crop>)>,
    /// The EXP box and the minimap strip have both been blank for GAME_HIDDEN_SECS.
    pub game_hidden: bool,
}

pub struct Screen {
    /// The EXP box last frame: an unchanged box isn't read again.
    exp_last: Option<LastExp>,
    /// Unrecognised EXP glyphs already logged this run.
    logged_unknown: Vec<Glyph>,
    /// The text height of the last readout that read, logged when it changes (a new scale).
    logged_text_height: Option<u32>,
    /// Frames this minute whose EXP readout had touching glyphs split apart, and
    /// whether a split line was logged this run.
    minute_splits: u32,
    split_logged: bool,
    confirmer: MapConfirmer,
    level_box: LevelBox,
    exp_log: ReadLog,
    map_log: ReadLog,
    /// The wall-clock minute being counted for the summary line, and its frames.
    minute: i64,
    minute_frames: u32,
}

impl Screen {
    pub fn new() -> Self {
        Self {
            exp_last: None,
            logged_unknown: Vec::new(),
            logged_text_height: None,
            minute_splits: 0,
            split_logged: false,
            confirmer: MapConfirmer::new(),
            level_box: LevelBox::default(),
            exp_log: ReadLog::new("EXP"),
            map_log: ReadLog::new("map strip"),
            minute: 0,
            minute_frames: 0,
        }
    }

    /// Read this frame's regions.
    pub fn read(&mut self, t: i64, exp: &Crop, map: Crop, level: Crop, bar: &Crop) -> FrameRead {
        self.count_frame(t);
        let read = self.read_exp_box(exp);
        let exp = self.exp_log.record(t, read);
        let signature = self.map_log.record(t, read_strip(&map));
        let map_readable = signature.is_some();
        let new_map = signature.and_then(|signature| match self.confirmer.feed(signature, map) {
            ConfirmOutcome::Changed { signature, strips } => Some((signature, strips)),
            ConfirmOutcome::Unchanged => None,
        });
        self.level_box.observe(level);
        let game_hidden = match (self.exp_log.blank_since, self.map_log.blank_since) {
            (Some(a), Some(b)) => t - a.max(b) >= GAME_HIDDEN_SECS,
            _ => false,
        };
        FrameRead { exp, map_readable, new_map, shopping: is_collapsed_minimap(bar), game_hidden }
    }

    /// The EXP readout. A box that looks as it did last frame reads as it did then;
    /// a changed one is read, and what's notable about it logged.
    fn read_exp_box(&mut self, exp: &Crop) -> Result<ExpReading, ReadError> {
        if let Some(last) = &self.exp_last
            && last.crop == *exp
        {
            self.minute_splits += u32::from(last.split);
            return last.read.clone();
        }
        let (read, split) = match read_line(exp) {
            Ok(line) => {
                self.log_line(&line);
                let read = reading(&line);
                if read.is_ok() && self.logged_text_height != Some(line.text_height) {
                    info!("EXP font: {} px text line, {:.2} px per font pixel", line.text_height, line.font_px());
                    self.logged_text_height = Some(line.text_height);
                }
                (read, line.is_readout() && line.glyphs.iter().any(|g| g.split))
            }
            Err(e) => (Err(e), false),
        };
        self.minute_splits += u32::from(split);
        self.exp_last = Some(LastExp { crop: exp.clone(), read: read.clone(), split });
        read
    }

    /// Log the first readout with touching glyphs (counted per minute after that) and
    /// glyphs not recognised. Only for the readout (Line::is_readout): a line drawn over
    /// the box gets the one-line failure from ReadLog, and doesn't use up the glyph log
    /// meant for the readout's own failures.
    fn log_line(&mut self, line: &Line) {
        if !line.is_readout() {
            return;
        }
        if !self.split_logged && line.glyphs.iter().any(|g| g.split) {
            let text: String = line.glyphs.iter().map(|g| format!("{}{}", if g.split { "|" } else { "" }, g.ch.unwrap_or('?'))).collect();
            info!("EXP readout had touching glyphs, split apart (marked |): {text:?}; counted per minute from now on");
            self.split_logged = true;
        }
        if line.glyphs.iter().any(|g| g.ch.is_none()) {
            self.log_unknown(line);
        }
    }

    /// Log each unrecognised glyph of `line` that doesn't look like one logged this
    /// run: the line it was in, where it is and its size, the closest characters with
    /// their scores against the acceptance limits, and its brightness.
    fn log_unknown(&mut self, line: &Line) {
        let text = line.text();
        for (i, glyph) in line.glyphs.iter().enumerate().filter(|(_, g)| g.ch.is_none()) {
            if self.logged_unknown.len() >= MAX_LOGGED_UNKNOWN {
                return;
            }
            let seen = |g: &Glyph| {
                (g.patch.width, g.patch.height) == (glyph.patch.width, glyph.patch.height)
                    && ncc(&g.patch.lums, &glyph.patch.lums) >= SAME_LOOK
            };
            if self.logged_unknown.iter().any(seen) {
                continue;
            }
            let near: Vec<String> = glyph.ranked.iter().take(3).map(|(ch, score)| format!("{ch:?} {score:.3}")).collect();
            let nearest = if near.is_empty() {
                format!("no font character is {} px wide at this scale", glyph.width)
            } else {
                format!("closest {} (a match needs ≥ {MIN_SCORE} and a lead of {MIN_LEAD})", near.join(", "))
            };
            let split = if glyph.split { ", cut from touching glyphs" } else { "" };
            warn!(
                "EXP glyph {} of {} not recognised in {text:?} (column {}, {} px wide, {} px text line, {:.2} px per font pixel{split}): {nearest}{}",
                i + 1,
                line.glyphs.len(),
                glyph.x,
                glyph.width,
                line.text_height,
                line.font_px(),
                brightness_art(glyph)
            );
            self.logged_unknown.push(glyph.clone());
        }
        if self.logged_unknown.len() >= MAX_LOGGED_UNKNOWN {
            warn!("{MAX_LOGGED_UNKNOWN} unrecognised EXP glyphs logged; not logging more this run");
        }
    }

    /// After the tracker took this frame's sample: count an accepted EXP reading
    /// toward learning level digits, learn any that are due, and give the tracker the
    /// level the box shows. Returns the digits learned (already in use), to save.
    pub fn sync_level(&mut self, tracker: &mut Tracker, exp: Option<ExpReading>) -> Vec<LearnedGlyph> {
        // Accepted readings move the baseline; rejected ones (misreads) don't.
        if exp.is_some() && tracker.baseline() == exp {
            self.level_box.accepted_reading(tracker.level());
        }
        let learned = tracker.level().map(|level| self.level_box.learn(level)).unwrap_or_default();
        if let Some(level) = self.level_box.level() {
            tracker.use_screen_level(level);
        }
        learned
    }

    /// Use the level digits learned at the current resolution; the level box is read
    /// afresh with them.
    pub fn set_level_glyphs(&mut self, level: Vec<LearnedGlyph>) {
        self.level_box.set_learned(level);
    }

    /// Forget the confirmed map, so the next frames confirm it again.
    pub fn reconfirm_map(&mut self) {
        self.confirmer = MapConfirmer::new();
    }

    /// At each new wall-clock minute, log the previous minute's reader failures.
    fn count_frame(&mut self, t: i64) {
        let minute = t.div_euclid(60);
        if minute != self.minute {
            if self.minute_frames > 0 {
                info!(
                    "minute summary: {} frames, {}, {}, EXP touching glyphs split in {} frame(s)",
                    self.minute_frames,
                    self.exp_log.end_minute(),
                    self.map_log.end_minute(),
                    self.minute_splits
                );
            }
            self.minute = minute;
            self.minute_frames = 0;
            self.minute_splits = 0;
        }
        self.minute_frames += 1;
    }
}

/// One reader's failures, for the log: a run of unreadable frames is logged when it
/// starts (with the reason) and when it ends, and counted for the minute summary.
struct ReadLog {
    name: &'static str,
    /// When the current run of unreadable frames started, and its length.
    failing: Option<(i64, u32)>,
    minute_failures: u32,
    last_error: Option<ReadError>,
    /// When the region last started showing no text at all (ReadError::Blank), while it still does.
    blank_since: Option<i64>,
}

impl ReadLog {
    fn new(name: &'static str) -> Self {
        Self { name, failing: None, minute_failures: 0, last_error: None, blank_since: None }
    }

    fn record<T>(&mut self, t: i64, read: Result<T, ReadError>) -> Option<T> {
        match read {
            Ok(value) => {
                self.blank_since = None;
                if let Some((since, frames)) = self.failing.take() {
                    info!("{} readable again after {frames} unreadable frame(s) over {}s", self.name, t - since);
                }
                Some(value)
            }
            Err(e) => {
                match &mut self.failing {
                    Some((_, frames)) => *frames += 1,
                    None => {
                        warn!("{} unreadable: {e}", self.name);
                        self.failing = Some((t, 1));
                    }
                }
                self.minute_failures += 1;
                self.blank_since = if e == ReadError::Blank { self.blank_since.or(Some(t)) } else { None };
                self.last_error = Some(e);
                None
            }
        }
    }

    /// This minute's failures for the summary line; starts counting the next minute.
    fn end_minute(&mut self) -> String {
        let mut line = format!("{} unreadable {}", self.name, self.minute_failures);
        if let Some(e) = self.last_error.take() {
            line += &format!(" (last: {e})");
        }
        self.minute_failures = 0;
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blank_run_starts_at_its_first_blank_frame_and_ends_on_anything_else() {
        let mut log = ReadLog::new("EXP");
        log.record::<()>(10, Err(ReadError::Blank));
        log.record::<()>(70, Err(ReadError::Blank));
        assert_eq!(log.blank_since, Some(10));
        // Text that doesn't read is something on screen: not a hidden game.
        log.record::<()>(71, Err(ReadError::Parse("1,2".into())));
        assert_eq!(log.blank_since, None);
        log.record::<()>(72, Err(ReadError::Blank));
        log.record(73, Ok(()));
        assert_eq!(log.blank_since, None);
    }
}
