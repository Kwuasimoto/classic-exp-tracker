//! Pure EXP/session state machine and map debouncing: samples in, effects out. No I/O.

mod confirm;
mod level;

pub use confirm::{ConfirmOutcome, MapConfirmer, CONFIRM_FRAMES};
pub use level::TotalBounds;

use std::collections::BTreeMap;

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

    pub fn total_estimate(&self) -> Option<u64> {
        self.bounds.map(TotalBounds::estimate)
    }

    pub fn on_sample(&mut self, s: Sample) -> Vec<Effect> {
        let mut fx = Vec::new();
        self.roll_minute(s.t, &mut fx);
        let gain = s.exp.map_or(0, |r| self.accept(r));

        if let Some(sess) = self.session.as_mut() {
            if s.map == Some(sess.map) {
                sess.last_on_map = s.t;
            }
        }
        if let Some(sess) = self.session {
            if s.t - sess.last_on_map > RETURN_WINDOW_SECS {
                fx.push(Effect::SessionEnded { key: sess.key, t: sess.last_on_map });
                self.session = None;
            }
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
            return 0;
        };
        if base.pct_bp >= LEVEL_UP_FROM_BP && r.pct_bp < LEVEL_UP_TO_BP && r.exp < base.exp {
            let total = self
                .bounds
                .or_else(|| TotalBounds::from_reading(base))
                .map_or(base.exp, TotalBounds::estimate);
            self.reset_to(r);
            return total.saturating_sub(base.exp) + r.exp;
        }
        match (self.bounds, TotalBounds::from_reading(r)) {
            (Some(known), Some(implied)) => match known.intersect(implied) {
                Some(merged) => self.bounds = Some(merged),
                None => {
                    self.rejections += 1;
                    if self.rejections >= MAX_CONSECUTIVE_REJECTIONS {
                        self.reset_to(r);
                    }
                    return 0;
                }
            },
            (None, implied) => self.bounds = implied,
            (Some(_), None) => {}
        }
        self.rejections = 0;
        self.baseline = Some(r);
        r.exp.saturating_sub(base.exp)
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

    fn feed(tr: &mut Tracker, t: i64, exp: Option<ExpReading>, map: Option<MapId>) -> Vec<Effect> {
        tr.on_sample(Sample { t, exp, map })
    }

    fn minutes(fx: &[Effect]) -> (u64, u32) {
        fx.iter().fold((0, 0), |(e, s), f| match f {
            Effect::Minute { exp_gained, training_secs, .. } => (e + exp_gained, s + training_secs),
            _ => (e, s),
        })
    }

    /// 10_000 + 10t EXP at exactly (1000 + t) bp: consistent with a 100_000 total.
    fn steady(t: i64) -> Option<ExpReading> {
        let t = u64::try_from(t).expect("non-negative t");
        reading(10_000 + 10 * t, 1_000 + u32::try_from(t).expect("small t"))
    }

    #[test]
    fn gain_starts_session_and_rolls_up_per_minute() {
        let mut tr = Tracker::new();
        assert!(feed(&mut tr, 0, reading(100, 100), MAP).is_empty());
        assert_eq!(feed(&mut tr, 1, reading(150, 150), MAP), vec![Effect::SessionStarted { key: SessionKey(0), map: 1, t: 1 }]);
        assert_eq!(
            feed(&mut tr, 60, None, MAP),
            vec![Effect::Minute { minute_start: 0, key: SessionKey(0), exp_gained: 50, training_secs: 1 }]
        );
    }

    #[test]
    fn level_up_counts_the_rest_of_the_old_level() {
        let mut tr = Tracker::new();
        feed(&mut tr, 0, reading(990_000, 9_900), MAP);
        feed(&mut tr, 1, reading(5_000, 50), MAP);
        let fx = tr.finish();
        assert_eq!(minutes(&fx).0, 15_000);
        assert_eq!(tr.baseline(), Some(ExpReading { exp: 5_000, pct_bp: 50 }));
    }

    #[test]
    fn death_resets_baseline_without_negative_gain() {
        let mut tr = Tracker::new();
        feed(&mut tr, 0, reading(500_000, 5_000), MAP);
        feed(&mut tr, 1, reading(450_000, 4_500), MAP);
        feed(&mut tr, 2, reading(460_000, 4_600), MAP);
        assert_eq!(minutes(&tr.finish()).0, 10_000);
    }

    #[test]
    fn inconsistent_reading_is_rejected() {
        let mut tr = Tracker::new();
        feed(&mut tr, 0, reading(500_000, 5_000), MAP);
        feed(&mut tr, 1, reading(900_000, 5_100), MAP);
        assert_eq!(tr.baseline(), Some(ExpReading { exp: 500_000, pct_bp: 5_000 }));
        feed(&mut tr, 2, reading(510_000, 5_100), MAP);
        assert_eq!(minutes(&tr.finish()).0, 10_000);
    }

    #[test]
    fn repeated_rejections_reset_the_baseline() {
        let mut tr = Tracker::new();
        feed(&mut tr, 0, reading(500_000, 5_000), MAP);
        for t in 1..=i64::from(MAX_CONSECUTIVE_REJECTIONS) {
            feed(&mut tr, t, reading(900_000, 5_100), MAP);
        }
        assert_eq!(tr.baseline(), Some(ExpReading { exp: 900_000, pct_bp: 5_100 }));
        let fx = feed(&mut tr, 6, reading(900_100, 5_100), MAP);
        assert_eq!(fx, vec![Effect::SessionStarted { key: SessionKey(0), map: 1, t: 6 }]);
        assert_eq!(minutes(&tr.finish()).0, 100);
    }

    #[test]
    fn gains_without_a_confirmed_map_are_not_recorded() {
        let mut tr = Tracker::new();
        feed(&mut tr, 0, steady(0), None);
        assert!(feed(&mut tr, 1, steady(1), None).is_empty());
        assert!(tr.finish().is_empty());
    }

    #[test]
    fn repot_trip_within_window_keeps_session_and_excludes_town_time() {
        let mut tr = Tracker::new();
        let mut fx = Vec::new();
        for t in 0..=10 {
            fx.extend(feed(&mut tr, t, steady(t), MAP));
        }
        for t in 11..=200 {
            fx.extend(feed(&mut tr, t, None, TOWN));
        }
        fx.extend(feed(&mut tr, 201, steady(20), MAP));
        fx.extend(tr.finish());
        let started = fx.iter().filter(|f| matches!(f, Effect::SessionStarted { .. })).count();
        let ended: Vec<_> = fx.iter().filter(|f| matches!(f, Effect::SessionEnded { .. })).collect();
        assert_eq!(started, 1);
        assert_eq!(ended, vec![&Effect::SessionEnded { key: SessionKey(0), t: 201 }]);
        assert_eq!(minutes(&fx), (200, 11));
    }

    #[test]
    fn away_longer_than_window_ends_session() {
        let mut tr = Tracker::new();
        let mut fx = Vec::new();
        for t in 0..=10 {
            fx.extend(feed(&mut tr, t, steady(t), MAP));
        }
        for t in 11..=700 {
            fx.extend(feed(&mut tr, t, None, TOWN));
        }
        assert!(fx.contains(&Effect::SessionEnded { key: SessionKey(0), t: 10 }));
        let fx = feed(&mut tr, 701, steady(20), MAP);
        assert!(fx.contains(&Effect::SessionStarted { key: SessionKey(1), map: 1, t: 701 }));
    }

    #[test]
    fn gain_on_another_map_ends_the_session() {
        let mut tr = Tracker::new();
        feed(&mut tr, 0, steady(0), MAP);
        feed(&mut tr, 1, steady(1), MAP);
        let fx = feed(&mut tr, 2, steady(2), Some(3));
        assert_eq!(
            fx,
            vec![
                Effect::SessionEnded { key: SessionKey(0), t: 1 },
                Effect::SessionStarted { key: SessionKey(1), map: 3, t: 2 },
            ]
        );
    }

    #[test]
    fn afk_stops_counting_after_grace_period() {
        let mut tr = Tracker::new();
        let mut fx = Vec::new();
        for t in 0..=10 {
            fx.extend(feed(&mut tr, t, steady(t), MAP));
        }
        for t in 11..=300 {
            fx.extend(feed(&mut tr, t, None, MAP));
        }
        fx.extend(tr.finish());
        assert_eq!(minutes(&fx), (100, 130));
    }

    #[test]
    fn capture_gap_is_capped() {
        let mut tr = Tracker::new();
        feed(&mut tr, 0, steady(0), MAP);
        feed(&mut tr, 1, steady(1), MAP);
        feed(&mut tr, 50, steady(2), MAP);
        assert_eq!(minutes(&tr.finish()).1, 1 + MAX_SAMPLE_GAP_SECS as u32);
    }
}
