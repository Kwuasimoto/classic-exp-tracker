//! Pure screen readers: in-memory crop in, parsed value out.

pub mod exp;
pub mod glyphs;
pub mod map_strip;
pub mod ocr;

/// A failed read is a normal outcome (loading screen, alt-tab, covered region).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReadError {
    #[error("region contains no text")]
    Blank,
    #[error("unrecognized glyph at column {0}")]
    UnknownGlyph(u32),
    #[error("text {0:?} is not an EXP readout")]
    Parse(String),
}
