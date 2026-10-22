//! Pure screen readers: in-memory crop in, parsed value out.

pub mod exp;
pub mod font;
pub mod level;
pub mod map_strip;
pub mod ocr;

/// A failed read is a normal outcome (loading screen, alt-tab, covered region).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReadError {
    #[error("region contains no text")]
    Blank,
    /// `line` is the readout with '?' for each unknown glyph, e.g. `1?,176[4.3?%]`.
    #[error("unrecognized glyph at column {column}, read as {line:?}")]
    UnknownGlyph { column: u32, line: String },
    #[error("text {0:?} is not an EXP readout")]
    Parse(String),
}

/// A level digit learned on this PC: its character and
/// centred ink as ASCII art (see level::to_art).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LearnedGlyph {
    pub ch: char,
    pub rows: Vec<String>,
}

