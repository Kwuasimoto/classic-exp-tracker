//! Automatic snapshots. A grind is the sessions on one map less than JOIN_GAP_SECS apart, so a
//! trip to town (or a quest handed in there) doesn't split it. It's over once its map has had no
//! session for JOIN_GAP_SECS, or when the user resets; then it's saved as a snapshot if it had
//! MIN_TRAINING_SECS of training, and dropped otherwise (maps walked through, a stray kill).
//! Pure: sessions in, finished grinds out.

use std::collections::BTreeMap;

use crate::tracker::MapId;

pub const JOIN_GAP_SECS: i64 = 30 * 60;
pub const MIN_TRAINING_SECS: u64 = 10 * 60;
/// Automatic snapshots are deleted this long after they end, unless the user keeps them.
pub const KEEP_AUTO_SECS: i64 = 30 * 86_400;

/// A session not yet filed into a grind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionSpan {
    pub id: i64,
    pub map: Option<MapId>,
    pub started_at: i64,
    /// `None` while the session is open.
    pub ended_at: Option<i64>,
    pub training_secs: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grind {
    pub map: Option<MapId>,
    pub sessions: Vec<i64>,
    pub training_secs: u64,
}

impl Grind {
    pub fn worth_saving(&self) -> bool {
        self.training_secs >= MIN_TRAINING_SECS
    }
}

/// The grinds over at `now` among `sessions` (in any order). With `all` (the user reset) every
/// grind without an open session is over, however recent.
pub fn finished(sessions: &[SessionSpan], now: i64, all: bool) -> Vec<Grind> {
    let mut by_map: BTreeMap<Option<MapId>, Vec<SessionSpan>> = BTreeMap::new();
    for s in sessions {
        by_map.entry(s.map).or_default().push(*s);
    }
    let mut done = Vec::new();
    for (map, mut spans) in by_map {
        spans.sort_by_key(|s| s.started_at);
        let mut grind = Grind { map, sessions: Vec::new(), training_secs: 0 };
        // When the grind's last session ended; `None` while one is open.
        let mut last_end = None;
        for s in spans {
            if let Some(end) = last_end
                && s.started_at - end >= JOIN_GAP_SECS
            {
                done.push(std::mem::replace(&mut grind, Grind { map, sessions: Vec::new(), training_secs: 0 }));
            }
            grind.sessions.push(s.id);
            grind.training_secs += s.training_secs;
            last_end = s.ended_at;
        }
        if let Some(end) = last_end
            && (all || now - end >= JOIN_GAP_SECS)
        {
            done.push(grind);
        }
    }
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIELD: Option<MapId> = Some(1);
    const TOWN: Option<MapId> = Some(2);
    const MIN: i64 = 60;

    fn span(id: i64, map: Option<MapId>, from_min: i64, to_min: Option<i64>, training_min: u64) -> SessionSpan {
        SessionSpan { id, map, started_at: from_min * MIN, ended_at: to_min.map(|m| m * MIN), training_secs: training_min * 60 }
    }

    #[test]
    fn a_town_trip_joins_a_long_gap_or_a_reset_ends_and_short_grinds_are_dropped() {
        let sessions = [
            // 40 min on the field, a quest handed in in town, 25 min back on the field.
            span(1, FIELD, 0, Some(40), 38),
            span(2, TOWN, 42, Some(44), 1),
            span(3, FIELD, 50, Some(75), 24),
            // Back on the field 30 min after leaving it: a new grind, still going.
            span(4, FIELD, 105, None, 5),
        ];
        let over = finished(&sessions, 106 * MIN, false);
        assert_eq!(over, [
            Grind { map: FIELD, sessions: vec![1, 3], training_secs: 62 * 60 },
            Grind { map: TOWN, sessions: vec![2], training_secs: 60 },
        ]);
        assert_eq!(over.iter().map(Grind::worth_saving).collect::<Vec<_>>(), [true, false]);

        // The open session's grind waits; once it ends, a reset finishes it at once.
        let ended = [span(4, FIELD, 105, Some(110), 5)];
        assert!(finished(&ended, 111 * MIN, false).is_empty());
        assert_eq!(finished(&ended, 111 * MIN, true), [Grind { map: FIELD, sessions: vec![4], training_secs: 300 }]);
    }
}
