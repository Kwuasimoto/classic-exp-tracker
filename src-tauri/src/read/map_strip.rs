//! Minimap name strip → pixel signature. The signature, not OCR, identifies a map:
//! the game draws a map's name identically every frame, while Windows OCR output
//! varies with upscale factor (verified 2026-10-15).

use super::ReadError;
use crate::geom::Crop;

/// Flat background of the name strip (measured 2026-10-15).
pub const STRIP_BG: [u8; 3] = [153, 187, 204];
const BG_TOLERANCE: u8 = 6;
/// Text pixels: every channel ≥ this.
const TEXT_MIN_CHANNEL: u8 = 215;
/// A row above the text, used to find where the background starts and the minimap edge.
const EDGE_PROBE_ROW: u32 = 2;
/// Summed per-column difference still considered the same map.
const MATCH_TOLERANCE: u32 = 4;

/// Text-pixel count per column, from the start of the background to the minimap's
/// right edge, or to the map box's edge when the box is narrower (`cut`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature {
    pub counts: Vec<u8>,
    pub cut: bool,
}

impl Signature {
    /// Same map: the columns both signatures cover differ by ≤ MATCH_TOLERANCE, and
    /// they cover the same width unless the shorter one was cut by its box (so a
    /// wider or narrower map box still finds the same map).
    pub fn matches(&self, other: &Signature) -> bool {
        let (short, long) = if self.counts.len() <= other.counts.len() { (self, other) } else { (other, self) };
        let widths_ok = short.counts.len() == long.counts.len() || short.cut;
        widths_ok
            && short.counts.iter().zip(&long.counts).map(|(a, b)| u32::from(a.abs_diff(*b))).sum::<u32>() <= MATCH_TOLERANCE
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StripRead {
    pub signature: Signature,
}

pub fn read_strip(crop: &Crop) -> Result<StripRead, ReadError> {
    if crop.height <= EDGE_PROBE_ROW {
        return Err(ReadError::Blank);
    }
    let is_bg = |x: u32| {
        let p = crop.px(x, EDGE_PROBE_ROW);
        (0..3).all(|i| p[i].abs_diff(STRIP_BG[i]) <= BG_TOLERANCE)
    };
    let start = (0..crop.width).find(|&x| is_bg(x)).ok_or(ReadError::Blank)?;
    let edge = (start..crop.width).find(|&x| !is_bg(x));
    let end = edge.unwrap_or(crop.width);
    let counts: Vec<u8> = (start..end)
        .map(|x| {
            let n = (0..crop.height)
                .filter(|&y| {
                    let p = crop.px(x, y);
                    p[0].min(p[1]).min(p[2]) >= TEXT_MIN_CHANNEL
                })
                .count();
            u8::try_from(n).unwrap_or(u8::MAX)
        })
        .collect();
    if counts.iter().all(|&c| c == 0) {
        return Err(ReadError::Blank);
    }
    Ok(StripRead { signature: Signature { counts, cut: edge.is_none() } })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::fixture;

    #[test]
    fn reads_the_fixture_signature() {
        let read = read_strip(&fixture("map_victoria_road_perion.png")).expect("readable");
        // Measured on 2026-10-15: minimap edge 235 columns in, 514 text pixels.
        assert_eq!(read.signature.counts.len(), 235);
        assert_eq!(read.signature.counts.iter().map(|&c| u32::from(c)).sum::<u32>(), 514);
        assert!(!read.signature.cut);
    }

    #[test]
    fn a_narrower_map_box_still_matches_the_same_map() {
        let full = read_strip(&fixture("map_victoria_road_perion.png")).expect("readable").signature;
        let image = fixture("map_victoria_road_perion.png");
        let narrow = image.sub(crate::geom::Rect { x: 0, y: 0, w: image.width - 60, h: image.height }).expect("fits");
        let cut = read_strip(&narrow).expect("readable").signature;
        assert!(cut.cut && cut.matches(&full) && full.matches(&cut));
        let other = Signature { counts: full.counts[..cut.counts.len()].to_vec(), cut: false };
        assert!(!other.matches(&full), "an uncut signature of a different width is a different map");
    }
}
