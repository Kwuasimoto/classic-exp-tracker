//! 10-minute bars and the 60-minute trailing EXP/hr and meso rates. Pure functions over minute rows.

use serde::Serialize;

pub const BUCKET_SECS: i64 = 600;
pub const SMA_BUCKETS: usize = 6;

/// Mesos made from the inventory and quickslot (tracker::loot): mesos picked up, the NPC
/// value of Etc items picked up, and the shop price of potions used.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Loot {
    pub meso: u64,
    pub etc_value: u64,
    pub potion_cost: u64,
}

impl Loot {
    pub fn is_zero(&self) -> bool {
        *self == Self::default()
    }
}

impl std::ops::AddAssign for Loot {
    fn add_assign(&mut self, other: Self) {
        self.meso += other.meso;
        self.etc_value += other.etc_value;
        self.potion_cost += other.potion_cost;
    }
}

/// EXP and mesos made on every map since the user last reset the totals, and the first
/// minute counted since then (`None` until one is).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub exp: u64,
    pub loot: Loot,
    pub since: Option<i64>,
}

impl Totals {
    pub fn add(&mut self, minute_start: i64, exp_gained: u64, loot: Loot) {
        self.exp += exp_gained;
        self.loot += loot;
        self.since.get_or_insert(minute_start);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MinuteRow {
    pub minute_start: i64,
    pub exp_gained: u64,
    pub training_secs: u32,
    pub loot: Loot,
}

/// Per hour of training, over the same 60 minutes as the EXP/hr line.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LootRates {
    pub meso: f64,
    pub etc_value: f64,
    pub potion_cost: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bar {
    pub start: i64,
    pub exp_gained: u64,
    /// Σexp / Σtraining_secs × 3600 over this and the previous 5 buckets.
    pub rate_per_hour: Option<f64>,
    pub loot: Loot,
    /// The loot rates over the same buckets as `rate_per_hour`.
    pub loot_per_hour: Option<LootRates>,
}

pub fn bucket_start(t: i64) -> i64 {
    t.div_euclid(BUCKET_SECS) * BUCKET_SECS
}

/// First and last bar starts for a live window of `window_minutes` ending at `now`.
pub fn live_range(now: i64, window_minutes: u32) -> (i64, i64) {
    let last = bucket_start(now);
    let bars = (i64::from(window_minutes) * 60 / BUCKET_SECS).max(1);
    (last - (bars - 1) * BUCKET_SECS, last)
}

/// The live chart's windows, in minutes (2H and 3H); the last is the longest.
pub const WINDOWS_MINUTES: [u32; 2] = [120, 180];
const MAX_WINDOW_MINUTES: u32 = WINDOWS_MINUTES[WINDOWS_MINUTES.len() - 1];

/// Earliest minute needed to compute the SMA of a bar starting at `first_bar`.
pub fn rows_from(first_bar: i64) -> i64 {
    first_bar - (SMA_BUCKETS as i64 - 1) * BUCKET_SECS
}

/// Earliest minute the live chart can still need at `now`; older minutes can be deleted.
pub fn keep_from(now: i64) -> i64 {
    rows_from(live_range(now, MAX_WINDOW_MINUTES).0)
}

/// One bar per bucket from `first_bar` to `last_bar` inclusive (both bucket-aligned).
pub fn bars_range(rows: &[MinuteRow], first_bar: i64, last_bar: i64) -> Vec<Bar> {
    if last_bar < first_bar {
        return Vec::new();
    }
    let origin = rows_from(first_bar);
    // Non-negative: last_bar ≥ first_bar > origin.
    let count = ((last_bar - origin) / BUCKET_SECS + 1) as usize;
    let mut buckets = vec![(0u64, 0u64, Loot::default()); count];
    for r in rows {
        if r.minute_start < origin {
            continue;
        }
        let i = ((r.minute_start - origin) / BUCKET_SECS) as usize;
        if let Some(b) = buckets.get_mut(i) {
            b.0 += r.exp_gained;
            b.1 += u64::from(r.training_secs);
            b.2 += r.loot;
        }
    }
    (SMA_BUCKETS - 1..count)
        .map(|i| {
            let window = &buckets[i + 1 - SMA_BUCKETS..=i];
            let exp: u64 = window.iter().map(|b| b.0).sum();
            let secs: u64 = window.iter().map(|b| b.1).sum();
            let mut loot = Loot::default();
            window.iter().for_each(|b| loot += b.2);
            let per_hour = |v: u64| v as f64 * 3600.0 / secs as f64;
            Bar {
                start: origin + i as i64 * BUCKET_SECS,
                exp_gained: buckets[i].0,
                rate_per_hour: (secs > 0).then(|| per_hour(exp)),
                loot: buckets[i].2,
                loot_per_hour: (secs > 0).then(|| LootRates {
                    meso: per_hour(loot.meso),
                    etc_value: per_hour(loot.etc_value),
                    potion_cost: per_hour(loot.potion_cost),
                }),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_ignores_buckets_without_training() {
        // 30 min of training at 100 EXP/min, then 30 min away (no rows).
        let rows: Vec<_> = (0..30).map(|m| MinuteRow { minute_start: m * 60, exp_gained: 100, training_secs: 60, ..Default::default() }).collect();
        let bars = bars_range(&rows, 3_000, 3_000);
        assert_eq!(bars[0].rate_per_hour, Some(6_000.0));
    }
}
