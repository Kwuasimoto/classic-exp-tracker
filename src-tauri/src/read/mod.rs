//! Pure screen readers: in-memory crop in, parsed value out.

pub mod exp;
pub mod glyphs;
pub mod level;
pub mod map_strip;
pub mod ocr;

use serde::{Deserialize, Serialize};

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

/// A glyph learned on this PC (by the app or from the user): its character and
/// centred ink as ASCII art (see exp::to_art).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LearnedGlyph {
    pub ch: char,
    pub rows: Vec<String>,
}

/// Which readout a glyph belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GlyphKind {
    Exp,
    Level,
}

impl GlyphKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exp => "exp",
            Self::Level => "level",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "exp" => Some(Self::Exp),
            "level" => Some(Self::Level),
            _ => None,
        }
    }

    /// The characters this readout is made of.
    pub fn alphabet(self) -> &'static str {
        match self {
            Self::Exp => "0123456789,.[]%",
            Self::Level => "0123456789",
        }
    }

    /// Whether `glyph` reads as a character with these learned glyphs.
    pub fn recognises(glyph: &exp::Glyph, learned: &[LearnedGlyph]) -> bool {
        exp::classify(glyph, learned).is_some()
    }
}
