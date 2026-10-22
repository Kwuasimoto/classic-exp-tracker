//! What the screen shows, frame by frame: reads the calibrated regions, learns EXP
//! glyphs it doesn't know yet, debounces map changes, follows the level box, and logs
//! reader failures. No I/O apart from log lines; pipeline.rs saves what this module finds.

mod exp_learner;
mod level_box;

use std::ops::RangeInclusive;

use log::{info, warn};

use crate::geom::Crop;
use crate::read::exp::{read_exp, read_line, reading, ExpReading, Glyph};
use crate::read::map_strip::{read_strip, Signature};
use crate::read::{GlyphKind, LearnedGlyph, ReadError};
use crate::tracker::{ConfirmOutcome, MapConfirmer, Tracker};
use level_box::LevelBox;

/// Frames the EXP box must look the same before its unknown glyphs are learned or
/// reported, so a readout caught mid-change isn't.
const EXP_STEADY_FRAMES: u32 = 3;
/// Glyphs in a real readout, `0[0.00%]` … `9,999,999,999[99.99%]`; anything else in
/// the box (a window over it) isn't learned from or reported.
const READOUT_GLYPHS: RangeInclusive<usize> = 8..=21;

/// One frame's regions, read.
pub struct FrameRead {
    pub exp: Option<ExpReading>,
    /// The EXP readout has glyphs that no known glyph matches.
    pub exp_unknown: bool,
    pub map_readable: bool,
    /// A map confirmed on this frame, with the strips that confirmed it (for OCR).
    pub new_map: Option<(Signature, Vec<Crop>)>,
    /// EXP glyphs learned on this frame (already in use), to save.
    pub learned_exp: Vec<LearnedGlyph>,
    /// Shapes no glyph matches, for the Glyphs page.
    pub unknown: Vec<(GlyphKind, Glyph)>,
}

/// The EXP box on one frame: its reading, the glyphs learned from it, and the
/// glyphs reported as unknown.
struct ExpBox {
    read: Result<ExpReading, ReadError>,
    learned: Vec<LearnedGlyph>,
    unknown: Vec<(GlyphKind, Glyph)>,
}

impl ExpBox {
    fn read(read: Result<ExpReading, ReadError>) -> Self {
        Self { read, learned: Vec::new(), unknown: Vec::new() }
    }
}

pub struct Screen {
    /// EXP glyphs learned at this resolution, used with the built-in ones.
    exp_glyphs: Vec<LearnedGlyph>,
    /// The EXP box's pixels and how many frames in a row they've looked the same.
    exp_crop: Option<Crop>,
    exp_still: u32,
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
            exp_glyphs: Vec::new(),
            exp_crop: None,
            exp_still: 0,
            confirmer: MapConfirmer::new(),
            level_box: LevelBox::default(),
            exp_log: ReadLog::new("EXP"),
            map_log: ReadLog::new("map strip"),
            minute: 0,
            minute_frames: 0,
        }
    }

    /// Read this frame's regions; `tracker` (before this frame's sample) tells which
    /// characters an unknown EXP glyph could be.
    pub fn read(&mut self, t: i64, exp: &Crop, map: Crop, level: Option<Crop>, tracker: &Tracker) -> FrameRead {
        self.count_frame(t);
        let ExpBox { read, learned: learned_exp, mut unknown } = self.read_exp_box(exp, tracker);
        let exp_unknown = matches!(read, Err(ReadError::UnknownGlyph { .. }));
        let exp = self.exp_log.record(t, read);
        let signature = self.map_log.record(t, read_strip(&map).map(|strip| strip.signature));
        let map_readable = signature.is_some();
        let new_map = signature.and_then(|signature| match self.confirmer.feed(signature, map) {
            ConfirmOutcome::Changed { signature, strips } => Some((signature, strips)),
            ConfirmOutcome::Unchanged => None,
        });
        self.level_box.observe(level);
        unknown.extend(self.level_box.take_unknown().into_iter().map(|g| (GlyphKind::Level, g)));
        FrameRead { exp, exp_unknown, map_readable, new_map, learned_exp, unknown }
    }

    /// The EXP readout, and once the box has held still with glyphs it can't read:
    /// those glyphs learned (when only one character fits), or else reported.
    fn read_exp_box(&mut self, exp: &Crop, tracker: &Tracker) -> ExpBox {
        if self.exp_crop.as_ref() == Some(exp) {
            self.exp_still = self.exp_still.saturating_add(1);
        } else {
            self.exp_crop = Some(exp.clone());
            self.exp_still = 1;
        }
        let line = match read_line(exp, &self.exp_glyphs) {
            Ok(line) => line,
            Err(e) => return ExpBox::read(Err(e)),
        };
        let has_unknown = line.iter().any(|(_, ch)| ch.is_none());
        if !has_unknown || self.exp_still != EXP_STEADY_FRAMES || !READOUT_GLYPHS.contains(&line.len()) {
            return ExpBox::read(reading(&line));
        }
        match exp_learner::learn(&line, |r| tracker.plausible(r)) {
            Some(learned) => {
                for glyph in &learned {
                    info!("learned EXP glyph {:?}", glyph.ch);
                }
                self.exp_glyphs.extend(learned.iter().cloned());
                ExpBox { read: read_exp(exp, &self.exp_glyphs), learned, unknown: Vec::new() }
            }
            None => {
                let read = reading(&line);
                let unknown = line.into_iter().filter(|(_, ch)| ch.is_none()).map(|(g, _)| (GlyphKind::Exp, g)).collect();
                ExpBox { read, learned: Vec::new(), unknown }
            }
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

    /// Use the glyphs learned at the current resolution. The EXP and level boxes are
    /// read afresh with them.
    pub fn set_glyphs(&mut self, exp: Vec<LearnedGlyph>, level: Vec<LearnedGlyph>) {
        self.exp_glyphs = exp;
        self.exp_crop = None;
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
                info!("minute summary: {} frames, {}, {}", self.minute_frames, self.exp_log.end_minute(), self.map_log.end_minute());
            }
            self.minute = minute;
            self.minute_frames = 0;
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
}

impl ReadLog {
    fn new(name: &'static str) -> Self {
        Self { name, failing: None, minute_failures: 0, last_error: None }
    }

    fn record<T>(&mut self, t: i64, read: Result<T, ReadError>) -> Option<T> {
        match read {
            Ok(value) => {
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
