//! Meso tracking, frame by frame: reads the inventory and quickslot crops, tells which
//! item each stack is (learning new ones), and counts what was made (tracker::loot).
//! The pipeline calls it with the store; nothing here touches the screen.

use std::collections::{BTreeMap, BTreeSet};

use log::info;

use crate::capture::LootCrops;
use crate::items::{self, ItemKind, PotionSlot};
use crate::pipeline::{LootStatus, MainPotion, PickedUp, QuickslotLayout, QuickslotPot};
use crate::rates::Loot;
use crate::read::inventory::{keyboard_layout, patch_distance, read_inventory, read_quickslot, Patch, SlotRead, Tab};
use crate::store::{ItemPatch, Store, StoreError};
use crate::tracker::loot::{Change, EtcView, ItemId, LootFrame, LootTracker, Stack, Stacks};

/// Patches this close (mean difference per channel) are the same icon. At native
/// resolution an icon is drawn identically every time, so this only absorbs a stray pixel.
const SAME_ICON: f32 = 3.0;

#[derive(Default)]
pub struct LootReader {
    /// Every item's patches, as stored.
    items: Vec<ItemPatch>,
    loaded: bool,
    /// Patches of no known item seen last frame: one seen again is a new item.
    unseen: Vec<(ItemKind, Patch)>,
    tracker: LootTracker,
    /// The main HP and MP items: only their use costs mesos.
    main_hp: Option<ItemId>,
    main_mp: Option<ItemId>,
    /// Each Etc item picked up since the app started or the session was reset: how many,
    /// and what they were worth when counted.
    picked_up: BTreeMap<ItemId, (u32, u64)>,
    /// The Use items on the quickslot when it last read (LootStatus::quickslot_pots).
    quickslot_pots: Vec<QuickslotPot>,
    /// When the inventory was last seen with everything counted (LootStatus::seen_at).
    seen_at: Option<i64>,
}

impl LootReader {
    /// The user named an item: read the items again before the next frame.
    pub fn reload(&mut self) {
        self.loaded = false;
    }

    /// Meso tracking was off for a while: what's on screen now is a new baseline.
    pub fn restart(&mut self) {
        self.tracker = LootTracker::default();
        self.unseen.clear();
        self.quickslot_pots.clear();
        self.seen_at = None;
    }

    /// One frame: what it made, whether the inventory was seen with everything counted
    /// (tracker::loot::Counted::seen), and what the UI shows about it. `None` crops when
    /// tracking is off.
    pub fn on_frame(
        &mut self,
        store: &Store,
        t: i64,
        crops: Option<&LootCrops>,
        shopping: bool,
    ) -> Result<(Loot, bool, LootStatus), StoreError> {
        let Some(crops) = crops else {
            self.restart();
            return Ok((Loot::default(), false, LootStatus::default()));
        };
        let mut status = LootStatus { enabled: true, ..LootStatus::default() };
        if !self.loaded {
            self.items = store.item_patches()?;
            self.main_hp = store.main_potion(PotionSlot::Hp)?;
            self.main_mp = store.main_potion(PotionSlot::Mp)?;
            self.loaded = true;
        }
        let inventory = read_inventory(&crops.inventory, &crops.inventory_at);
        let quickslot = read_quickslot(&crops.quickslot, &crops.quickslot_at);
        status.found = inventory.found;
        status.inventory = inventory.found && inventory.meso.is_some();
        status.meso_total = inventory.meso.filter(|_| inventory.found);
        status.etc_tab = inventory.tab == Some(Tab::Etc);
        status.quickslot = quickslot.is_some();
        status.quickslot_layout = quickslot.is_some().then(|| {
            if keyboard_layout(&crops.quickslot, &crops.quickslot_at) { QuickslotLayout::Keyboard } else { QuickslotLayout::Controller }
        });

        let mut unseen = Vec::new();
        let etc = match (&inventory.slots, status.etc_tab && status.inventory) {
            (Some(slots), true) => Some(EtcView { scroll: inventory.scroll, stacks: self.stacks(store, t, ItemKind::Etc, slots, &mut unseen)? }),
            _ => None,
        };
        // Every quickslot stack is recognised (and new ones learned), but only the main HP
        // and MP items count: anything else used is cushioning.
        let main = [self.main_hp, self.main_mp];
        let potions = match &quickslot {
            Some(slots) => {
                let stacks = self.stacks(store, t, ItemKind::Use, slots, &mut unseen)?;
                self.quickslot_pots = self.quickslot_pots(slots, &stacks);
                Some(stacks.into_iter().filter(|(id, _)| main.contains(&Some(*id))).collect())
            }
            None => None,
        };
        status.quickslot_pots = self.quickslot_pots.clone();
        status.main_hp = self.main_hp.map(|id| self.main_potion(id));
        status.main_mp = self.main_mp.map(|id| self.main_potion(id));
        self.unseen = unseen;
        status.unnamed_items = self.items.iter().filter(|i| i.name.is_none() && !i.ignored).map(|i| i.item_id).collect::<BTreeSet<_>>().len() as u32;

        let counted = self.tracker.on_frame(LootFrame { meso: inventory.meso, etc, potions, shopping });
        for Change { item, count, value } in counted.pickups {
            let total = self.picked_up.entry(item).or_default();
            total.0 += count;
            total.1 += value;
        }
        status.picked_up = self.picked_up();
        if counted.seen {
            self.seen_at = Some(t);
        }
        status.seen_at = self.seen_at;
        Ok((counted.loot, counted.seen, status))
    }

