//! The level box across frames. It's re-read only when its pixels change (about
//! once per level), and its level is trusted once it has held still for
//! STEADY_FRAMES: one bad frame (a loading screen, a window passing over the game)
//! can read as another level. Digits it can't read yet are learned from the level
//! the tracker worked out from the EXP readings (see read/level.rs).

use log::{info, warn};

use crate::geom::Crop;
use crate::read::level::{self, classify, Glyph};
use crate::read::LearnedGlyph;

const STEADY_FRAMES: u32 = 3;
/// Accepted EXP readings, with the box and the tracker's level both unchanged,
/// before an unknown digit is learned: right after a level-up the box can show the
/// new level before the EXP readout confirms it.
const LEARN_AFTER_READS: u32 = 3;
/// Frames the box must hold still before digits it can't read go to the Glyphs page:
/// long enough for learning from the EXP readings to have had its chance.
const REPORT_UNKNOWN_AFTER_FRAMES: u32 = 30;

#[derive(Default)]
pub struct LevelBox {
    crop: Option<Crop>,
    glyphs: Vec<Glyph>,
    /// What the box reads as with the digits learned so far.
    reads_as: Option<u32>,
    /// Consecutive frames the box has looked the same, including this one.
    still_frames: u32,
    learned: Vec<LearnedGlyph>,
    /// Whether the box's current pixels were already tried for learning.
    learn_tried: bool,
    /// Accepted EXP readings since the box last changed, all at `steady_level`.
    steady_reads: u32,
    steady_level: Option<u32>,
    /// Whether the box's current pixels were already reported as unknown.
    unknown_reported: bool,
}

impl LevelBox {
    /// This frame's box; `None` without a level box.
    pub fn observe(&mut self, crop: Option<Crop>) {
        if crop == self.crop {
            self.still_frames = self.still_frames.saturating_add(1);
            return;
        }
        self.still_frames = 1;
        self.glyphs = crop.as_ref().and_then(|c| level::segment(c).ok()).unwrap_or_default();
        self.crop = crop;
        self.learn_tried = false;
        self.steady_reads = 0;
        self.unknown_reported = false;
        self.reread();
    }

    /// An accepted EXP reading, with the tracker at `level`.
    pub fn accepted_reading(&mut self, level: Option<u32>) {
        if level == self.steady_level {
            self.steady_reads += 1;
        } else {
            self.steady_level = level;
            self.steady_reads = 1;
        }
    }

    /// Learn the digits the box shows but can't read yet, labelled from `level`, once
    /// the box and the level have held over LEARN_AFTER_READS accepted readings.
    /// Returns the new digits, already in use.
    pub fn learn(&mut self, level: u32) -> Vec<LearnedGlyph> {
        let due = self.reads_as.is_none()
            && !self.glyphs.is_empty()
            && !self.learn_tried
            && self.steady_reads >= LEARN_AFTER_READS
            && self.steady_level == Some(level);
        if !due {
            return Vec::new();
        }
        self.learn_tried = true;
        match level::learn(&self.glyphs, &self.learned, level) {
            Ok(new) => {
                for glyph in &new {
                    info!("learned level digit {:?} from level {level}", glyph.ch);
                }
                self.learned.extend(new.iter().cloned());
                self.reread();
                new
            }
            Err(why) => {
                warn!("not learning level digits: {why}");
                Vec::new()
            }
        }
    }

    /// Replace the learned digits; the box is read again on the next frame.
    pub fn set_learned(&mut self, digits: Vec<LearnedGlyph>) {
        self.learned = digits;
        self.crop = None;
        self.reads_as = None;
    }

    /// The digits no learned glyph matches, once per box that has held still for
    /// REPORT_UNKNOWN_AFTER_FRAMES and shows one or two glyphs (a level, not something
    /// passing over the box). For the Glyphs page.
    pub fn take_unknown(&mut self) -> Vec<Glyph> {
        if self.unknown_reported || self.still_frames < REPORT_UNKNOWN_AFTER_FRAMES || !(1..=2).contains(&self.glyphs.len()) {
            return Vec::new();
        }
        self.unknown_reported = true;
        self.glyphs.iter().filter(|g| classify(g, &self.learned).is_none()).cloned().collect()
    }

    /// The level the box shows, once it has held still.
    pub fn level(&self) -> Option<u32> {
        self.reads_as.filter(|_| self.still_frames >= STEADY_FRAMES)
    }

    fn reread(&mut self) {
        let read = level::read_level(&self.glyphs, &self.learned);
        if read.is_some() && read != self.reads_as {
            info!("level box reads {read:?}");
        }
        self.reads_as = read;
    }
}
