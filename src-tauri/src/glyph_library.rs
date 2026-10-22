//! A resolution's learned level digits as a file, exported and imported on the Glyphs
//! page, so another PC with the same screen reads the same way. Each glyph is its character and centred ink as ASCII art
//! (see read::level::to_art), as the database stores it. Never pixels from a screenshot.

use serde::de::IgnoredAny;
use serde::{Deserialize, Serialize};

use crate::read::level::valid_art;
use crate::read::{GlyphKind, LearnedGlyph};

const FORMAT: &str = "classic-exp-tracker-glyphs";
/// Version 2 added calibration boxes; version 3 (0.3.0) has no EXP glyphs, since EXP is
/// read from the game's font; version 4 (0.4.0) has no boxes, since the app places them.
/// Older files still import (their EXP glyphs and boxes are skipped).
const VERSION: u32 = 4;
/// Far more than a library holds (15 + 10 characters, a few shapes each).
const MAX_GLYPHS: usize = 1_000;
/// Files larger than this aren't read.
pub const MAX_FILE_BYTES: u64 = 2_000_000;
/// Largest monitor side accepted in a resolution.
const MAX_SIDE: u32 = 16_384;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Library {
    format: String,
    version: u32,
    pub resolution: String,
    /// Calibration boxes in a version 2 or 3 file: accepted, not used.
    #[serde(default, skip_serializing)]
    calibration: Option<IgnoredAny>,
    pub glyphs: Vec<LibraryGlyph>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryGlyph {
    pub kind: GlyphKind,
    pub ch: char,
    pub art: Vec<String>,
}

impl Library {
    pub fn new(resolution: &str, glyphs: impl IntoIterator<Item = (GlyphKind, LearnedGlyph)>) -> Self {
        Self {
            format: FORMAT.to_string(),
            version: VERSION,
            resolution: resolution.to_string(),
            calibration: None,
            glyphs: glyphs.into_iter().map(|(kind, g)| LibraryGlyph { kind, ch: g.ch, art: g.rows }).collect(),
        }
    }

    /// Whether the file has calibration boxes from an older version (ignored).
    pub fn has_boxes(&self) -> bool {
        self.calibration.is_some()
    }

    /// An imported file, checked: its format, resolution, and every glyph's character
    /// and shape. The reason it was refused otherwise.
    pub fn parse(json: &str) -> Result<Self, String> {
        // Format and version first: a newer app's file may have fields this one doesn't know.
        let value: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("not a Classic EXP Tracker config file: {e}"))?;
        if value.get("format").and_then(serde_json::Value::as_str) != Some(FORMAT) {
            return Err("not a Classic EXP Tracker config file".to_string());
        }
        match value.get("version").and_then(serde_json::Value::as_u64) {
            Some(v) if v > u64::from(VERSION) => {
                return Err("this config was made by a newer version of Classic EXP Tracker; install the latest version to import it".to_string());
            }
            Some(v) if v >= 1 => {}
            _ => return Err("not a Classic EXP Tracker config file (no valid version)".to_string()),
        }
        let library: Self = serde_json::from_value(value).map_err(|e| format!("this config file is damaged: {e}"))?;
        if parse_resolution(&library.resolution).is_none() {
            return Err(format!("{:?} is not a screen resolution", library.resolution));
        }
        if library.glyphs.len() > MAX_GLYPHS {
            return Err(format!("{} glyphs is more than a library holds ({MAX_GLYPHS})", library.glyphs.len()));
        }
        for (i, glyph) in library.glyphs.iter().enumerate() {
            if !glyph.kind.alphabet().contains(glyph.ch) {
                return Err(format!("glyph {}: {:?} isn't a character of the {} readout", i + 1, glyph.ch, glyph.kind.as_str()));
            }
            if !valid_art(&glyph.art) {
                return Err(format!("glyph {} ({:?}): its shape isn't valid glyph art", i + 1, glyph.ch));
            }
        }
        Ok(library)
    }
}

/// Width and height of `<width>x<height>`, as the app names resolutions (`1920x1080`).
pub fn parse_resolution(s: &str) -> Option<(u32, u32)> {
    let side = |v: &str| v.parse::<u32>().ok().filter(|n| (1..=MAX_SIDE).contains(n) && n.to_string() == v);
    let (w, h) = s.split_once('x')?;
    Some((side(w)?, side(h)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_exported_library_imports_and_bad_files_are_refused() {
        let art = vec![" ".repeat(12) + "@"; 11];
        let glyph = (GlyphKind::Exp, LearnedGlyph { ch: '[', rows: art.clone() });
        let json = serde_json::to_string(&Library::new("1920x1080", [glyph])).unwrap();
        let library = Library::parse(&json).expect("round trip");
        assert_eq!((library.resolution.as_str(), library.glyphs[0].ch), ("1920x1080", '['));
        assert!(!library.has_boxes());

        let level_bracket = json.replace("\"exp\"", "\"level\"");
        assert!(Library::parse(&level_bracket).is_err(), "'[' isn't a level digit");
        let short_row = json.replacen(&"@".to_string(), "", 1);
        assert!(Library::parse(&short_row).is_err(), "rows must be glyph art width");
        assert!(Library::parse(&json.replace("1920x1080", "1920x")).is_err());
        assert!(Library::parse("{}").is_err());
        let newer = json.replace("\"version\":4", "\"version\":5").replace("\"glyphs\"", "\"future\":1,\"glyphs\"");
        assert!(Library::parse(&newer).unwrap_err().contains("newer version"), "a newer app's file says so");
        // A 0.2 file: version 2, with calibration boxes and EXP glyphs.
        let boxes = r#""calibration":{"exp":{"x":1053,"y":1037,"w":132,"h":16},"map":{"x":62,"y":36,"w":309,"h":44},"level":null},"#;
        let older = json.replace("\"version\":4", "\"version\":2").replace("\"glyphs\"", &format!("{boxes}\"glyphs\""));
        let older = Library::parse(&older).expect("an older file imports");
        assert!(older.has_boxes());
        assert_eq!(older.glyphs[0].kind, GlyphKind::Exp);
    }
}
