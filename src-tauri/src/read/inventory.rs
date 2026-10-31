//! The item inventory (pushed into a corner of the screen) and the quickslot: the meso
//! total, which tab is open, and each slot's item (a patch of its icon) and stack count.
//! Positions are native 1366x768 pixels from the box's top-left corner (the inventory's
//! 209x289 window, geom::NATIVE_BOXES), measured 2026-10-23; on a bigger 16:9 screen each
//! is scaled to where the game draws it (geom::Scale), and the fonts are drawn at that
//! scale, as the EXP reader does.

use crate::geom::{Crop, Rect, Scale};
use crate::read::count::{read_count, Band};
use crate::read::exp::{read_text_line, Ink};
use crate::read::font;

/// A patch's width in pixels: the part of an icon that tells items apart.
pub const PATCH: u32 = 12;
/// Where the patch is in a slot: the icon's upper middle, clear of the count digits.
pub const ICON_PATCH: Rect = Rect { x: 10, y: 4, w: PATCH, h: PATCH };
/// On the keyboard quickslot each key's label (and its white edge) covers the icon's rows
/// 3–11, so the patch is the 8 rows between it and the count, the same on every key.
pub const KEY_PATCH: Rect = Rect { x: 10, y: 12, w: PATCH, h: 8 };
pub const SLOT: u32 = 32;
/// A slot's count rows: the outline row above the digits, their 9 rows and the outline
/// row below (read::count).
pub const COUNT_TOP: u32 = 20;
pub const COUNT_HEIGHT: u32 = 11;

/// An empty inventory slot's flat colour.
const EMPTY_SLOT: [u8; 3] = [225, 223, 213];
/// Inventory slots: 5 columns 36 px apart, 6 rows 34 px apart.
const GRID_AT: (u32, u32) = (8, 50);
const GRID: (u32, u32) = (5, 6);
const PITCH: (u32, u32) = (36, 34);
/// The window's title, "ITEM INVENTORY": black on white at TITLE_AT, 1:1 at native. The
/// window is where the user's corner setting puts it when its title is there.
const TITLE: &[&str] = &[
    "#.#####.#####.#...#....#.#...#.#...#.#####.#...#.#####..###..####..#...#",
    "#...#...#.....##.##....#.##..#.#...#.#.....##..#...#...#...#.#...#..#.#.",
    "#...#...####..#.#.#....#.#.#.#.#...#.####..#.#.#...#...#...#.####....#..",
    "#...#...#.....#...#....#.#..##..#.#..#.....#..##...#...#...#.#...#...#..",
    "#...#...#####.#...#....#.#...#...#...#####.#...#...#....###..#...#...#..",
];
const TITLE_AT: (u32, u32) = (10, 10);

/// Whether the title is drawn where it should be: the title bitmap drawn at the screen's
/// scale and compared with the screen (brightness inverted, so the text is bright) as the
/// EXP readout's glyphs are.
fn title_found(crop: &Crop, at: &Scale) -> bool {
    const MARGIN: u32 = 2;
    let (w, h) = (TITLE[0].len() as u32 + 2 * MARGIN, TITLE.len() as u32 + 2 * MARGIN);
    let Some((area, left, top)) = at.around(crop, Rect { x: TITLE_AT.0 - MARGIN, y: TITLE_AT.1 - MARGIN, w, h }) else { return false };
    let lums: Vec<f32> = area
        .rgba
        .chunks(4)
        .map(|p| 255.0 - (0.299 * f32::from(p[0]) + 0.587 * f32::from(p[1]) + 0.114 * f32::from(p[2])))
        .collect();
    let p = at.font_px();
    let margin = MARGIN as f32 * p;
    let drawn = font::render(TITLE, p, left + margin, top + margin, area.width as usize, area.height as usize);
    font::ncc(&lums, &drawn) >= font::MIN_SCORE
}

/// The meso total's white field, inside its border.
const MESO_FIELD: Rect = Rect { x: 25, y: 268, w: 143, h: 14 };

