//! A resolution's setup as a file, exported and imported on the Glyphs page: its
//! calibration boxes and learned glyphs, so another PC with the same screen setup reads
//! the same way. Each glyph is its character and centred ink as ASCII art (see
//! read::exp::to_art), as the database stores it. Never pixels from a screenshot.

use serde::{Deserialize, Serialize};

use crate::geom::Calibration;
use crate::read::exp::valid_art;
use crate::read::{GlyphKind, LearnedGlyph};

const FORMAT: &str = "classic-exp-tracker-glyphs";
/// Version 2 added the calibration; version 1 files (glyphs only) still import.
const VERSION: u32 = 2;
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calibration: Option<Calibration>,
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
    pub fn new(
        resolution: &str,
        calibration: Option<Calibration>,
        glyphs: impl IntoIterator<Item = (GlyphKind, LearnedGlyph)>,
    ) -> Self {
        Self {
            format: FORMAT.to_string(),
            version: VERSION,
            resolution: resolution.to_string(),
            calibration,
            glyphs: glyphs.into_iter().map(|(kind, g)| LibraryGlyph { kind, ch: g.ch, art: g.rows }).collect(),
        }
    }

    /// An imported file, checked: its format, resolution, calibration boxes, and every
    /// glyph's character and shape. The reason it was refused otherwise.
    pub fn parse(json: &str) -> Result<Self, String> {
        let library: Self = serde_json::from_str(json).map_err(|e| format!("not a glyph library file: {e}"))?;
        if library.format != FORMAT || !(1..=VERSION).contains(&library.version) {
            return Err(format!("not a glyph library file (format {:?} version {})", library.format, library.version));
        }
        let Some((width, height)) = parse_resolution(&library.resolution) else {
            return Err(format!("{:?} is not a screen resolution", library.resolution));
        };
        if let Some(c) = &library.calibration
            && [Some(c.exp), Some(c.map), c.level].into_iter().flatten().any(|r| !r.fits_in(width, height))
        {
            return Err(format!("its calibration boxes don't fit a {} screen", library.resolution));
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
        let rect = |x| crate::geom::Rect { x, y: 1037, w: 132, h: 16 };
        let calibration = Calibration { exp: rect(1053), map: rect(62), level: None };
        let glyph = (GlyphKind::Exp, LearnedGlyph { ch: '[', rows: art.clone() });
        let json = serde_json::to_string(&Library::new("1920x1080", Some(calibration), [glyph])).unwrap();
        let library = Library::parse(&json).expect("round trip");
        assert_eq!((library.resolution.as_str(), library.glyphs[0].ch), ("1920x1080", '['));
        assert_eq!(library.calibration, Some(calibration));

        let off_screen = json.replace("1053", "1900");
        assert!(Library::parse(&off_screen).is_err(), "the EXP box would run off a 1920-wide screen");

        let level_bracket = json.replace("\"exp\"", "\"level\"");
        assert!(Library::parse(&level_bracket).is_err(), "'[' isn't a level digit");
        let short_row = json.replacen(&"@".to_string(), "", 1);
        assert!(Library::parse(&short_row).is_err(), "rows must be glyph art width");
        assert!(Library::parse(&json.replace("1920x1080", "1920x")).is_err());
        assert!(Library::parse("{}").is_err());
    }
}