    /// The quickslot's Use items in slot order, each once with its stacks added up. Items not
    /// learned yet and ignored ones aren't among `stacks`, so they're left out.
    fn quickslot_pots(&self, slots: &[SlotRead], stacks: &Stacks) -> Vec<QuickslotPot> {
        let mut pots: Vec<QuickslotPot> = Vec::new();
        for slot in slots {
            let Some(id) = self.find(ItemKind::Use, &slot.patch) else { continue };
            let Some(stack) = stacks.get(&id) else { continue };
            if pots.iter().any(|p| p.item_id == id) {
                continue;
            }
            let name = self.items.iter().find(|i| i.item_id == id).and_then(|i| i.name.clone());
            // The patch as on screen now: an item seen on both layouts has a patch for each.
            pots.push(QuickslotPot { item_id: id, name, count: stack.count, patch: slot.patch.0.clone() });
        }
        pots
    }

    fn main_potion(&self, item_id: ItemId) -> MainPotion {
        MainPotion { item_id, name: self.items.iter().find(|i| i.item_id == item_id).and_then(|i| i.name.clone()) }
    }

    /// The session's pickups start over (the user reset the session).
    pub fn clear_pickups(&mut self) {
        self.picked_up.clear();
    }

    /// One line per item picked up, highest value first, with its icon patch and current name.
    fn picked_up(&self) -> Vec<PickedUp> {
        let mut lines: Vec<PickedUp> = self
            .picked_up
            .iter()
            .map(|(&item_id, &(count, value))| {
                let item = self.items.iter().find(|i| i.item_id == item_id);
                PickedUp {
                    item_id,
                    name: item.and_then(|i| i.name.clone()),
                    count,
                    value,
                    patch: item.map(|i| i.patch.clone()).unwrap_or_default(),
                }
            })
            .collect();
        lines.sort_by(|a, b| b.value.cmp(&a.value).then(b.count.cmp(&a.count)));
        lines
    }

    /// Each slot's item and count, summed per item. A patch no item has is learned as a
    /// new (unnamed) item once it's seen two frames in a row; until then its stack is left
    /// out, and an unnamed item's stack is worth nothing.
    fn stacks(&mut self, store: &Store, t: i64, kind: ItemKind, slots: &[SlotRead], unseen: &mut Vec<(ItemKind, Patch)>) -> Result<Stacks, StoreError> {
        let mut stacks = Stacks::new();
        for slot in slots {
            let id = match self.find(kind, &slot.patch) {
                Some(id) => id,
                None if self.unseen.iter().any(|(k, p)| *k == kind && patch_distance(&p.0, &slot.patch.0) <= SAME_ICON) => {
                    let id = store.insert_item(kind, &slot.patch.0, t)?;
                    info!("new {} item #{id} seen (stack of {}); unnamed until the user names it", kind.as_str(), slot.count);
                    self.items.push(ItemPatch { item_id: id, kind, name: None, ignored: false, patch: slot.patch.0.clone() });
                    id
                }
                None => {
                    unseen.push((kind, slot.patch.clone()));
                    continue;
                }
            };
            if self.items.iter().any(|i| i.item_id == id && i.ignored) {
                continue;
            }
            let price = self.price(id);
            let stack = stacks.entry(id).or_insert(Stack { count: 0, price });
            stack.count += slot.count;
        }
        Ok(stacks)
    }

    fn find(&self, kind: ItemKind, patch: &Patch) -> Option<ItemId> {
        self.items
            .iter()
            .filter(|i| i.kind == kind)
            .map(|i| (i.item_id, patch_distance(&i.patch, &patch.0)))
            .filter(|&(_, d)| d <= SAME_ICON)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    /// The item list's price for a named item; 0 while unnamed.
    fn price(&self, id: ItemId) -> u64 {
        self.items
            .iter()
            .find(|i| i.item_id == id)
            .and_then(|i| i.name.as_deref().and_then(|n| items::find(i.kind, n)))
            .map_or(0, |listed| listed.price)
    }
}
