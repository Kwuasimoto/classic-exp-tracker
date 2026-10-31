//! Mesos made, from what the inventory and quickslot show: meso pickups, Etc items picked
//! up (at their NPC sell price) and potions used (at their shop price). Pure: readings in,
//! loot out.
//!
//! A reading counts once it reads the same twice in a row, so the cursor or a highlight
//! passing over a slot doesn't look like a change. Only what goes up (mesos, Etc) or down
//! (potions) counts: selling, dropping, buying and moving items between slots don't.
//! In a shop nothing counts, since everything that changes there is a trade.

use std::collections::BTreeMap;

use log::warn;

use crate::rates::Loot;

pub type ItemId = i64;

/// An item's count across the slots it's in, and what one is worth (0 until named).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stack {
    pub count: u32,
    pub price: u64,
}

pub type Stacks = BTreeMap<ItemId, Stack>;

/// The Etc tab's visible stacks, and where it's scrolled to: another scroll position
/// shows other slots, so its stacks aren't compared with these.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EtcView {
    pub scroll: Option<u32>,
    pub stacks: Stacks,
}

/// One frame of the inventory and quickslot; `None` where it couldn't be read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LootFrame {
    pub meso: Option<u64>,
    /// The Etc tab's items, `None` while another tab is open.
    pub etc: Option<EtcView>,
    pub potions: Option<Stacks>,
    pub shopping: bool,
}

/// One reading: the last confirmed value, and a new value waiting for its second read.
#[derive(Debug)]
struct Confirmed<T> {
    value: Option<T>,
    pending: Option<T>,
}

impl<T> Default for Confirmed<T> {
    fn default() -> Self {
        Self { value: None, pending: None }
    }
}

impl<T: Clone + PartialEq> Confirmed<T> {
    /// The previous and new value when `read` confirms a change; `None` otherwise,
    /// including the first value ever confirmed (nothing to compare it with).
    fn observe(&mut self, read: Option<T>) -> Option<(T, T)> {
        let Some(read) = read else {
            self.pending = None;
            return None;
        };
        if self.value.as_ref() == Some(&read) {
            self.pending = None;
            return None;
        }
        if self.pending.as_ref() != Some(&read) {
            self.pending = Some(read);
            return None;
        }
        self.pending = None;
        self.value.replace(read.clone()).map(|old| (old, read))
    }
}

#[derive(Debug, Default)]
pub struct LootTracker {
    meso: Confirmed<u64>,
    etc: Confirmed<EtcView>,
    potions: Confirmed<Stacks>,
}

/// Some of one item counted: picked up (Etc) or used (potions), and what they're worth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    pub item: ItemId,
    pub count: u32,
    pub value: u64,
}

/// What one frame counted: the loot, and the Etc pickups it's made of.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Counted {
    pub loot: Loot,
    pub pickups: Vec<Change>,
    /// The meso total and the Etc tab read, both at their confirmed values: everything
    /// made up to now is counted. False while the inventory is closed (or on another tab),
    /// and on a first read that differs, until it's confirmed and its catch-up is counted.
    pub seen: bool,
}

impl LootTracker {
    pub fn on_frame(&mut self, frame: LootFrame) -> Counted {
        let read = frame.meso.is_some() && frame.etc.is_some();
        let meso = self.meso.observe(frame.meso);
        let etc = self.etc.observe(frame.etc);
        let potions = self.potions.observe(frame.potions);
        // Read, and nothing waiting for a second read: both are at their confirmed values.
        let seen = read && self.meso.pending.is_none() && self.etc.pending.is_none();
        if frame.shopping {
            return Counted { seen, ..Counted::default() };
        }
        let pickups = match etc {
            // Scrolled: other slots are showing; this view is the new baseline.
            Some((old, new)) if old.scroll == new.scroll => changes("Etc picked up", &old.stacks, &new.stacks, Missing::CountsFromZero),
            _ => Vec::new(),
        };
        let used = potions.map_or_else(Vec::new, |(old, new)| changes("potions used", &new, &old, Missing::CountsNothing));
        let total = |c: &[Change]| c.iter().map(|c| c.value).sum();
        Counted {
            loot: Loot {
                meso: meso.map_or(0, |(old, new)| new.saturating_sub(old)),
                etc_value: total(&pickups),
                potion_cost: total(&used),
            },
            pickups,
            seen,
        }
    }
}

/// An item in `more` that `less` doesn't have.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Missing {
    /// A new Etc item picked up: all of its stack counts.
    CountsFromZero,
    /// A potion that was on the quickslot and isn't now: moved off it, or the quickslot
    /// shows the other layout. It doesn't count.
    CountsNothing,
}

/// The items `more` has beyond `less`, each with what it's worth at `more`'s price.
fn changes(what: &str, less: &Stacks, more: &Stacks, missing: Missing) -> Vec<Change> {
    more.iter()
        .filter_map(|(&item, s)| {
            let before = match less.get(&item) {
                Some(l) => l.count,
                None if missing == Missing::CountsFromZero => 0,
                None => return None,
            };
            let count = s.count.saturating_sub(before);
            let value = u64::from(count) * s.price;
            if count > LOGGED_CHANGE {
                warn!("{what}: item #{item} {before} -> {} counted at once, {value} mesos", s.count);
            }
            (count > 0).then_some(Change { item, count, value })
        })
        .collect()
}