/// The inventory's tabs, left to right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Equip,
    Use,
    Setup,
    Etc,
    Cash,
    Deco,
}

const TABS: [Tab; 6] = [Tab::Equip, Tab::Use, Tab::Setup, Tab::Etc, Tab::Cash, Tab::Deco];

/// Tab `i`'s box, border to border: tabs are 33 px wide side by side. The selected tab
/// stands 1 px taller (from row 23; the others from 24), and is pink inside (a paler
/// pink while the game isn't focused) where the others are grey.
pub fn tab_box(i: u32) -> Rect {
    Rect { x: 5 + 33 * i, y: 23, w: 33, h: 19 }
}

/// The scrollbar's track, below its up arrow and above its down arrow; its thumb is blue.
const SCROLL_TRACK_X: u32 = 196;
const SCROLL_TRACK: std::ops::RangeInclusive<u32> = 61..=240;

/// A pixel is pink when its red is this far above its green; grey tabs have them equal.
const PINK_BY: u8 = 25;
/// Share of a tab's box that's pink while it's selected (about 0.7 measured; 0 otherwise).
const SELECTED_SHARE: f32 = 0.5;

/// Controller quickslot slots that can hold an item, inside the quickslot box.
pub const CONTROLLER_SLOTS: [(u32, u32); 4] = [(19, 50), (53, 34), (53, 66), (87, 50)];
/// Keyboard quickslot slots: 4 keys by 2, 35 px apart, measured 2026-10-28.
pub const KEY_SLOTS: [(u32, u32); 8] = [(81, 29), (116, 29), (151, 29), (186, 29), (81, 63), (116, 63), (151, 63), (186, 63)];
/// The middles of the gaps between the keys (3 px wide), drawn KEY_GAP on the keyboard
/// layout only: down between the columns, and across between the rows.
const KEY_GAPS: [(u32, u32); 10] = [(113, 40), (148, 40), (183, 40), (113, 84), (148, 84), (183, 84), (96, 62), (131, 62), (166, 62), (201, 62)];
const KEY_GAP: [u8; 3] = [23, 23, 23];

/// An item's patch: RGB pixels, PATCH wide, row by row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Patch(pub Vec<u8>);

