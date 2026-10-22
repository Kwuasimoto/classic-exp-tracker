//! Pure EXP/session state machine and map debouncing: samples in, effects out.
//! No I/O apart from log lines explaining its decisions.

mod confirm;
mod level;

pub use confirm::{ConfirmOutcome, MapConfirmer};
use level::{exp_to_next, TotalBounds};

use std::collections::BTreeMap;

use log::{info, warn};

use crate::rates::MinuteRow;
use crate::read::exp::ExpReading;

pub type MapId = i64;

/// Tracker-local session identity; the pipeline maps it to a database id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionKey(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub t: i64,
    pub exp: Option<ExpReading>,
    /// The currently confirmed map: `None` until one is confirmed. EXP counts either
    /// way, in a session without a map until then.
    pub map: Option<MapId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// A session's effects come in order: `SessionStarted`, its `Minute`s, `SessionEnded`.
/// Nothing refers to a session after it ends.
pub enum Effect {
    SessionStarted { key: SessionKey, map: Option<MapId>, t: i64 },
    SessionEnded { key: SessionKey, t: i64 },
    Minute { minute_start: i64, key: SessionKey, exp_gained: u64, training_secs: u32 },
}

const TRAINING_GRACE_SECS: i64 = 120;
const MAX_SAMPLE_GAP_SECS: i64 = 3;
const RETURN_WINDOW_SECS: i64 = 600;
const LEVEL_UP_FROM_BP: u32 = 8_000;
const LEVEL_UP_TO_BP: u32 = 2_000;
const MAX_CONSECUTIVE_REJECTIONS: u32 = 5;

#[derive(Clone, Copy, Debug)]
struct ActiveSession {
    key: SessionKey,
    map: Option<MapId>,
    last_on_map: i64,
}

#[derive(Debug, Default)]
pub struct Tracker {
    baseline: Option<ExpReading>,
    bounds: Option<TotalBounds>,
    /// The player's level, once the bounds fit exactly one table total (or a
    /// level read from the screen fits them).
    level: Option<u32>,
    /// Last screen level that did not fit the readings, so it is logged once.
    unfit_screen_level: Option<u32>,
    rejections: u32,
    last_gain_t: Option<i64>,
    last_t: Option<i64>,
    session: Option<ActiveSession>,
    next_key: u64,
    minute: Option<i64>,
    /// Per-session (exp_gained, training_secs) for the current minute.
    acc: BTreeMap<SessionKey, (u64, u32)>,
}

