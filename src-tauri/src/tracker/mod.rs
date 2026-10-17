//! Pure EXP/session state machine and map debouncing: samples in, effects out.
//! No I/O apart from log lines explaining its decisions.

mod confirm;
mod level;

pub use confirm::{ConfirmOutcome, MapConfirmer, CONFIRM_FRAMES};
pub use level::{exp_to_next, TotalBounds};

use std::collections::BTreeMap;

use log::{info, warn};

use crate::read::exp::ExpReading;

pub type MapId = i64;

/// Tracker-local session identity; the pipeline maps it to a database id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionKey(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub t: i64,
    pub exp: Option<ExpReading>,
    /// The currently confirmed map, if any.
    pub map: Option<MapId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    SessionStarted { key: SessionKey, map: MapId, t: i64 },
    SessionEnded { key: SessionKey, t: i64 },
    Minute { minute_start: i64, key: SessionKey, exp_gained: u64, training_secs: u32 },
}

pub const TRAINING_GRACE_SECS: i64 = 120;
pub const MAX_SAMPLE_GAP_SECS: i64 = 3;
pub const RETURN_WINDOW_SECS: i64 = 600;
pub const LEVEL_UP_FROM_BP: u32 = 8_000;
pub const LEVEL_UP_TO_BP: u32 = 2_000;
pub const MAX_CONSECUTIVE_REJECTIONS: u32 = 5;

#[derive(Clone, Copy, Debug)]
struct ActiveSession {
    key: SessionKey,
    map: MapId,
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
            && s.map == Some(sess.map) {
                sess.last_on_map = s.t;
            }
        if let Some(sess) = self.session
            && s.t - sess.last_on_map > RETURN_WINDOW_SECS {
                fx.push(Effect::SessionEnded { key: sess.key, t: sess.last_on_map });
                self.session = None;
            }

        if gain > 0 {
            self.last_gain_t = Some(s.t);
            if let Some(map) = s.map {
                let key = self.session_for(map, s.t, &mut fx);
                self.acc.entry(key).or_default().0 += gain;
            }
        }

        let dt = self.last_t.map_or(0, |prev| (s.t - prev).clamp(0, MAX_SAMPLE_GAP_SECS));
        self.last_t = Some(s.t);
        if let (Some(sess), Some(map)) = (self.session, s.map) {
            let recently_gained = self.last_gain_t.is_some_and(|g| s.t - g <= TRAINING_GRACE_SECS);
            if sess.map == map && recently_gained && dt > 0 {
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
        if let Some(sess) = self.session.take() {
            fx.push(Effect::SessionEnded { key: sess.key, t: sess.last_on_map });
        }
        fx
    }

    fn session_for(&mut self, map: MapId, t: i64, fx: &mut Vec<Effect>) -> SessionKey {
        if let Some(sess) = self.session {
            if sess.map == map {
                return sess.key;
            }
            fx.push(Effect::SessionEnded { key: sess.key, t: sess.last_on_map });
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
        if base.pct_bp >= LEVEL_UP_FROM_BP && r.pct_bp < LEVEL_UP_TO_BP && r.exp < base.exp {
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

    /// A level read from the screen (OCR). Used only if its table total fits the
    /// EXP readings, so a misread can't change the totals.
    pub fn use_screen_level(&mut self, level: u32) {
        if self.level == Some(level) {
            return;
        }
        let total = exp_to_next(level);
        match self.bounds {
            Some(bounds) if total.is_some_and(|t| bounds.contains(t)) => {
                info!("screen shows level {level}; its table total {total:?} fits readings {bounds:?}");
                self.level = Some(level);
                self.unfit_screen_level = None;
            }
            Some(bounds) if self.unfit_screen_level != Some(level) => {
                warn!("screen shows level {level}, but its table total {total:?} doesn't fit readings {bounds:?}; ignored");
                self.unfit_screen_level = Some(level);
            }
            _ => {}
        }
    }

    /// Learn the level once the bounds fit exactly one table total; forget it
    /// (and say so) if the readings stop fitting its total.
    fn check_level(&mut self) {
        let Some(bounds) = self.bounds else { return };
        match self.level {
            Some(level) if exp_to_next(level).is_some_and(|t| bounds.contains(t)) => {}
            Some(level) => {
                warn!(
                    "readings imply a level total in {bounds:?}, but the table says level {level} needs {:?}; level unknown until the readings fit one level",
                    exp_to_next(level)
                );
                self.level = None;
            }
            None => {
                if let Some(level) = bounds.matching_level() {
                    info!("level {level} identified: table total {:?} fits readings {bounds:?}", exp_to_next(level));
                    self.level = Some(level);
                }
            }
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
}
