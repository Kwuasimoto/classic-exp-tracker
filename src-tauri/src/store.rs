//! Single-file SQLite storage, compiled into the exe. Parsed numbers only — never images.
//! Default rollback journal (not WAL) so the data stays one file.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

use crate::rates::MinuteRow;
use crate::read::{GlyphKind, LearnedGlyph};
use crate::read::map_strip::Signature;
use crate::tracker::MapId;

/// The `calibration` table (boxes drawn per resolution) is unread since 0.4.0, when the
/// app started placing its boxes itself (geom::boxes_for); its rows are left as they are.
const SCHEMA_VERSION: i64 = 7;
const SCHEMA: &str = "
BEGIN;
CREATE TABLE maps (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    signature BLOB NOT NULL,
    first_seen INTEGER NOT NULL
);
CREATE TABLE sessions (
    id INTEGER PRIMARY KEY,
    map_id INTEGER NOT NULL REFERENCES maps(id),
    started_at INTEGER NOT NULL,
    ended_at INTEGER
);
CREATE INDEX sessions_map ON sessions(map_id);
CREATE TABLE minutes (
    minute_start INTEGER NOT NULL,
    session_id INTEGER NOT NULL REFERENCES sessions(id),
    exp_gained INTEGER NOT NULL,
    training_secs INTEGER NOT NULL,
    PRIMARY KEY (minute_start, session_id)
);
CREATE INDEX minutes_session ON minutes(session_id);
CREATE TABLE calibration (
    resolution TEXT PRIMARY KEY,
    exp_x INTEGER NOT NULL, exp_y INTEGER NOT NULL, exp_w INTEGER NOT NULL, exp_h INTEGER NOT NULL,
    map_x INTEGER NOT NULL, map_y INTEGER NOT NULL, map_w INTEGER NOT NULL, map_h INTEGER NOT NULL
);
PRAGMA user_version = 1;
COMMIT;
";
/// Version 2: optional level box in the calibration, and the level digits the
/// app has learned (ASCII art rows joined by newlines; see read/level.rs).
const MIGRATION_2: &str = "
BEGIN;
CREATE TABLE level_glyphs (
    id INTEGER PRIMARY KEY,
    ch TEXT NOT NULL,
    art TEXT NOT NULL
);
ALTER TABLE calibration ADD COLUMN level_x INTEGER;
ALTER TABLE calibration ADD COLUMN level_y INTEGER;
ALTER TABLE calibration ADD COLUMN level_w INTEGER;
ALTER TABLE calibration ADD COLUMN level_h INTEGER;
PRAGMA user_version = 2;
COMMIT;
";
/// Version 3: whether a map's signature was cut by the map box; saved session
/// snapshots (a copy of their minutes); where the live view starts (moved by a reset).
const MIGRATION_3: &str = "
BEGIN;
ALTER TABLE maps ADD COLUMN cut_by_box INTEGER NOT NULL DEFAULT 0;
CREATE TABLE snapshots (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    map_name TEXT,
    level INTEGER
);
CREATE TABLE snapshot_minutes (
    snapshot_id INTEGER NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
    minute_start INTEGER NOT NULL,
    exp_gained INTEGER NOT NULL,
    training_secs INTEGER NOT NULL,
    PRIMARY KEY (snapshot_id, minute_start)
);
CREATE TABLE settings (key TEXT PRIMARY KEY, value INTEGER NOT NULL);
PRAGMA user_version = 3;
COMMIT;
";
/// Version 4: a session's map is optional, so EXP counts before a map is confirmed.
/// SQLite can't drop NOT NULL in place, so the table is rebuilt; foreign keys are off
/// meanwhile so dropping the old table leaves `minutes` alone.
const MIGRATION_4: &str = "
PRAGMA foreign_keys = OFF;
BEGIN;
CREATE TABLE sessions_new (
    id INTEGER PRIMARY KEY,
    map_id INTEGER REFERENCES maps(id),
    started_at INTEGER NOT NULL,
    ended_at INTEGER
);
INSERT INTO sessions_new (id, map_id, started_at, ended_at) SELECT id, map_id, started_at, ended_at FROM sessions;
DROP TABLE sessions;
ALTER TABLE sessions_new RENAME TO sessions;
CREATE INDEX sessions_map ON sessions(map_id);
PRAGMA user_version = 4;
COMMIT;
PRAGMA foreign_keys = ON;
";
/// Version 5: glyphs learned per resolution, EXP and level alike (level digits were
/// global; they move to the calibrated resolution when there is exactly one, and are
/// otherwise relearned), and the unrecognised shapes waiting on the Glyphs page.
const MIGRATION_5: &str = "
BEGIN;
CREATE TABLE glyphs (
    id INTEGER PRIMARY KEY,
    resolution TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('exp', 'level')),
    ch TEXT NOT NULL,
    art TEXT NOT NULL
);
CREATE INDEX glyphs_resolution ON glyphs(resolution);
INSERT INTO glyphs (resolution, kind, ch, art)
    SELECT (SELECT resolution FROM calibration), 'level', ch, art FROM level_glyphs
    WHERE (SELECT COUNT(*) FROM calibration) = 1
    ORDER BY id;