/// Two patches' mean difference per channel; 0 for the same icon.
pub fn patch_distance(a: &[u8], b: &[u8]) -> f32 {
    if a.len() != b.len() {
        return f32::INFINITY;
    }
    let sum: u32 = a.iter().zip(b).map(|(x, y)| u32::from(x.abs_diff(*y))).sum();
    sum as f32 / a.len().max(1) as f32
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SlotError {
    #[error("the slot holds a stack whose count doesn't read")]
    CountUnreadable,
    #[error("the slot isn't inside the box")]
    OutsideBox,
}

/// A slot holding a stack: its item's patch and the count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotRead {
    pub patch: Patch,
    pub count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InventoryRead {
    /// The window's title is where it should be: the inventory is in the chosen corner.
    pub found: bool,
    /// `None` when the meso field doesn't read.
    pub meso: Option<u64>,
    /// The selected tab, `None` when no tab looks selected.
    pub tab: Option<Tab>,
    /// The scrollbar thumb's top row: which slots the tab shows. `None` without a thumb.
    pub scroll: Option<u32>,
    /// The visible slots holding a stack whose count reads. A slot whose count doesn't
    /// read (covered by a tooltip, say) makes the whole read `None` in `slots`, so a
    /// missing stack never looks like a stack used up.
    pub slots: Option<Vec<SlotRead>>,
}

fn same(p: [u8; 4], c: [u8; 3], tolerance: u8) -> bool {
    (0..3).all(|i| p[i].abs_diff(c[i]) <= tolerance)
}

/// The slot at native `(x, y)` in `crop`: `Ok(None)` when it holds nothing with a count,
/// an error when it holds something whose count doesn't read. The patch (`patch`, from the
/// slot's corner) is the icon's native pixels (geom::Scale::native_rgb), so one learned
/// patch serves every 16:9 screen.
pub fn read_slot(crop: &Crop, at: &Scale, x: u32, y: u32, patch: Rect) -> Result<Option<SlotRead>, SlotError> {
    let (band, left, top) = at.around(crop, Rect { x, y: y + COUNT_TOP, w: SLOT, h: COUNT_HEIGHT }).ok_or(SlotError::OutsideBox)?;
    let Some(count) = read_count(&Band::new(&band, left, top), at.font_px()).map_err(|_| SlotError::CountUnreadable)? else {
        return Ok(None);
    };
    let rgb = at.native_rgb(crop, Rect { x: x + patch.x, y: y + patch.y, ..patch }).ok_or(SlotError::OutsideBox)?;
    Ok(Some(SlotRead { patch: Patch(rgb), count }))
}

/// The meso total's text: digits with a comma every three from the right.
pub fn parse_mesos(text: &str) -> Option<u64> {
    let groups: Vec<&str> = text.split(',').collect();
    let well_grouped = groups.iter().enumerate().all(|(i, g)| {
        let digits = g.len() == 3 || (i == 0 && (1..=3).contains(&g.len()));
        digits && g.bytes().all(|b| b.is_ascii_digit())
    });
    if !well_grouped || (groups[0].starts_with('0') && text != "0") {
        return None;
    }
    groups.concat().parse().ok()
}

/// The visible inventory slots' top-left corners, row by row.
pub fn inventory_slots() -> impl Iterator<Item = (u32, u32)> {
    (0..GRID.1).flat_map(|r| (0..GRID.0).map(move |c| (GRID_AT.0 + c * PITCH.0, GRID_AT.1 + r * PITCH.1)))
}

/// The meso total's field: inside it only, as its dark border, smoothed in, would read as ink.
pub fn meso_field(crop: &Crop, at: &Scale) -> Option<Crop> {
    at.inside(crop, MESO_FIELD)
}

/// The inventory in `crop`, its native pixels landing as `at` says.
pub fn read_inventory(crop: &Crop, at: &Scale) -> InventoryRead {
    let meso = meso_field(crop, at)
        .and_then(|field| read_text_line(&field, &font::MESO, Ink::Dark).ok())
        .and_then(|line| parse_mesos(&line.text()));
    let tab = (0..TABS.len() as u32).find(|&i| {
        at.inside(crop, tab_box(i)).is_some_and(|tab| {
            let pink = tab.rgba.chunks(4).filter(|p| p[0] >= p[1].saturating_add(PINK_BY)).count();
            pink as f32 >= SELECTED_SHARE * (tab.width * tab.height) as f32
        })
    });
    let tab = tab.map(|i| TABS[i as usize]);
    let mut slots = Some(Vec::new());
    for (x, y) in inventory_slots() {
        if at.inside(crop, Rect { x, y, w: SLOT, h: SLOT }).is_some_and(|s| is_empty_slot(&s)) {
            continue;
        }
        match read_slot(crop, at, x, y, ICON_PATCH) {
            Ok(Some(slot)) => slots.as_mut().map_or((), |s| s.push(slot)),
            Ok(None) => {}
            Err(_) => slots = None,
        }
    }
    let scroll = SCROLL_TRACK.into_iter().find(|&y| at.pixel(crop, SCROLL_TRACK_X, y).is_some_and(|px| px[2] > px[0].saturating_add(40)));
    InventoryRead { found: title_found(crop, at), meso, tab, scroll, slots }
}

fn is_empty_slot(slot: &Crop) -> bool {
    slot.rgba.chunks(4).all(|p| same([p[0], p[1], p[2], 255], EMPTY_SLOT, 0))
}

/// The game shows the keyboard quickslot in `crop`, told by its dark gaps between the keys.
pub fn keyboard_layout(crop: &Crop, at: &Scale) -> bool {
    KEY_GAPS.iter().all(|&(x, y)| at.pixel(crop, x, y).is_some_and(|p| same(p, KEY_GAP, 12)))
}

/// The quickslot layout the game shows in `crop`: its slots and where their patches are.
/// Anything but the keyboard layout is read as the controller layout.
pub fn quickslot_layout(crop: &Crop, at: &Scale) -> (&'static [(u32, u32)], Rect) {
    if keyboard_layout(crop, at) {
        (&KEY_SLOTS, KEY_PATCH)
    } else {
        (&CONTROLLER_SLOTS, ICON_PATCH)
    }
}

/// The quickslot box (geom::Boxes::quickslot): its stacks, or `None` when a count is there
/// but doesn't read, or no stack is. `None` keeps the last reading, so potions used
/// meanwhile count once the stacks are back.
pub fn read_quickslot(crop: &Crop, at: &Scale) -> Option<Vec<SlotRead>> {
    let (slots, patch) = quickslot_layout(crop, at);
    let mut stacks = Vec::new();
    for &(x, y) in slots {
        if let Some(slot) = read_slot(crop, at, x, y, patch).ok()? {
            stacks.push(slot);
        }
    }
    (!stacks.is_empty()).then_some(stacks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::fixture;

    #[test]
    fn reads_the_inventory_and_quickslot_at_1366x768() {
        let read = read_inventory(&fixture("inventory_etc_240529.png"), &Scale::NATIVE);
        assert_eq!((read.found, read.meso, read.tab), (true, Some(240_529), Some(Tab::Etc)));
        // A box that isn't the inventory (the quickslot's) has no title.
        assert!(!read_inventory(&fixture("quickslot_171_244_868_29.png"), &Scale::NATIVE).found);
        let counts: Vec<u32> = read.slots.unwrap().iter().map(|s| s.count).collect();
        assert_eq!(counts, [1, 3, 22, 2, 100, 9, 1, 36, 3, 11, 33, 24, 45, 24, 3, 35, 13, 15, 55, 17, 3, 36, 23, 16, 1, 3, 1, 200, 200, 13]);
        // The meat's white bone shows inside and between the 244's digits.
        let quick: Vec<u32> = read_quickslot(&fixture("quickslot_171_244_868_29.png"), &Scale::NATIVE).unwrap().iter().map(|s| s.count).collect();
        assert_eq!(quick, [171, 244, 868, 29]);
        // The keyboard layout, Ctrl holding a skill. On a white key the 3s of 137 and 63
        // look like a 9 and an 8, the key showing through their open side.
        let keys = read_quickslot(&fixture("quickslot_keys_29_342_24_30_851_137_63.png"), &Scale::NATIVE).unwrap();
        assert_eq!(keys.iter().map(|s| s.count).collect::<Vec<_>>(), [29, 342, 24, 30, 851, 137, 63]);
        // Seven different items: seven patches no two alike.
        assert!(keys.iter().enumerate().all(|(i, a)| keys[i + 1..].iter().all(|b| patch_distance(&a.patch.0, &b.patch.0) > 3.0)));
    }

    #[test]
    fn reads_the_inventory_stretched_to_1920x1080() {
        // The inventory's box from a real 1920x1080 frame, the inventory bottom right.
        let at = crate::geom::boxes_for(1920, 1080, crate::geom::Corner::BottomRight).expect("16:9").inventory_at;
        let read = read_inventory(&fixture("inventory_1920x1080_bottom_right_12384.png"), &at);
        assert_eq!((read.found, read.meso, read.tab), (true, Some(12_384), Some(Tab::Etc)));
        let counts: Vec<u32> = read.slots.unwrap().iter().map(|s| s.count).collect();
        assert_eq!(counts, [18, 2, 1, 17, 2, 9, 2, 3, 11, 1, 8, 135, 1, 3, 30, 2, 1, 16, 4, 2, 1, 1, 49, 1]);
        // A meso field from the same screen whose 8 smoothing split in two (read "418,011").
        let line = read_text_line(&fixture("meso_field_1920x1080_48011.png"), &font::MESO, Ink::Dark).unwrap();
        assert_eq!(parse_mesos(&line.text()), Some(48_011));
    }
}
