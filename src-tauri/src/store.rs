//! Single-file SQLite storage, compiled into the exe. Parsed numbers only — never images.
//! Default rollback journal (not WAL) so the data stays one file.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

use crate::geom::{Calibration, Rect};
use crate::rates::MinuteRow;
use crate::read::map_strip::Signature;
use crate::tracker::MapId;

const SCHEMA_VERSION: i64 = 1;
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapTotals {
    pub map_id: MapId,
    pub name: String,
    pub sessions: u32,
    pub exp_gained: u64,
    pub training_secs: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub id: i64,
    pub map_id: MapId,
    pub map_name: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
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

fn rect_at(row: &Row, first: usize) -> rusqlite::Result<Rect> {
    Ok(Rect { x: col_u32(row, first)?, y: col_u32(row, first + 1)?, w: col_u32(row, first + 2)?, h: col_u32(row, first + 3)? })
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
        match version {
            0 => conn.execute_batch(SCHEMA)?,
            SCHEMA_VERSION => {}
            newer => return Err(StoreError::NewerSchema(newer)),
        }
        Ok(Self { conn })
    }

    pub fn maps(&self) -> Result<Vec<MapRecord>, StoreError> {
        let mut stmt = self.conn.prepare("SELECT id, name, signature FROM maps ORDER BY id")?;
        let rows = stmt
            .query_map([], |r| Ok(MapRecord { id: r.get(0)?, name: r.get(1)?, signature: Signature(r.get(2)?) }))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn map_name(&self, id: MapId) -> Result<Option<String>, StoreError> {
        Ok(self.conn.query_row("SELECT name FROM maps WHERE id = ?1", [id], |r| r.get(0)).optional()?)
    }

    pub fn insert_map(&self, name: &str, signature: &Signature, t: i64) -> Result<MapId, StoreError> {
        self.conn.execute(
            "INSERT INTO maps (name, signature, first_seen) VALUES (?1, ?2, ?3)",
            params![name, signature.0.as_slice(), t],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// `false` when no map has that id.
    pub fn rename_map(&self, id: MapId, name: &str) -> Result<bool, StoreError> {
        Ok(self.conn.execute("UPDATE maps SET name = ?2 WHERE id = ?1", params![id, name])? == 1)
    }

    pub fn start_session(&self, map_id: MapId, t: i64) -> Result<i64, StoreError> {
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

    /// Minute totals across all sessions, `from` inclusive to `to` exclusive.
    pub fn minutes_between(&self, from: i64, to: i64) -> Result<Vec<MinuteRow>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT minute_start, SUM(exp_gained), SUM(training_secs) FROM minutes
             WHERE minute_start >= ?1 AND minute_start < ?2 GROUP BY minute_start ORDER BY minute_start",
        )?;
        let rows = stmt.query_map(params![from, to], minute_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn session_minutes(&self, session_id: i64) -> Result<Vec<MinuteRow>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT minute_start, exp_gained, training_secs FROM minutes WHERE session_id = ?1 ORDER BY minute_start",
        )?;
        let rows = stmt.query_map([session_id], minute_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn map_minutes(&self, map_id: MapId) -> Result<Vec<MinuteRow>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT mi.minute_start, SUM(mi.exp_gained), SUM(mi.training_secs)
             FROM minutes mi JOIN sessions s ON s.id = mi.session_id
             WHERE s.map_id = ?1 GROUP BY mi.minute_start ORDER BY mi.minute_start",
        )?;
        let rows = stmt.query_map([map_id], minute_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn map_totals(&self) -> Result<Vec<MapTotals>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT m.id, m.name, COUNT(DISTINCT s.id), COALESCE(SUM(mi.exp_gained), 0), COALESCE(SUM(mi.training_secs), 0)
             FROM maps m JOIN sessions s ON s.map_id = m.id LEFT JOIN minutes mi ON mi.session_id = s.id
             GROUP BY m.id ORDER BY 5 DESC",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(MapTotals {
                    map_id: r.get(0)?,
                    name: r.get(1)?,
                    sessions: col_u32(r, 2)?,
                    exp_gained: col_u64(r, 3)?,
                    training_secs: col_u64(r, 4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn sessions(&self, map_id: Option<MapId>) -> Result<Vec<SessionRecord>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.map_id, m.name, s.started_at, s.ended_at,
                    COALESCE(SUM(mi.exp_gained), 0), COALESCE(SUM(mi.training_secs), 0)
             FROM sessions s JOIN maps m ON m.id = s.map_id LEFT JOIN minutes mi ON mi.session_id = s.id
             WHERE ?1 IS NULL OR s.map_id = ?1
             GROUP BY s.id ORDER BY s.started_at DESC LIMIT 500",
        )?;
        let rows = stmt
            .query_map([map_id], |r| {
                Ok(SessionRecord {
                    id: r.get(0)?,
                    map_id: r.get(1)?,
                    map_name: r.get(2)?,
                    started_at: r.get(3)?,
                    ended_at: r.get(4)?,
                    exp_gained: col_u64(r, 5)?,
                    training_secs: col_u64(r, 6)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn calibration(&self, resolution: &str) -> Result<Option<Calibration>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT exp_x, exp_y, exp_w, exp_h, map_x, map_y, map_w, map_h FROM calibration WHERE resolution = ?1",
                [resolution],
                |r| Ok(Calibration { exp: rect_at(r, 0)?, map: rect_at(r, 4)? }),
            )
            .optional()?)
    }

    pub fn save_calibration(&self, resolution: &str, c: &Calibration) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO calibration (resolution, exp_x, exp_y, exp_w, exp_h, map_x, map_y, map_w, map_h)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT (resolution) DO UPDATE SET
               exp_x = excluded.exp_x, exp_y = excluded.exp_y, exp_w = excluded.exp_w, exp_h = excluded.exp_h,
               map_x = excluded.map_x, map_y = excluded.map_y, map_w = excluded.map_w, map_h = excluded.map_h",
            params![resolution, c.exp.x, c.exp.y, c.exp.w, c.exp.h, c.map.x, c.map.y, c.map.w, c.map.h],
        )?;
        Ok(())
    }
}
