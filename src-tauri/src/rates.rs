//! 10-minute bars and the 60-minute trailing EXP/hr line. Pure functions over minute rows.

use serde::Serialize;

pub const BUCKET_SECS: i64 = 600;
pub const SMA_BUCKETS: usize = 6;
/// A "best hour" must contain at least this much training time.
pub const MIN_BEST_WINDOW_SECS: u64 = 1_800;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MinuteRow {
    pub minute_start: i64,
    pub exp_gained: u64,
    pub training_secs: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bar {
    pub start: i64,
    pub exp_gained: u64,
    /// Σexp / Σtraining_secs × 3600 over this and the previous 5 buckets.
    pub rate_per_hour: Option<f64>,
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

/// Earliest minute needed to compute the SMA of a bar starting at `first_bar`.
pub fn rows_from(first_bar: i64) -> i64 {
    first_bar - (SMA_BUCKETS as i64 - 1) * BUCKET_SECS
}

/// One bar per bucket from `first_bar` to `last_bar` inclusive (both bucket-aligned).
pub fn bars_range(rows: &[MinuteRow], first_bar: i64, last_bar: i64) -> Vec<Bar> {
    if last_bar < first_bar {
        return Vec::new();
    }
    let origin = rows_from(first_bar);
    // Non-negative: last_bar ≥ first_bar > origin.
    let count = ((last_bar - origin) / BUCKET_SECS + 1) as usize;
    let mut buckets = vec![(0u64, 0u64); count];
    for r in rows {
        if r.minute_start < origin {
            continue;
        }
        let i = ((r.minute_start - origin) / BUCKET_SECS) as usize;
        if let Some(b) = buckets.get_mut(i) {
            b.0 += r.exp_gained;
            b.1 += u64::from(r.training_secs);
        }
    }
    (SMA_BUCKETS - 1..count)
        .map(|i| {
            let window = &buckets[i + 1 - SMA_BUCKETS..=i];
            let exp: u64 = window.iter().map(|b| b.0).sum();
            let secs: u64 = window.iter().map(|b| b.1).sum();
            Bar {
                start: origin + i as i64 * BUCKET_SECS,
                exp_gained: buckets[i].0,
                rate_per_hour: (secs > 0).then(|| exp as f64 * 3600.0 / secs as f64),
            }
        })
        .collect()
}

/// Highest EXP/hr over any 60-minute window with ≥ MIN_BEST_WINDOW_SECS of training.
/// `rows` must be sorted by `minute_start`.
pub fn best_hour_rate(rows: &[MinuteRow]) -> Option<f64> {
    let mut best: Option<f64> = None;
    let (mut lo, mut exp, mut secs) = (0usize, 0u64, 0u64);
    for hi in 0..rows.len() {
        exp += rows[hi].exp_gained;
        secs += u64::from(rows[hi].training_secs);
        while rows[hi].minute_start - rows[lo].minute_start >= 3_600 {
            exp -= rows[lo].exp_gained;
            secs -= u64::from(rows[lo].training_secs);
            lo += 1;
        }
        if secs >= MIN_BEST_WINDOW_SECS {
            let rate = exp as f64 * 3600.0 / secs as f64;
            best = Some(best.map_or(rate, |b| b.max(rate)));
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_ignores_buckets_without_training() {
        // 30 min of training at 100 EXP/min, then 30 min away (no rows).
        let rows: Vec<_> = (0..30).map(|m| MinuteRow { minute_start: m * 60, exp_gained: 100, training_secs: 60 }).collect();
        let bars = bars_range(&rows, 3_000, 3_000);
        assert_eq!(bars[0].rate_per_hour, Some(6_000.0));
    }
}
