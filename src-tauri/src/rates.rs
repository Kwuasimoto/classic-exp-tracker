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
    /// The inventory was seen this minute with everything made counted. While it's closed the
    /// loot made is counted when it's next seen, so the loot rates run only to the last minute
    /// it was seen (`bars_range`).
    pub loot_seen: bool,
}

/// Per hour of training, over the same 60 minutes as the EXP/hr line, up to the last minute
/// the inventory was seen in them: held while it's closed, caught up when it's next seen.
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
    /// The loot rates over the same buckets as `rate_per_hour`, up to the last minute the
    /// inventory was seen in them; `None` when it wasn't seen in them at all.
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
    let mut buckets = vec![Bucket::default(); count];
    let in_range = |r: &&MinuteRow| r.minute_start >= origin && r.minute_start < origin + count as i64 * BUCKET_SECS;
    let index = |r: &MinuteRow| ((r.minute_start - origin) / BUCKET_SECS) as usize;
    for r in rows.iter().filter(in_range) {
        let b = &mut buckets[index(r)];
        b.exp += r.exp_gained;
        b.secs += u64::from(r.training_secs);
        b.loot += r.loot;
        if r.loot_seen {
            b.last_seen = b.last_seen.max(Some(r.minute_start));
        }
    }
    // The bucket's training and loot up to (and with) its last minute the inventory was seen.
    for r in rows.iter().filter(in_range) {
        let b = &mut buckets[index(r)];
        if b.last_seen.is_some_and(|seen| r.minute_start <= seen) {
            b.seen_secs += u64::from(r.training_secs);
            b.seen_loot += r.loot;
        }
    }
    (SMA_BUCKETS - 1..count)
        .map(|i| {
            let window = &buckets[i + 1 - SMA_BUCKETS..=i];
            let exp: u64 = window.iter().map(|b| b.exp).sum();
            let secs: u64 = window.iter().map(|b| b.secs).sum();
            let per_hour = |v: u64, secs: u64| v as f64 * 3600.0 / secs as f64;
            // Loot runs to the window's last minute the inventory was seen: all of the buckets
            // before that one, and that bucket up to it.
            let loot_per_hour = window.iter().rposition(|b| b.last_seen.is_some()).and_then(|last| {
                let mut loot = window[last].seen_loot;
                window[..last].iter().for_each(|b| loot += b.loot);
                let secs = window[..last].iter().map(|b| b.secs).sum::<u64>() + window[last].seen_secs;
                (secs > 0).then(|| LootRates {
                    meso: per_hour(loot.meso, secs),
                    etc_value: per_hour(loot.etc_value, secs),
                    potion_cost: per_hour(loot.potion_cost, secs),
                })
            });
            Bar {
                start: origin + i as i64 * BUCKET_SECS,
                exp_gained: buckets[i].exp,
                rate_per_hour: (secs > 0).then(|| per_hour(exp, secs)),
                loot: buckets[i].loot,
                loot_per_hour,
            }
        })
        .collect()
}

/// One bucket's minutes added up.
#[derive(Clone, Copy, Default)]
struct Bucket {
    exp: u64,
    secs: u64,
    loot: Loot,
    /// Its last minute the inventory was seen, and its training and loot up to then.
    last_seen: Option<i64>,
    seen_secs: u64,
    seen_loot: Loot,
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

    #[test]
    fn loot_rates_hold_while_the_inventory_is_closed_and_catch_up_when_it_is_seen() {
        let minute = |m: i64, meso: u64, loot_seen: bool| MinuteRow {
            minute_start: m * 60,
            exp_gained: 100,
            training_secs: 60,
            loot: Loot { meso, ..Loot::default() },
            loot_seen,
        };
        // 20 min at 100 mesos a minute with the inventory open, then 20 min with it closed.
        let mut rows: Vec<_> = (0..20).map(|m| minute(m, 100, true)).collect();
        rows.extend((20..40).map(|m| minute(m, 0, false)));
        let meso = |rows: &[MinuteRow]| bars_range(rows, 2_400, 2_400)[0].loot_per_hour.map(|r| r.meso);
        assert_eq!(meso(&rows), Some(6_000.0), "held at the rate when it was last seen");
        // Opened again: the 2,000 mesos made meanwhile count, and so does the time.
        rows.push(minute(40, 2_100, true));
        assert_eq!(meso(&rows), Some(6_000.0));
    }
}
