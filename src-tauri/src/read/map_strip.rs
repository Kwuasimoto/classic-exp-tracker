//! Minimap name strip → pixel signature. The signature, not OCR, identifies a map:
//! the game draws a map's name identically every frame, while Windows OCR output
//! varies with upscale factor (verified 2026-10-15).

use super::ReadError;
use crate::geom::Crop;

/// Flat background of the name strip (measured 2026-10-15).
const STRIP_BG: [u8; 3] = [153, 187, 204];
const BG_TOLERANCE: u8 = 6;
/// Text pixels: every channel ≥ this.
const TEXT_MIN_CHANNEL: u8 = 215;
/// A row above the text, used to find where the background starts and the minimap edge.
const EDGE_PROBE_ROW: u32 = 2;
/// Summed per-column difference still considered the same map.
const MATCH_TOLERANCE: u32 = 4;
/// Background columns the minimap leaves after a name: 5–6 on names that fill it
/// (measured 2026-10-17). Fewer means something ended the strip inside the name.
const MIN_MARGIN: usize = 5;

/// Text-pixel count per column, from the start of the background to the minimap's
/// right edge, or to whatever ends the strip first: the map box's edge (`cut`) or a
/// game window drawn over the minimap.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature {
    pub counts: Vec<u8>,
    pub cut: bool,
}

impl Signature {
    /// The name's columns: the strip without the empty background after it, whose
    /// width depends on the minimap and on windows covering it, not on the map.
    fn name(&self) -> &[u8] {
        let len = self.counts.iter().rposition(|&c| c > 0).map_or(0, |i| i + 1);
        &self.counts[..len]
    }

    /// Whether the name may continue past the strip: the map box or a window cut it off.
    fn may_be_cut(&self) -> bool {
        self.cut || self.counts.len() - self.name().len() < MIN_MARGIN
    }

    /// Same map: the names differ by ≤ MATCH_TOLERANCE over the columns both cover,
    /// and are the same width unless the shorter one may be cut off.
    pub fn matches(&self, other: &Signature) -> bool {
        let (short, long) = if self.name().len() <= other.name().len() { (self, other) } else { (other, self) };
        let widths_ok = short.name().len() == long.name().len() || short.may_be_cut();
        widths_ok
            && short.name().iter().zip(long.name()).map(|(a, b)| u32::from(a.abs_diff(*b))).sum::<u32>() <= MATCH_TOLERANCE
    }
}

/// The collapsed minimap's title bar is flat #dddddd behind its black text; the expanded
/// minimap's is white with a blue frame, and a map is never this grey (measured
/// 2026-10-22, 1366x768).
const COLLAPSED_BAR: [u8; 3] = [0xdd, 0xdd, 0xdd];
const COLLAPSED_TOLERANCE: u8 = 10;
/// Share of the bar box that must be that grey (the rest is the bar's text).
const COLLAPSED_SHARE: f32 = 0.5;

/// Whether the minimap is collapsed to its title bar, as the game does in shops.
pub fn is_collapsed_minimap(bar: &Crop) -> bool {
    let total = bar.width as usize * bar.height as usize;
    let grey = (0..bar.height)
        .flat_map(|y| (0..bar.width).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let p = bar.px(x, y);
            (0..3).all(|i| p[i].abs_diff(COLLAPSED_BAR[i]) <= COLLAPSED_TOLERANCE)
        })
        .count();
    total > 0 && grey as f32 >= COLLAPSED_SHARE * total as f32
}

pub fn read_strip(crop: &Crop) -> Result<Signature, ReadError> {
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
    Ok(Signature { counts, cut: edge.is_none() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::fixture;

    #[test]
    fn a_collapsed_minimap_is_its_grey_title_bar() {
        // The bar box at 1366x768 measured in a shop (83% grey) and on a map (0%).
        let filled = |rgb: [u8; 3], grey_rows: u32| {
            let rgba = (0..13u32).flat_map(|y| (0..120).flat_map(move |_| if y < grey_rows { [0xdd, 0xdd, 0xdd, 255] } else { [rgb[0], rgb[1], rgb[2], 255] })).collect();
            Crop::new(120, 13, rgba).expect("sized")
        };
        assert!(is_collapsed_minimap(&filled([0, 0, 0], 11)), "grey behind black text");
        assert!(!is_collapsed_minimap(&filled([0xff, 0xff, 0xff], 0)), "the expanded minimap's white title bar");
    }

    #[test]
    fn reads_the_fixture_signature() {
        let signature = read_strip(&fixture("map_victoria_road_perion.png")).expect("readable");
        // Measured on 2026-10-15: minimap edge 235 columns in, 514 text pixels.
        assert_eq!(signature.counts.len(), 235);
        assert_eq!(signature.counts.iter().map(|&c| u32::from(c)).sum::<u32>(), 514);
        assert!(!signature.cut);
    }

    #[test]
    fn a_narrower_map_box_still_matches_the_same_map() {
        let full = read_strip(&fixture("map_victoria_road_perion.png")).expect("readable");
        let image = fixture("map_victoria_road_perion.png");
        let narrow = image.sub(crate::geom::Rect { x: 0, y: 0, w: image.width - 60, h: image.height }).expect("fits");
        let cut = read_strip(&narrow).expect("readable");
        assert!(cut.cut && cut.matches(&full) && full.matches(&cut));
    }

    #[test]
    fn a_window_over_the_minimap_still_matches_the_same_map() {
        let full = read_strip(&fixture("map_victoria_road_perion.png")).expect("readable");
        let name_len = full.counts.iter().rposition(|&c| c > 0).expect("has text") + 1;
        let ended_at = |len: usize| Signature { counts: full.counts[..len].to_vec(), cut: false };
        // A window over the background after the name (seen 2026-10-17 with the inventory open).
        assert!(ended_at(name_len + 30).matches(&full));
        // A window over the name itself.
        assert!(ended_at(name_len - 20).matches(&full));
        // A whole shorter name is another map, even when a longer name starts with it.
        let mut shorter = full.counts[..name_len - 20].to_vec();
        shorter.extend([0; 30]);
        assert!(!Signature { counts: shorter, cut: false }.matches(&full));
    }
}