impl Tracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn baseline(&self) -> Option<ExpReading> {
        self.baseline
    }

    pub fn level(&self) -> Option<u32> {
        self.level
    }

    /// The level's total EXP: exact from the table once the level is known,
    /// otherwise the middle of the measured bounds.
    pub fn total_estimate(&self) -> Option<u64> {
        self.level.and_then(exp_to_next).or_else(|| self.bounds.map(TotalBounds::estimate))
    }

    pub fn on_sample(&mut self, s: Sample) -> Vec<Effect> {
        let mut fx = Vec::new();
        self.roll_minute(s.t, &mut fx);
        let gain = s.exp.map_or(0, |r| self.accept(r));

        if let Some(sess) = self.session.as_mut()
            && s.map == sess.map {
                sess.last_on_map = s.t;
            }
        if let Some(sess) = self.session
            && s.t - sess.last_on_map > RETURN_WINDOW_SECS {
                self.end(sess, &mut fx);
            }

        if gain > 0 {
            self.last_gain_t = Some(s.t);
            let key = self.session_for(s.map, s.t, &mut fx);
            self.acc.entry(key).or_default().0 += gain;
        }

        let dt = self.last_t.map_or(0, |prev| (s.t - prev).clamp(0, MAX_SAMPLE_GAP_SECS));
        self.last_t = Some(s.t);
        if let Some(sess) = self.session {
            let recently_gained = self.last_gain_t.is_some_and(|g| s.t - g <= TRAINING_GRACE_SECS);
            if sess.map == s.map && recently_gained && dt > 0 {
                // dt ≤ MAX_SAMPLE_GAP_SECS, so the cast cannot truncate.
                self.acc.entry(sess.key).or_default().1 += dt as u32;
            }
        }
        fx
    }

    /// Flush the current minute and end the open session (app shutdown).
    pub fn finish(&mut self) -> Vec<Effect> {
        let mut fx = Vec::new();
        if let Some(minute) = self.minute.take() {
            self.flush(minute, &mut fx);
        }
        if let Some(sess) = self.session {
            self.end(sess, &mut fx);
        }
        fx
    }

    /// End the open session now (the user reset it); the next EXP gain starts a new one.
    pub fn end_session(&mut self) -> Vec<Effect> {
        let mut fx = Vec::new();
        if let Some(sess) = self.session {
            self.end(sess, &mut fx);
        }
        fx
    }

    /// End `sess`: its part of the minute in progress first, then `SessionEnded`.
    fn end(&mut self, sess: ActiveSession, fx: &mut Vec<Effect>) {
        if let (Some(minute_start), Some((exp_gained, training_secs))) = (self.minute, self.acc.remove(&sess.key))
            && (exp_gained > 0 || training_secs > 0)
        {
            fx.push(Effect::Minute { minute_start, key: sess.key, exp_gained, training_secs });
        }
        fx.push(Effect::SessionEnded { key: sess.key, t: sess.last_on_map });
        self.session = None;
    }

    fn session_for(&mut self, map: Option<MapId>, t: i64, fx: &mut Vec<Effect>) -> SessionKey {
        if let Some(sess) = self.session {
            if sess.map == map {
                return sess.key;
            }
            self.end(sess, fx);
        }
        let key = SessionKey(self.next_key);
        self.next_key += 1;
        self.session = Some(ActiveSession { key, map, last_on_map: t });
        fx.push(Effect::SessionStarted { key, map, t });
        key
    }

    /// Returns the EXP gained since the baseline; 0 for first, rejected, or death readings.
    fn accept(&mut self, r: ExpReading) -> u64 {
        let Some(base) = self.baseline else {
            self.reset_to(r);
            self.check_level();
            return 0;
        };
        if r.exp < base.exp && (base.pct_bp >= LEVEL_UP_FROM_BP && r.pct_bp < LEVEL_UP_TO_BP || self.fits_next_level_only(r)) {
            let total = self.total_estimate().unwrap_or(base.exp);
            info!("level-up: {base:?} -> {r:?}, old level total {total} (level {:?})", self.level);
            self.level = self.level.map(|l| l + 1);
            self.reset_to(r);
            self.check_level();
            return total.saturating_sub(base.exp) + r.exp;
        }
        match (self.bounds, TotalBounds::from_reading(r)) {
            (Some(known), Some(implied)) => match known.intersect(implied) {
                Some(merged) => self.bounds = Some(merged),
                None => {
                    self.rejections += 1;
                    warn!("rejected {r:?}: implies level total {implied:?}, outside {known:?} (rejection {})", self.rejections);
                    if self.rejections >= MAX_CONSECUTIVE_REJECTIONS {
                        warn!("{MAX_CONSECUTIVE_REJECTIONS} rejections in a row; restarting from {r:?}");
                        self.level = None;
                        self.reset_to(r);
                        self.check_level();
                    }
                    return 0;
                }
            },
            (None, implied) => self.bounds = implied,
            (Some(_), None) => {}
        }
        self.check_level();
        if r.exp < base.exp {
            info!("EXP dropped {base:?} -> {r:?}: counted as a death, no gain");
        }
        self.rejections = 0;
        self.baseline = Some(r);
        r.exp.saturating_sub(base.exp)
    }

    /// With the level known, a reading that fits the next level's table total but
    /// not the current one is a level-up, even when the last good reading was
    /// below LEVEL_UP_FROM_BP (e.g. the readout was covered before levelling).
    fn fits_next_level_only(&self, r: ExpReading) -> bool {
        let (Some(level), Some(implied)) = (self.level, TotalBounds::from_reading(r)) else { return false };
        let fits = |l: u32| exp_to_next(l).is_some_and(|t| implied.contains(t));
        fits(level + 1) && !fits(level)
    }

    /// A level read from the level box (digits learned from the EXP readings). Used
    /// only if its table total fits the EXP readings, so a misread can't change the totals.
    /// With no EXP readings to check it against, it's used as is: the level shows even
    /// while the EXP box can't be read, and check_level drops it if later readings disagree.
    pub fn use_screen_level(&mut self, level: u32) {
        if self.level == Some(level) {
            return;
        }
        let total = exp_to_next(level);
        let Some(bounds) = self.bounds else {
            info!("screen shows level {level}; no EXP readings to check it against yet");
            self.level = Some(level);
            return;
        };
        if total.is_some_and(|t| bounds.contains(t)) {
            info!("screen shows level {level}; its table total {total:?} fits readings {bounds:?}");
            self.level = Some(level);
            self.unfit_screen_level = None;
        } else if self.unfit_screen_level != Some(level) {
            warn!("screen shows level {level}, but its table total {total:?} doesn't fit readings {bounds:?}; ignored");
            self.unfit_screen_level = Some(level);
        }
    }

    /// Learn the level once the bounds fit exactly one table total; forget it
    /// (and say so) if the readings stop fitting its total.
    fn check_level(&mut self) {
        let Some(bounds) = self.bounds else { return };
        if let Some(level) = self.level
            && !exp_to_next(level).is_some_and(|t| bounds.contains(t))
        {
            warn!(
                "readings imply a level total in {bounds:?}, but the table says level {level} needs {:?}; level unknown until the readings fit one level",
                exp_to_next(level)
            );
            self.level = None;
        }
        if self.level.is_none()
            && let Some(level) = bounds.matching_level()
        {
            info!("level {level} identified: table total {:?} fits readings {bounds:?}", exp_to_next(level));
            self.level = Some(level);
        }
    }

    fn reset_to(&mut self, r: ExpReading) {
        self.baseline = Some(r);
        self.bounds = TotalBounds::from_reading(r);
        self.rejections = 0;
    }

    fn roll_minute(&mut self, t: i64, fx: &mut Vec<Effect>) {
        let minute = t.div_euclid(60) * 60;
        match self.minute {
            Some(current) if current == minute => {}
            Some(current) => {
                self.flush(current, fx);
                self.minute = Some(minute);
            }
            None => self.minute = Some(minute),
        }
    }

    /// The minute in progress (not yet flushed as `Minute` effects), summed over
    /// sessions, so live rates can count it before it ends.
    pub fn current_minute(&self) -> Option<MinuteRow> {
        let minute_start = self.minute?;
        let (exp_gained, training_secs) = self.acc.values().fold((0, 0), |(e, s), &(de, ds)| (e + de, s + ds));
        (exp_gained > 0 || training_secs > 0).then_some(MinuteRow { minute_start, exp_gained, training_secs })
    }

    fn flush(&mut self, minute_start: i64, fx: &mut Vec<Effect>) {
        for (key, (exp_gained, training_secs)) in std::mem::take(&mut self.acc) {
            if exp_gained > 0 || training_secs > 0 {
                fx.push(Effect::Minute { minute_start, key, exp_gained, training_secs });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAP: Option<MapId> = Some(1);
    const TOWN: Option<MapId> = Some(2);

    fn reading(exp: u64, pct_bp: u32) -> Option<ExpReading> {
        Some(ExpReading { exp, pct_bp })
    }

    fn total_exp(fx: &[Effect]) -> u64 {
        fx.iter().map(|f| if let Effect::Minute { exp_gained, .. } = f { *exp_gained } else { 0 }).sum()
    }

    #[test]
    fn level_up_counts_the_rest_of_the_old_level() {
        let mut tr = Tracker::new();
        tr.on_sample(Sample { t: 0, exp: reading(990_000, 9_900), map: MAP });
        tr.on_sample(Sample { t: 1, exp: reading(5_000, 50), map: MAP });
        assert_eq!(total_exp(&tr.finish()), 15_000);
    }

    #[test]
    fn level_up_uses_the_table_total_once_the_level_is_known() {
        let mut tr = Tracker::new();
        // 291,000 EXP at 89.75% only fits level 41's table total, 324,240.
        tr.on_sample(Sample { t: 0, exp: reading(291_000, 8_975), map: MAP });
        assert_eq!((tr.level(), tr.total_estimate()), (Some(41), Some(324_240)));
        // 1,000 EXP is 0.28% of level 42's 356,916.
        tr.on_sample(Sample { t: 1, exp: reading(1_000, 28), map: MAP });
        assert_eq!(tr.level(), Some(42));
        assert_eq!(total_exp(&tr.finish()), 324_240 - 291_000 + 1_000);
    }

    #[test]
    fn level_up_below_80_percent_is_caught_by_the_table() {
        let mut tr = Tracker::new();
        // 200,000 EXP at 61.68% fits only level 41 (324,240).
        tr.on_sample(Sample { t: 0, exp: reading(200_000, 6_168), map: MAP });
        assert_eq!(tr.level(), Some(41));
        // 1,000 EXP at 0.28% fits level 42 (356,916), not 41: a level-up, not a death.
        tr.on_sample(Sample { t: 1, exp: reading(1_000, 28), map: MAP });
        assert_eq!(tr.level(), Some(42));
        assert_eq!(total_exp(&tr.finish()), 324_240 - 200_000 + 1_000);
    }

    #[test]
    fn current_minute_counts_gains_before_the_minute_ends() {
        let mut tr = Tracker::new();
        tr.on_sample(Sample { t: 60, exp: reading(10_000, 1_000), map: MAP });
        tr.on_sample(Sample { t: 61, exp: reading(10_050, 1_005), map: MAP });
        assert_eq!(tr.current_minute(), Some(MinuteRow { minute_start: 60, exp_gained: 50, training_secs: 1 }));
        let fx = tr.on_sample(Sample { t: 120, exp: None, map: TOWN });
        assert_eq!(total_exp(&fx), 50);
        assert_eq!(tr.current_minute(), None);
    }

    #[test]
    fn exp_counts_before_any_map_is_confirmed() {
        let mut tr = Tracker::new();
        let mut fx = Vec::new();
        for t in 0..=2 {
            fx.extend(tr.on_sample(Sample { t, exp: reading(10_000 + 10 * t as u64, 1_000 + t as u32), map: None }));
        }
        // Once the map is confirmed, its gains go to a session on that map.
        fx.extend(tr.on_sample(Sample { t: 3, exp: reading(10_030, 1_003), map: MAP }));
        fx.extend(tr.finish());
        let maps: Vec<_> = fx.iter().filter_map(|f| if let Effect::SessionStarted { map, .. } = f { Some(*map) } else { None }).collect();
        assert_eq!((maps, total_exp(&fx)), (vec![None, MAP], 30));
    }

    #[test]
    fn screen_level_shows_without_exp_readings_until_they_disagree() {
        let mut tr = Tracker::new();
        tr.use_screen_level(40);
        assert_eq!(tr.level(), Some(40));
        // 291,000 EXP at 89.75% fits only level 41's table total, so 40 was a misread.
        tr.on_sample(Sample { t: 0, exp: reading(291_000, 8_975), map: MAP });
        assert_eq!(tr.level(), Some(41));
    }

    #[test]
    fn death_is_not_negative_gain() {
        let mut tr = Tracker::new();
        tr.on_sample(Sample { t: 0, exp: reading(500_000, 5_000), map: MAP });
        tr.on_sample(Sample { t: 1, exp: reading(450_000, 4_500), map: MAP });
        tr.on_sample(Sample { t: 2, exp: reading(460_000, 4_600), map: MAP });
        assert_eq!(total_exp(&tr.finish()), 10_000);
    }

    #[test]
    fn short_repot_trip_keeps_one_session_and_excludes_town_time() {
        let mut tr = Tracker::new();
        let mut fx = Vec::new();
        // 10_000 + 10t EXP at (1000 + t) bp: consistent with a 100_000 total.
        let steady = |t: i64| reading(10_000 + 10 * t as u64, 1_000 + t as u32);
        for t in 0..=10 {
            fx.extend(tr.on_sample(Sample { t, exp: steady(t), map: MAP }));
        }
        for t in 11..=200 {
            fx.extend(tr.on_sample(Sample { t, exp: None, map: TOWN }));
        }
        fx.extend(tr.on_sample(Sample { t: 201, exp: steady(20), map: MAP }));
        fx.extend(tr.finish());
        let started = fx.iter().filter(|f| matches!(f, Effect::SessionStarted { .. })).count();
        let training: u32 = fx.iter().map(|f| if let Effect::Minute { training_secs, .. } = f { *training_secs } else { 0 }).sum();
        assert_eq!((started, total_exp(&fx), training), (1, 200, 11));
    }

    #[test]
    fn nothing_refers_to_a_session_after_it_ends() {
        let mut tr = Tracker::new();
        let mut fx = Vec::new();
        // Gain on MAP, then on TOWN within the same minute: MAP's session ends mid-minute.
        for t in 0..=5 {
            fx.extend(tr.on_sample(Sample { t, exp: reading(10_000 + 10 * t as u64, 1_000 + t as u32), map: MAP }));
        }
        for t in 6..=8 {
            fx.extend(tr.on_sample(Sample { t, exp: reading(10_000 + 10 * t as u64, 1_000 + t as u32), map: TOWN }));
        }
        fx.extend(tr.end_session());
        let mut ended = Vec::new();
        for f in &fx {
            let key = match f {
                Effect::SessionStarted { key, .. } | Effect::Minute { key, .. } => key,
                Effect::SessionEnded { key, .. } => {
                    ended.push(*key);
                    continue;
                }
            };
            assert!(!ended.contains(key), "{f:?} after its session ended");
        }
        assert_eq!((ended.len(), total_exp(&fx)), (2, 80));
    }
}