DROP TABLE level_glyphs;
CREATE TABLE unknown_glyphs (
    id INTEGER PRIMARY KEY,
    resolution TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('exp', 'level')),
    art TEXT NOT NULL,
    first_seen INTEGER NOT NULL
);
CREATE INDEX unknown_glyphs_resolution ON unknown_glyphs(resolution);
PRAGMA user_version = 5;
COMMIT;
";
/// Version 6: dismissed unknown shapes are kept (hidden), so the same shape isn't
/// recorded again while it stays on screen.
const MIGRATION_6: &str = "
BEGIN;
ALTER TABLE unknown_glyphs ADD COLUMN dismissed INTEGER NOT NULL DEFAULT 0;
PRAGMA user_version = 6;
COMMIT;
";

/// Learned shapes kept per character and resolution: enough for the sub-pixel
/// positions a character lands at, without growing forever.
const MAX_GLYPHS_PER_CHAR: i64 = 8;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("{0} is out of range for storage")]
    Range(&'static str),
    #[error("database was written by a newer version of the app (schema {0})")]
    NewerSchema(i64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapRecord {
    pub id: MapId,
    pub name: String,
    pub signature: Signature,
}

/// A learned glyph as stored, with the id the Glyphs page forgets it by.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredGlyph {
    pub id: i64,
    pub kind: GlyphKind,
    pub glyph: LearnedGlyph,
}

/// A shape the app couldn't read, waiting on the Glyphs page for its character, or
/// dismissed there as not part of a readout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownGlyph {
    pub id: i64,
    pub kind: GlyphKind,
    pub rows: Vec<String>,
    pub dismissed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotRecord {
    pub id: i64,
    pub name: String,
    pub created_at: i64,
    pub map_name: Option<String>,
    pub level: Option<u32>,
    /// First and last+1 minute covered; `None` for an empty snapshot.
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub exp_gained: u64,
    pub training_secs: u64,
}

pub struct Store {
    conn: Connection,
}

fn to_i64(value: u64, what: &'static str) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(|_| StoreError::Range(what))
}

fn col_u64(row: &Row, idx: usize) -> rusqlite::Result<u64> {
    let v: i64 = row.get(idx)?;
    u64::try_from(v).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(idx, v))
}

fn col_u32(row: &Row, idx: usize) -> rusqlite::Result<u32> {
    let v: i64 = row.get(idx)?;
    u32::try_from(v).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(idx, v))
}

fn minute_row(row: &Row) -> rusqlite::Result<MinuteRow> {
    Ok(MinuteRow { minute_start: row.get(0)?, exp_gained: col_u64(row, 1)?, training_secs: col_u32(row, 2)? })
}

