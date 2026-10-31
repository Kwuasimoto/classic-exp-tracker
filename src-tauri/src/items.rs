//! The game's Etc and Use items with their prices (data/items.txt, from the MapleStory
//! Classic Wiki). An item seen on screen is worth something only once the user has named
//! it from this list.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

const LIST: &str = include_str!("../data/items.txt");

/// Which inventory tab an item lives in: Etc items are counted when picked up, Use items
/// (potions) when used from the quickslot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ItemKind {
    Etc,
    Use,
}

impl ItemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Etc => "etc",
            Self::Use => "use",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "etc" => Some(Self::Etc),
            "use" => Some(Self::Use),
            _ => None,
        }
    }
}

/// The two potions whose use costs mesos: the main HP and main MP items. Any other Use
/// item used (a drop the pet drinks, say) is cushioning, not counted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PotionSlot {
    Hp,
    Mp,
}

impl PotionSlot {
    pub fn setting_key(self) -> &'static str {
        match self {
            Self::Hp => "main_hp_item",
            Self::Mp => "main_mp_item",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct ListedItem {
    pub kind: ItemKind,
    pub name: &'static str,
    /// Etc: what an NPC pays for one. Use: what an NPC shop sells one for (0 if none does).
    pub price: u64,
}

/// Every listed item, in list order.
pub fn listed() -> &'static [ListedItem] {
    static ITEMS: OnceLock<Vec<ListedItem>> = OnceLock::new();
    ITEMS.get_or_init(|| {
        LIST.lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
            .map(|l| {
                let mut cols = l.split('\t');
                let item = (|| {
                    let kind = ItemKind::parse(cols.next()?)?;
                    let name = cols.next()?;
                    let price = cols.next()?.parse().ok()?;
                    Some(ListedItem { kind, name, price })
                })();
                // The list is compiled in and checked by the test below.
                item.unwrap_or_else(|| panic!("bad line in data/items.txt: {l:?}"))
            })
            .collect()
    })
}

/// The listed item of this kind with exactly this name.
pub fn find(kind: ItemKind, name: &str) -> Option<&'static ListedItem> {
    listed().iter().find(|i| i.kind == kind && i.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_parses_and_prices_potions() {
        assert!(listed().len() > 600);
        assert_eq!(find(ItemKind::Use, "Red Potion").map(|i| i.price), Some(50));
        assert_eq!(find(ItemKind::Etc, "Snail Shell").map(|i| i.price), Some(1));
    }
}