/// A change this big between two readings is logged: more than a few seconds of play
/// would make.
const LOGGED_CHANGE: u32 = 20;

#[cfg(test)]
mod tests {
    use super::*;

    fn stacks(items: &[(ItemId, u32, u64)]) -> Option<Stacks> {
        Some(items.iter().map(|&(id, count, price)| (id, Stack { count, price })).collect())
    }

    fn view(scroll: u32, items: &[(ItemId, u32, u64)]) -> Option<EtcView> {
        Some(EtcView { scroll: Some(scroll), stacks: stacks(items)? })
    }

    /// Feeds `frame` twice, as two frames in a row would.
    fn twice(tr: &mut LootTracker, frame: LootFrame) -> Loot {
        let mut loot = tr.on_frame(frame.clone()).loot;
        loot += tr.on_frame(frame).loot;
        loot
    }

    #[test]
    fn counts_pickups_and_potions_used_but_not_trades() {
        let mut tr = LootTracker::default();
        let frame = |meso, etc, potions, shopping| LootFrame { meso: Some(meso), etc: view(0, etc), potions: stacks(potions), shopping };
        assert!(twice(&mut tr, frame(1_000, &[(1, 5, 10)], &[(9, 100, 50)], false)).is_zero(), "the first reading is the baseline");

        // A one-frame misread doesn't count.
        assert!(tr.on_frame(frame(9_999, &[(1, 5, 10)], &[(9, 100, 50)], false)).loot.is_zero());
        // 300 mesos, 2 Etc and a new Etc item picked up; 3 potions used.
        let next = frame(1_300, &[(1, 7, 10), (2, 1, 40)], &[(9, 97, 50)], false);
        tr.on_frame(next.clone());
        let counted = tr.on_frame(next);
        assert_eq!(counted.loot, Loot { meso: 300, etc_value: 60, potion_cost: 150 });
        assert_eq!(counted.pickups, [Change { item: 1, count: 2, value: 20 }, Change { item: 2, count: 1, value: 40 }]);

        // In a shop: Etc sold, potions bought, mesos changing hands: nothing counts.
        assert!(twice(&mut tr, frame(5_000, &[], &[(9, 300, 50)], true)).is_zero());
        // Back out: the shop's values are the new baseline.
        assert!(twice(&mut tr, frame(5_000, &[], &[(9, 300, 50)], false)).is_zero());
    }

    #[test]
    fn the_inventory_is_seen_once_its_reading_is_confirmed() {
        let mut tr = LootTracker::default();
        let open = |meso| LootFrame { meso: Some(meso), etc: view(0, &[(1, 5, 10)]), ..Default::default() };
        assert!(!tr.on_frame(open(1_000)).seen, "a first read isn't confirmed yet");
        assert!(tr.on_frame(open(1_000)).seen);
        assert!(!tr.on_frame(LootFrame::default()).seen, "closed");
        // Reopened with more mesos: seen only once the catch-up is counted.
        assert!(!tr.on_frame(open(1_500)).seen);
        let caught_up = tr.on_frame(open(1_500));
        assert_eq!((caught_up.seen, caught_up.loot.meso), (true, 500));
    }

    #[test]
    fn another_tab_keeps_the_etc_counts() {
        let mut tr = LootTracker::default();
        let etc = |n| LootFrame { etc: view(0, &[(1, n, 10)]), ..Default::default() };
        twice(&mut tr, etc(5));
        // The Use tab is open for a while: Etc can't be read.
        twice(&mut tr, LootFrame::default());
        // Back on Etc: what was picked up meanwhile counts.
        assert_eq!(twice(&mut tr, etc(8)).etc_value, 30);
    }

    #[test]
    fn scrolling_the_etc_tab_is_not_a_pickup() {
        let mut tr = LootTracker::default();
        let etc = |scroll, items: &[(ItemId, u32, u64)]| LootFrame { etc: view(scroll, items), ..Default::default() };
        twice(&mut tr, etc(0, &[(1, 5, 10)]));
        // Scrolled down: other stacks show, none of them picked up.
        assert!(twice(&mut tr, etc(4, &[(2, 200, 32), (3, 40, 40)])).is_zero());
        assert_eq!(twice(&mut tr, etc(4, &[(2, 201, 32), (3, 40, 40)])).etc_value, 32);
    }

    #[test]
    fn a_potion_gone_from_the_quickslot_is_not_used_up() {
        let mut tr = LootTracker::default();
        let quick = |potions: &[(ItemId, u32, u64)]| LootFrame { potions: stacks(potions), ..Default::default() };
        twice(&mut tr, quick(&[(1, 868, 150), (2, 110, 80)]));
        // The quickslot shows the other layout: no stacks read.
        assert!(twice(&mut tr, quick(&[])).is_zero());
        assert_eq!(twice(&mut tr, quick(&[(1, 866, 150)])).potion_cost, 0, "back from nothing: no baseline to compare");
        assert_eq!(twice(&mut tr, quick(&[(1, 865, 150), (2, 109, 80)])).potion_cost, 150);
    }
}