fn kind_at(row: &Row, idx: usize) -> rusqlite::Result<GlyphKind> {
    let s: String = row.get(idx)?;
    GlyphKind::parse(&s).ok_or_else(|| rusqlite::Error::InvalidColumnType(idx, s, rusqlite::types::Type::Text))
}

fn char_at(row: &Row, idx: usize) -> rusqlite::Result<char> {
    let s: String = row.get(idx)?;
    s.chars().next().ok_or_else(|| rusqlite::Error::InvalidColumnType(idx, s, rusqlite::types::Type::Text))
}

fn art_at(row: &Row, idx: usize) -> rusqlite::Result<Vec<String>> {
    Ok(row.get::<_, String>(idx)?.lines().map(str::to_string).collect())
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, StoreError> {
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(StoreError::NewerSchema(version));
        }
        if version < 1 {
            conn.execute_batch(SCHEMA)?;
        }
        if version < 2 {
            conn.execute_batch(MIGRATION_2)?;
        }
        if version < 3 {
            conn.execute_batch(MIGRATION_3)?;
        }
        if version < 4 {
            conn.execute_batch(MIGRATION_4)?;
        }
        if version < 5 {
            conn.execute_batch(MIGRATION_5)?;
        }
        if version < 6 {
            conn.execute_batch(MIGRATION_6)?;
        }
        if version < 7 {
            // Version 7 seeded starter EXP glyphs at 1920x1080. Since 0.3.0 the EXP reader
            // matches the game's font and reads no EXP glyphs, so new databases skip it.
            conn.execute_batch("PRAGMA user_version = 7;")?;
        }
        Ok(Self { conn })
    }

    pub fn maps(&self) -> Result<Vec<MapRecord>, StoreError> {
        let mut stmt = self.conn.prepare("SELECT id, name, signature, cut_by_box FROM maps ORDER BY id")?;
        let rows = stmt
            .query_map([], |r| {
                Ok(MapRecord { id: r.get(0)?, name: r.get(1)?, signature: Signature { counts: r.get(2)?, cut: r.get(3)? } })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn map_name(&self, id: MapId) -> Result<Option<String>, StoreError> {
        Ok(self.conn.query_row("SELECT name FROM maps WHERE id = ?1", [id], |r| r.get(0)).optional()?)
    }

    pub fn insert_map(&self, name: &str, signature: &Signature, t: i64) -> Result<MapId, StoreError> {
        self.conn.execute(
            "INSERT INTO maps (name, signature, cut_by_box, first_seen) VALUES (?1, ?2, ?3, ?4)",
            params![name, signature.counts.as_slice(), signature.cut, t],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn start_session(&self, map_id: Option<MapId>, t: i64) -> Result<i64, StoreError> {
        self.conn.execute("INSERT INTO sessions (map_id, started_at) VALUES (?1, ?2)", params![map_id, t])?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn end_session(&self, id: i64, t: i64) -> Result<(), StoreError> {
        self.conn.execute("UPDATE sessions SET ended_at = ?2 WHERE id = ?1", params![id, t])?;
        Ok(())
    }

    pub fn add_minute(&self, minute_start: i64, session_id: i64, exp_gained: u64, training_secs: u32) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO minutes (minute_start, session_id, exp_gained, training_secs) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (minute_start, session_id) DO UPDATE SET
               exp_gained = exp_gained + excluded.exp_gained,
               training_secs = training_secs + excluded.training_secs",
            params![minute_start, session_id, to_i64(exp_gained, "exp_gained")?, i64::from(training_secs)],
        )?;
        Ok(())
    }

    /// Close sessions left open by a crash or kill, at their last recorded minute.
    pub fn close_open_sessions(&self) -> Result<usize, StoreError> {
        Ok(self.conn.execute(
            "UPDATE sessions SET ended_at = COALESCE(
                (SELECT MAX(minute_start) + 60 FROM minutes WHERE session_id = sessions.id), started_at)
             WHERE ended_at IS NULL",
            [],
        )?)
    }

    /// Delete minutes from before `before`, and the ended sessions left without any.
    /// The live view needs only its chart window; History keeps snapshots, which are copies.
    pub fn prune(&self, before: i64) -> Result<(), StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM minutes WHERE minute_start < ?1", [before])?;
        tx.execute(
            "DELETE FROM sessions WHERE ended_at IS NOT NULL AND NOT EXISTS (SELECT 1 FROM minutes WHERE session_id = sessions.id)",
            [],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Minute totals across all sessions, `from` inclusive to `to` exclusive.
    pub fn minutes_between(&self, from: i64, to: i64) -> Result<Vec<MinuteRow>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT minute_start, SUM(exp_gained), SUM(training_secs) FROM minutes
             WHERE minute_start >= ?1 AND minute_start < ?2 GROUP BY minute_start ORDER BY minute_start",
        )?;
        let rows = stmt.query_map(params![from, to], minute_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// The level digits learned at `resolution`, oldest first. EXP glyphs from before
    /// 0.3.0 stay in the table unread (see read::GlyphKind), here and below.
    pub fn glyphs(&self, resolution: &str) -> Result<Vec<StoredGlyph>, StoreError> {
        let mut stmt =
            self.conn.prepare("SELECT id, kind, ch, art FROM glyphs WHERE resolution = ?1 AND kind = 'level' ORDER BY id")?;
        let rows = stmt
            .query_map([resolution], |r| {
                Ok(StoredGlyph { id: r.get(0)?, kind: kind_at(r, 1)?, glyph: LearnedGlyph { ch: char_at(r, 2)?, rows: art_at(r, 3)? } })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Store a learned glyph; `false` when its character already has MAX_GLYPHS_PER_CHAR here.
    pub fn add_glyph(&self, resolution: &str, kind: GlyphKind, glyph: &LearnedGlyph) -> Result<bool, StoreError> {
        let added = self.conn.execute(
            "INSERT INTO glyphs (resolution, kind, ch, art)
             SELECT ?1, ?2, ?3, ?4
             WHERE (SELECT COUNT(*) FROM glyphs WHERE resolution = ?1 AND kind = ?2 AND ch = ?3) < ?5",
            params![resolution, kind.as_str(), glyph.ch.to_string(), glyph.rows.join("
"), MAX_GLYPHS_PER_CHAR],
        )?;
        Ok(added == 1)
    }

    /// Resolutions with learned glyphs or unknown glyphs waiting, in no particular order.
    pub fn glyph_resolutions(&self) -> Result<Vec<String>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT resolution FROM glyphs WHERE kind = 'level' UNION SELECT resolution FROM unknown_glyphs WHERE kind = 'level' AND NOT dismissed")?;
        let rows = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
        Ok(rows)
    }

    /// Add imported glyphs at `resolution`, skipping exact copies of ones already
    /// there (the user asked for them, so MAX_GLYPHS_PER_CHAR doesn't apply). Returns
    /// how many were added.
    pub fn import_glyphs(&self, resolution: &str, glyphs: &[(GlyphKind, LearnedGlyph)]) -> Result<usize, StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        let mut added = 0;
        for (kind, glyph) in glyphs {
            added += tx.execute(
                "INSERT INTO glyphs (resolution, kind, ch, art)
                 SELECT ?1, ?2, ?3, ?4
                 WHERE NOT EXISTS (SELECT 1 FROM glyphs WHERE resolution = ?1 AND kind = ?2 AND ch = ?3 AND art = ?4)",
                params![resolution, kind.as_str(), glyph.ch.to_string(), glyph.rows.join("\n")],
            )?;
        }
        tx.commit()?;
        Ok(added)
    }

    /// Which readout a learned glyph belongs to; `None` when no learned glyph has that id.
    pub fn glyph_kind(&self, id: i64) -> Result<Option<GlyphKind>, StoreError> {
        Ok(self.conn.query_row("SELECT kind FROM glyphs WHERE id = ?1 AND kind = 'level'", [id], |r| kind_at(r, 0)).optional()?)
    }

    /// Change a learned glyph's character (it was learned or named wrongly).
    pub fn relabel_glyph(&self, id: i64, ch: char) -> Result<(), StoreError> {
        self.conn.execute("UPDATE glyphs SET ch = ?2 WHERE id = ?1", params![id, ch.to_string()])?;
        Ok(())
    }

    /// `false` when no learned glyph has that id.
    pub fn forget_glyph(&self, id: i64) -> Result<bool, StoreError> {
        Ok(self.conn.execute("DELETE FROM glyphs WHERE id = ?1 AND kind = 'level'", [id])? == 1)
    }

    /// Waiting and dismissed alike.
    pub fn unknown_glyphs(&self, resolution: &str) -> Result<Vec<UnknownGlyph>, StoreError> {
        let mut stmt = self.conn.prepare("SELECT id, kind, art, dismissed FROM unknown_glyphs WHERE resolution = ?1 AND kind = 'level' ORDER BY id")?;
        let rows = stmt
            .query_map([resolution], |r| {
                Ok(UnknownGlyph { id: r.get(0)?, kind: kind_at(r, 1)?, rows: art_at(r, 2)?, dismissed: r.get(3)? })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// A waiting (not dismissed) unknown glyph.
    pub fn unknown_glyph(&self, id: i64) -> Result<Option<UnknownGlyph>, StoreError> {
        Ok(self
            .conn
            .query_row("SELECT id, kind, art, dismissed FROM unknown_glyphs WHERE id = ?1 AND kind = 'level' AND NOT dismissed", [id], |r| {
                Ok(UnknownGlyph { id: r.get(0)?, kind: kind_at(r, 1)?, rows: art_at(r, 2)?, dismissed: r.get(3)? })
            })
            .optional()?)
    }

    pub fn add_unknown_glyph(&self, resolution: &str, kind: GlyphKind, rows: &[String], t: i64) -> Result<i64, StoreError> {
        self.conn.execute(
            "INSERT INTO unknown_glyphs (resolution, kind, art, first_seen) VALUES (?1, ?2, ?3, ?4)",
            params![resolution, kind.as_str(), rows.join("
"), t],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Hide a waiting unknown glyph; it's kept so the shape isn't recorded again.
    /// `false` when no waiting unknown glyph has that id.
    pub fn dismiss_unknown_glyph(&self, id: i64) -> Result<bool, StoreError> {
        Ok(self.conn.execute("UPDATE unknown_glyphs SET dismissed = 1 WHERE id = ?1 AND kind = 'level' AND NOT dismissed", [id])? == 1)
    }

    /// Delete an unknown glyph that a learned glyph now reads.
    pub fn delete_unknown_glyph(&self, id: i64) -> Result<(), StoreError> {
        self.conn.execute("DELETE FROM unknown_glyphs WHERE id = ?1", [id])?;
        Ok(())
    }

    /// The user named an unknown glyph: it becomes a learned glyph `ch` at its resolution
    /// (even past MAX_GLYPHS_PER_CHAR: the user asked for it). `false` when there's no such entry.
    pub fn assign_unknown_glyph(&self, id: i64, ch: char) -> Result<bool, StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        let added = tx.execute(
            "INSERT INTO glyphs (resolution, kind, ch, art) SELECT resolution, kind, ?2, art FROM unknown_glyphs WHERE id = ?1 AND NOT dismissed",
            params![id, ch.to_string()],
        )?;
        tx.execute("DELETE FROM unknown_glyphs WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok(added == 1)
    }

    /// Where the live view starts (unix seconds); 0 until the first reset.
    pub fn live_since(&self) -> Result<i64, StoreError> {
        Ok(self.conn.query_row("SELECT value FROM settings WHERE key = 'live_since'", [], |r| r.get(0)).optional()?.unwrap_or(0))
    }

    pub fn set_live_since(&self, t: i64) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES ('live_since', ?1) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            [t],
        )?;
        Ok(())
    }

    /// Save a snapshot and its own copy of `rows`, in one transaction. Rows for the same
    /// minute add up: a session that ended mid-minute stored part of the minute in progress.
    pub fn create_snapshot(&self, name: &str, created_at: i64, map_name: Option<&str>, level: Option<u32>, rows: &[MinuteRow]) -> Result<i64, StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO snapshots (name, created_at, map_name, level) VALUES (?1, ?2, ?3, ?4)",
            params![name, created_at, map_name, level],
        )?;
        let id = tx.last_insert_rowid();
        for r in rows {
            tx.execute(
                "INSERT INTO snapshot_minutes (snapshot_id, minute_start, exp_gained, training_secs) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (snapshot_id, minute_start) DO UPDATE SET
                   exp_gained = exp_gained + excluded.exp_gained,
                   training_secs = training_secs + excluded.training_secs",
                params![id, r.minute_start, to_i64(r.exp_gained, "exp_gained")?, i64::from(r.training_secs)],
            )?;
        }
        tx.commit()?;
        Ok(id)
    }

    pub fn snapshots(&self) -> Result<Vec<SnapshotRecord>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.name, s.created_at, s.map_name, s.level, MIN(m.minute_start), MAX(m.minute_start) + 60,
                    COALESCE(SUM(m.exp_gained), 0), COALESCE(SUM(m.training_secs), 0)
             FROM snapshots s LEFT JOIN snapshot_minutes m ON m.snapshot_id = s.id
             GROUP BY s.id ORDER BY s.created_at DESC",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(SnapshotRecord {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    created_at: r.get(2)?,
                    map_name: r.get(3)?,
                    level: r.get(4)?,
                    from: r.get(5)?,
                    to: r.get(6)?,
                    exp_gained: col_u64(r, 7)?,
                    training_secs: col_u64(r, 8)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn snapshot_minutes(&self, id: i64) -> Result<Vec<MinuteRow>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT minute_start, exp_gained, training_secs FROM snapshot_minutes WHERE snapshot_id = ?1 ORDER BY minute_start",
        )?;
        let rows = stmt.query_map([id], minute_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// `false` when no snapshot has that id.
    pub fn rename_snapshot(&self, id: i64, name: &str) -> Result<bool, StoreError> {
        Ok(self.conn.execute("UPDATE snapshots SET name = ?2 WHERE id = ?1", params![id, name])? == 1)
    }

    /// `false` when no snapshot has that id. Its minutes go with it (ON DELETE CASCADE).
    pub fn delete_snapshot(&self, id: i64) -> Result<bool, StoreError> {
        Ok(self.conn.execute("DELETE FROM snapshots WHERE id = ?1", [id])? == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prune_keeps_snapshots_recent_minutes_and_the_open_session() {
        let store = Store::open_in_memory().unwrap();
        let map = store.insert_map("Map", &Signature { counts: vec![1], cut: false }, 0).unwrap();
        let old = store.start_session(Some(map), 0).unwrap();
        store.add_minute(0, old, 100, 60).unwrap();
        store.end_session(old, 60).unwrap();
        let open = store.start_session(None, 6_000).unwrap();
        store.add_minute(6_000, open, 50, 60).unwrap();
        let row = MinuteRow { minute_start: 0, exp_gained: 100, training_secs: 60 };
        let snapshot = store.create_snapshot("Saved", 60, None, None, &[row, row]).unwrap();

        store.prune(3_000).unwrap();

        assert_eq!(store.minutes_between(0, 10_000).unwrap(), [MinuteRow { minute_start: 6_000, exp_gained: 50, training_secs: 60 }]);
        let sessions: i64 = store.conn.query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0)).unwrap();
        assert_eq!(sessions, 1);
        assert_eq!(store.snapshot_minutes(snapshot).unwrap(), [MinuteRow { minute_start: 0, exp_gained: 200, training_secs: 120 }]);
    }
}
