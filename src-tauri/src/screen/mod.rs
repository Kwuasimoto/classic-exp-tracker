//! What the screen shows, frame by frame: reads the calibrated regions, debounces
//! map changes, follows the level box, and logs reader failures. No I/O apart from
//! log lines; pipeline.rs saves what this module finds.

mod level_box;

use log::{info, warn};

use crate::geom::Crop;
use crate::read::exp::{read_exp, ExpReading};
use crate::read::level::LevelGlyph;
use crate::read::map_strip::{read_strip, Signature};
use crate::read::ReadError;
use crate::tracker::{ConfirmOutcome, MapConfirmer, Tracker};
use level_box::LevelBox;

/// One frame's regions, read.
pub struct FrameRead {
    pub exp: Option<ExpReading>,
    pub map_readable: bool,
    /// A map confirmed on this frame, with the strips that confirmed it (for OCR).
    pub new_map: Option<(Signature, Vec<Crop>)>,
}

pub struct Screen {
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
            confirmer: MapConfirmer::new(),
            level_box: LevelBox::default(),
            exp_log: ReadLog::new("EXP"),
            map_log: ReadLog::new("map strip"),
            minute: 0,
            minute_frames: 0,
        }
    }

    pub fn read(&mut self, t: i64, exp: &Crop, map: Crop, level: Option<Crop>) -> FrameRead {
        self.count_frame(t);
        let exp = self.exp_log.record(t, read_exp(exp));
        let signature = self.map_log.record(t, read_strip(&map).map(|strip| strip.signature));
        let map_readable = signature.is_some();
        let new_map = signature.and_then(|signature| match self.confirmer.feed(signature, map) {
            ConfirmOutcome::Changed { signature, strips } => Some((signature, strips)),
            ConfirmOutcome::Unchanged => None,
        });
        self.level_box.observe(level);
        FrameRead { exp, map_readable, new_map }
    }

    /// After the tracker took this frame's sample: count an accepted EXP reading
    /// toward learning level digits, learn any that are due, and give the tracker the
    /// level the box shows. Returns the digits learned (already in use), to save.
    pub fn sync_level(&mut self, tracker: &mut Tracker, exp: Option<ExpReading>) -> Vec<LevelGlyph> {
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

    /// Replace the learned level digits: those saved earlier, or none to relearn them.
    pub fn set_level_digits(&mut self, digits: Vec<LevelGlyph>) {
        self.level_box.set_learned(digits);
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
