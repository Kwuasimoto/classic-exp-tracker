//! Single-file SQLite storage, compiled into the exe. Parsed numbers only — never images.
//! Default rollback journal (not WAL) so the data stays one file.

use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

use crate::geom::Corner;
use crate::grinds::SessionSpan;
use crate::items::{ItemKind, PotionSlot};
use crate::rates::{Loot, MinuteRow, Totals};
use crate::read::LearnedGlyph;
use crate::read::map_strip::Signature;
use crate::tracker::loot::ItemId;
use crate::tracker::MapId;

/// The `calibration` table (boxes drawn per resolution) is unread since 0.4.0, when the
/// app started placing its boxes itself (geom::boxes_for); its rows are left as they are.
const SCHEMA_VERSION: i64 = 12;
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

/// Version 8 (0.5.0): maps start from scratch. Names from OCR alone were wrong about a
/// third of the time, and one map was saved once per look (resolution, minimap width).
/// A map now has a name only from the map list (read::map_names) or from the user, and
/// any number of looks. Sessions keep their minutes and lose their map; snapshots keep
/// the names they were saved with. The database is copied first (Store::open).
const MIGRATION_8: &str = "
PRAGMA foreign_keys = OFF;
BEGIN;
UPDATE sessions SET map_id = NULL;
DROP TABLE maps;
CREATE TABLE maps (
    id INTEGER PRIMARY KEY,
    name TEXT,
    named_by TEXT CHECK (named_by IN ('list', 'user')),
    first_seen INTEGER NOT NULL,
    CHECK ((name IS NULL) = (named_by IS NULL))
);
CREATE TABLE map_signatures (
    id INTEGER PRIMARY KEY,
    map_id INTEGER NOT NULL REFERENCES maps(id),
    signature BLOB NOT NULL,
    cut_by_box INTEGER NOT NULL
);
CREATE INDEX map_signatures_map ON map_signatures(map_id);
PRAGMA user_version = 8;
COMMIT;
PRAGMA foreign_keys = ON;
";

/// Version 9 (0.6.0): meso tracking. Items seen in the inventory's Etc tab or on the
/// quickslot, named from the item list (read::items) by the user, each with the patches it
/// was recognised by; and the mesos each minute made, beside its EXP. Adds only.
const MIGRATION_9: &str = "
BEGIN;
CREATE TABLE items (
    id INTEGER PRIMARY KEY,
    name TEXT,
    kind TEXT NOT NULL CHECK (kind IN ('etc', 'use')),
    first_seen INTEGER NOT NULL
);
CREATE TABLE item_patches (
    id INTEGER PRIMARY KEY,
    item_id INTEGER NOT NULL REFERENCES items(id),
    patch BLOB NOT NULL
);
CREATE INDEX item_patches_item ON item_patches(item_id);
ALTER TABLE minutes ADD COLUMN meso INTEGER NOT NULL DEFAULT 0;
ALTER TABLE minutes ADD COLUMN etc_value INTEGER NOT NULL DEFAULT 0;
ALTER TABLE minutes ADD COLUMN potion_cost INTEGER NOT NULL DEFAULT 0;
ALTER TABLE snapshot_minutes ADD COLUMN meso INTEGER NOT NULL DEFAULT 0;
ALTER TABLE snapshot_minutes ADD COLUMN etc_value INTEGER NOT NULL DEFAULT 0;
ALTER TABLE snapshot_minutes ADD COLUMN potion_cost INTEGER NOT NULL DEFAULT 0;
PRAGMA user_version = 9;
COMMIT;
";

/// Version 10 (0.6.0): items the user doesn't want tracked (look-alikes that can't be told
/// apart, say). Adds only.
const MIGRATION_10: &str = "
BEGIN;
ALTER TABLE items ADD COLUMN ignored INTEGER NOT NULL DEFAULT 0;
PRAGMA user_version = 10;
COMMIT;
";

/// Version 11 (0.8.0): automatic snapshots (grinds.rs). A session is filed once its grind is
/// saved or dropped; until then its minutes outlive the live chart's window. Sessions from
/// before are filed already, and snapshots from before are the user's own.
const MIGRATION_11: &str = "
BEGIN;
ALTER TABLE sessions ADD COLUMN filed INTEGER NOT NULL DEFAULT 0;
UPDATE sessions SET filed = 1;
ALTER TABLE snapshots ADD COLUMN auto INTEGER NOT NULL DEFAULT 0;
PRAGMA user_version = 11;
COMMIT;
";

/// Version 12 (0.8.0): whether the inventory was seen in a minute with everything made counted
/// (rates::MinuteRow::loot_seen). Minutes from before count as seen, so their rates stay as they were.
const MIGRATION_12: &str = "
BEGIN;
ALTER TABLE minutes ADD COLUMN loot_seen INTEGER NOT NULL DEFAULT 1;
ALTER TABLE snapshot_minutes ADD COLUMN loot_seen INTEGER NOT NULL DEFAULT 1;
PRAGMA user_version = 12;
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
    #[error("could not back up the database before upgrading it: {0}")]
    Backup(#[from] std::io::Error),
}

/// Where a map's name came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamedBy {
    /// The map list (read::map_names).
    List,
    User,
}

impl NamedBy {
    fn as_str(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::User => "user",
        }
    }
}

/// An item and one patch it was recognised by (an item can have several).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemPatch {
    pub item_id: ItemId,
    pub kind: ItemKind,
    pub name: Option<String>,
    /// The user doesn't want it tracked: it's recognised, never counted.
    pub ignored: bool,
    pub patch: Vec<u8>,
}

/// One look of a map: its name strip's signature at some resolution and minimap width.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapRecord {
    pub id: MapId,
    pub signature: Signature,
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
    pub loot: Loot,
    /// Saved automatically from a grind, and deleted grinds::KEEP_AUTO_SECS after `created_at`
    /// unless the user keeps it.
    pub auto: bool,
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

/// Columns from `idx`: meso, etc_value, potion_cost.
fn loot_at(row: &Row, idx: usize) -> rusqlite::Result<Loot> {
    Ok(Loot { meso: col_u64(row, idx)?, etc_value: col_u64(row, idx + 1)?, potion_cost: col_u64(row, idx + 2)? })
}

/// Columns: minute_start, exp_gained, training_secs, meso, etc_value, potion_cost, loot_seen.
fn minute_row(row: &Row) -> rusqlite::Result<MinuteRow> {
    Ok(MinuteRow { minute_start: row.get(0)?, exp_gained: col_u64(row, 1)?, training_secs: col_u32(row, 2)?, loot: loot_at(row, 3)?, loot_seen: row.get(6)? })
}

fn kind_at(row: &Row, idx: usize) -> rusqlite::Result<ItemKind> {
    let s: String = row.get(idx)?;
    ItemKind::parse(&s).ok_or_else(|| rusqlite::Error::InvalidColumnType(idx, s, rusqlite::types::Type::Text))
}

fn char_at(row: &Row, idx: usize) -> rusqlite::Result<char> {
    let s: String = row.get(idx)?;
    s.chars().next().ok_or_else(|| rusqlite::Error::InvalidColumnType(idx, s, rusqlite::types::Type::Text))
}

fn art_at(row: &Row, idx: usize) -> rusqlite::Result<Vec<String>> {
    Ok(row.get::<_, String>(idx)?.lines().map(str::to_string).collect())
}

impl Store {
    /// Before any upgrade (migration 8 deletes the saved maps), the file is copied next to itself.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let version: i64 = Connection::open(path)?.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if (1..SCHEMA_VERSION).contains(&version) {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("tracker");
            let backup = path.with_file_name(format!("{stem}.backup-v{version}.db"));
            std::fs::copy(path, &backup)?;
            log::info!("database backed up to {} before upgrading it to version {SCHEMA_VERSION}", backup.display());
        }
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
        if version < 8 {
            conn.execute_batch(MIGRATION_8)?;
        }
        if version < 9 {
            conn.execute_batch(MIGRATION_9)?;
        }
        if version < 10 {
            conn.execute_batch(MIGRATION_10)?;
        }
        if version < 11 {
            conn.execute_batch(MIGRATION_11)?;
        }
        if version < 12 {
            conn.execute_batch(MIGRATION_12)?;
        }
        Ok(Self { conn })
    }

    /// Every look of every map.
    pub fn maps(&self) -> Result<Vec<MapRecord>, StoreError> {
        let mut stmt = self.conn.prepare("SELECT map_id, signature, cut_by_box FROM map_signatures ORDER BY id")?;
        let rows = stmt
            .query_map([], |r| {
                Ok(MapRecord { id: r.get(0)?, signature: Signature { counts: r.get(1)?, cut: r.get(2)? } })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// The map's name, `None` while it has none (or there's no such map).
    pub fn map_name(&self, id: MapId) -> Result<Option<(String, NamedBy)>, StoreError> {
        let row: Option<(Option<String>, Option<String>)> =
            self.conn.query_row("SELECT name, named_by FROM maps WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
        Ok(match row {
            Some((Some(name), Some(by))) => Some((name, if by == "user" { NamedBy::User } else { NamedBy::List })),
            _ => None,
        })
    }

    pub fn has_map(&self, id: MapId) -> Result<bool, StoreError> {
        Ok(self.conn.query_row("SELECT 1 FROM maps WHERE id = ?1", [id], |_| Ok(())).optional()?.is_some())
    }

    /// The map with this name, if one has it.
    pub fn map_by_name(&self, name: &str) -> Result<Option<MapId>, StoreError> {
        Ok(self.conn.query_row("SELECT id FROM maps WHERE name = ?1 ORDER BY id LIMIT 1", [name], |r| r.get(0)).optional()?)
    }

    /// A new map with its first look; `name` is `None` for a map not on the list.
    pub fn insert_map(&self, name: Option<(&str, NamedBy)>, signature: &Signature, t: i64) -> Result<MapId, StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO maps (name, named_by, first_seen) VALUES (?1, ?2, ?3)",
            params![name.map(|(n, _)| n), name.map(|(_, by)| by.as_str()), t],
        )?;
        let id = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO map_signatures (map_id, signature, cut_by_box) VALUES (?1, ?2, ?3)",
            params![id, signature.counts.as_slice(), signature.cut],
        )?;
        tx.commit()?;
        Ok(id)
    }

    /// Another look of a known map (another resolution or minimap width).
    pub fn add_signature(&self, map_id: MapId, signature: &Signature) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO map_signatures (map_id, signature, cut_by_box) VALUES (?1, ?2, ?3)",
            params![map_id, signature.counts.as_slice(), signature.cut],
        )?;
        Ok(())
    }

    /// Map `from` turned out to be `into`: its looks move over, so it's found as `into`
    /// from now on. `from` stays (its sessions and queued writes still refer to it).
    pub fn move_looks(&self, from: MapId, into: MapId) -> Result<(), StoreError> {
        self.conn.execute("UPDATE map_signatures SET map_id = ?2 WHERE map_id = ?1", params![from, into])?;
        Ok(())
    }

    /// `false` when no map has that id.
    pub fn set_map_name(&self, id: MapId, name: &str, by: NamedBy) -> Result<bool, StoreError> {
        Ok(self.conn.execute("UPDATE maps SET name = ?2, named_by = ?3 WHERE id = ?1", params![id, name, by.as_str()])? == 1)
    }

    pub fn start_session(&self, map_id: Option<MapId>, t: i64) -> Result<i64, StoreError> {
        self.conn.execute("INSERT INTO sessions (map_id, started_at) VALUES (?1, ?2)", params![map_id, t])?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn end_session(&self, id: i64, t: i64) -> Result<(), StoreError> {
        self.conn.execute("UPDATE sessions SET ended_at = ?2 WHERE id = ?1", params![id, t])?;
        Ok(())
    }

    /// Add to a minute, its training to the grinding time and its EXP and mesos to the
    /// totals, in one transaction.
    pub fn add_minute(&self, minute_start: i64, session_id: i64, exp_gained: u64, training_secs: u32, loot: Loot, loot_seen: bool) -> Result<(), StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO minutes (minute_start, session_id, exp_gained, training_secs, meso, etc_value, potion_cost, loot_seen)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (minute_start, session_id) DO UPDATE SET
               exp_gained = exp_gained + excluded.exp_gained,
               training_secs = training_secs + excluded.training_secs,
               meso = meso + excluded.meso,
               etc_value = etc_value + excluded.etc_value,
               potion_cost = potion_cost + excluded.potion_cost,
               loot_seen = MAX(loot_seen, excluded.loot_seen)",
            params![
                minute_start,
                session_id,
                to_i64(exp_gained, "exp_gained")?,
                i64::from(training_secs),
                to_i64(loot.meso, "meso")?,
                to_i64(loot.etc_value, "etc_value")?,
                to_i64(loot.potion_cost, "potion_cost")?,
                loot_seen,
            ],
        )?;
        tx.execute(
            "INSERT INTO settings (key, value) VALUES ('grind_secs', ?1) ON CONFLICT (key) DO UPDATE SET value = value + excluded.value",
            [i64::from(training_secs)],
        )?;
        tx.execute(
            "INSERT INTO settings (key, value) VALUES ('total_exp', ?1), ('total_meso', ?2), ('total_etc_value', ?3), ('total_potion_cost', ?4)
             ON CONFLICT (key) DO UPDATE SET value = value + excluded.value",
            params![
                to_i64(exp_gained, "exp_gained")?,
                to_i64(loot.meso, "meso")?,
                to_i64(loot.etc_value, "etc_value")?,
                to_i64(loot.potion_cost, "potion_cost")?,
            ],
        )?;
        tx.execute("INSERT INTO settings (key, value) VALUES ('totals_since', ?1) ON CONFLICT (key) DO NOTHING", [minute_start])?;
        tx.commit()?;
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

    /// Delete minutes from before `before`, and the ended sessions left without any. The live
    /// view needs only its chart window; History keeps snapshots, which are copies. A session
    /// not yet filed keeps its minutes: its grind may still be saved (grinds.rs).
    pub fn prune(&self, before: i64) -> Result<(), StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM minutes WHERE minute_start < ?1 AND session_id IN (SELECT id FROM sessions WHERE filed = 1)", [before])?;
        tx.execute(
            "DELETE FROM sessions WHERE filed = 1 AND ended_at IS NOT NULL AND NOT EXISTS (SELECT 1 FROM minutes WHERE session_id = sessions.id)",
            [],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Minute totals, `from` inclusive to `to` exclusive: across all sessions, or only the
    /// sessions on `map` (the map the live view is locked to).
    pub fn minutes_between(&self, from: i64, to: i64, map: Option<MapId>) -> Result<Vec<MinuteRow>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT m.minute_start, SUM(m.exp_gained), SUM(m.training_secs), SUM(m.meso), SUM(m.etc_value), SUM(m.potion_cost), MAX(m.loot_seen)
             FROM minutes m JOIN sessions s ON s.id = m.session_id
             WHERE m.minute_start >= ?1 AND m.minute_start < ?2 AND (?3 IS NULL OR s.map_id = ?3)
             GROUP BY m.minute_start ORDER BY m.minute_start",
        )?;
        let rows = stmt.query_map(params![from, to, map], minute_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// The level digits learned at `resolution`, oldest first. EXP glyphs from before
    /// 0.3.0, and the shapes the Glyphs page kept before 0.4.0 (`unknown_glyphs`), stay
    /// in the database unread.
    pub fn glyphs(&self, resolution: &str) -> Result<Vec<LearnedGlyph>, StoreError> {
        let mut stmt = self.conn.prepare("SELECT ch, art FROM glyphs WHERE resolution = ?1 AND kind = 'level' ORDER BY id")?;
        let rows = stmt
            .query_map([resolution], |r| Ok(LearnedGlyph { ch: char_at(r, 0)?, rows: art_at(r, 1)? }))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Store a learned level digit; `false` when its character already has
    /// MAX_GLYPHS_PER_CHAR here.
    pub fn add_glyph(&self, resolution: &str, glyph: &LearnedGlyph) -> Result<bool, StoreError> {
        let added = self.conn.execute(
            "INSERT INTO glyphs (resolution, kind, ch, art)
             SELECT ?1, 'level', ?2, ?3
             WHERE (SELECT COUNT(*) FROM glyphs WHERE resolution = ?1 AND kind = 'level' AND ch = ?2) < ?4",
            params![resolution, glyph.ch.to_string(), glyph.rows.join("\n"), MAX_GLYPHS_PER_CHAR],
        )?;
        Ok(added == 1)
    }

    /// Every patch of every item.
    pub fn item_patches(&self) -> Result<Vec<ItemPatch>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT i.id, i.kind, i.name, i.ignored, p.patch FROM item_patches p JOIN items i ON i.id = p.item_id ORDER BY p.id",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(ItemPatch { item_id: r.get(0)?, kind: kind_at(r, 1)?, name: r.get(2)?, ignored: r.get(3)?, patch: r.get(4)? })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// An item seen for the first time, unnamed, with the patch it was seen by.
    pub fn insert_item(&self, kind: ItemKind, patch: &[u8], t: i64) -> Result<ItemId, StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("INSERT INTO items (kind, first_seen) VALUES (?1, ?2)", params![kind.as_str(), t])?;
        let id = tx.last_insert_rowid();
        tx.execute("INSERT INTO item_patches (item_id, patch) VALUES (?1, ?2)", params![id, patch])?;
        tx.commit()?;
        Ok(id)
    }

    /// The user names an item from the item list. When another item of its kind already
    /// has the name (the same item seen by another patch), `id`'s patches move to it and
    /// that item's id is returned; otherwise `id`. `None` when there's no item `id`.
    pub fn name_item(&self, id: ItemId, name: &str) -> Result<Option<ItemId>, StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        let kind: Option<String> = tx.query_row("SELECT kind FROM items WHERE id = ?1", [id], |r| r.get(0)).optional()?;
        let Some(kind) = kind else { return Ok(None) };
        let named: Option<ItemId> = tx
            .query_row("SELECT id FROM items WHERE name = ?1 AND kind = ?2 AND id != ?3", params![name, kind, id], |r| r.get(0))
            .optional()?;
        let into = match named {
            Some(other) => {
                tx.execute("UPDATE item_patches SET item_id = ?2 WHERE item_id = ?1", params![id, other])?;
                tx.execute("DELETE FROM items WHERE id = ?1", [id])?;
                other
            }
            None => {
                tx.execute("UPDATE items SET name = ?2 WHERE id = ?1", params![id, name])?;
                id
            }
        };
        tx.commit()?;
        Ok(Some(into))
    }

    /// The corner the user keeps the inventory in; top-right until they choose.
    pub fn inventory_corner(&self) -> Result<Corner, StoreError> {
        let v: Option<i64> = self.conn.query_row("SELECT value FROM settings WHERE key = 'inventory_corner'", [], |r| r.get(0)).optional()?;
        Ok(v.and_then(Corner::from_i64).unwrap_or_default())
    }

    pub fn set_inventory_corner(&self, corner: Corner) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES ('inventory_corner', ?1) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            [corner.as_i64()],
        )?;
        Ok(())
    }

    /// Ignore an item (or track it again). `false` when there's no such item.
    pub fn set_item_ignored(&self, id: ItemId, ignored: bool) -> Result<bool, StoreError> {
        Ok(self.conn.execute("UPDATE items SET ignored = ?2 WHERE id = ?1", params![id, ignored])? == 1)
    }

    /// The item chosen as the main HP or MP potion, if any.
    pub fn main_potion(&self, slot: PotionSlot) -> Result<Option<ItemId>, StoreError> {
        Ok(self.conn.query_row("SELECT value FROM settings WHERE key = ?1", [slot.setting_key()], |r| r.get(0)).optional()?)
    }

    pub fn set_main_potion(&self, slot: PotionSlot, item: Option<ItemId>) -> Result<(), StoreError> {
        match item {
            Some(id) => self.conn.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                params![slot.setting_key(), id],
            )?,
            None => self.conn.execute("DELETE FROM settings WHERE key = ?1", [slot.setting_key()])?,
        };
        Ok(())
    }

    /// Whether meso tracking is on (off until the user turns it on).
    pub fn loot_enabled(&self) -> Result<bool, StoreError> {
        let v: Option<i64> = self.conn.query_row("SELECT value FROM settings WHERE key = 'loot_enabled'", [], |r| r.get(0)).optional()?;
        Ok(v == Some(1))
    }

    pub fn set_loot_enabled(&self, on: bool) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES ('loot_enabled', ?1) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            [i64::from(on)],
        )?;
        Ok(())
    }

    /// Whether the chat translator is on (off until the user turns it on).
    pub fn translator_enabled(&self) -> Result<bool, StoreError> {
        let v: Option<i64> = self.conn.query_row("SELECT value FROM settings WHERE key = 'translator_enabled'", [], |r| r.get(0)).optional()?;
        Ok(v == Some(1))
    }

    pub fn set_translator_enabled(&self, on: bool) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES ('translator_enabled', ?1) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            [i64::from(on)],
        )?;
        Ok(())
    }

    /// Where the live view starts (unix seconds); 0 until the first reset.
    pub fn live_since(&self) -> Result<i64, StoreError> {
        Ok(self.conn.query_row("SELECT value FROM settings WHERE key = 'live_since'", [], |r| r.get(0)).optional()?.unwrap_or(0))
    }

    /// The map the live view is locked to, if any.
    pub fn locked_map(&self) -> Result<Option<MapId>, StoreError> {
        Ok(self.conn.query_row("SELECT value FROM settings WHERE key = 'locked_map'", [], |r| r.get(0)).optional()?)
    }

    pub fn set_locked_map(&self, map: Option<MapId>) -> Result<(), StoreError> {
        match map {
            Some(id) => self.conn.execute(
                "INSERT INTO settings (key, value) VALUES ('locked_map', ?1) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                [id],
            )?,
            None => self.conn.execute("DELETE FROM settings WHERE key = 'locked_map'", [])?,
        };
        Ok(())
    }

    /// Training time since the live view started (seconds), across maps: the popout's clock.
    /// Kept as a running total because the minutes it adds up are pruned after a few hours.
    pub fn grind_secs(&self) -> Result<u64, StoreError> {
        let v: i64 = self.conn.query_row("SELECT value FROM settings WHERE key = 'grind_secs'", [], |r| r.get(0)).optional()?.unwrap_or(0);
        u64::try_from(v).map_err(|_| StoreError::Range("grind_secs"))
    }

    /// EXP and mesos made since the user last reset the totals. Kept as running totals, like
    /// the grinding time, so they outlive the pruned minutes and survive a restart.
    pub fn totals(&self) -> Result<Totals, StoreError> {
        let counter = |key: &'static str| -> Result<u64, StoreError> {
            let v: i64 = self.conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional()?.unwrap_or(0);
            u64::try_from(v).map_err(|_| StoreError::Range(key))
        };
        Ok(Totals {
            exp: counter("total_exp")?,
            loot: Loot { meso: counter("total_meso")?, etc_value: counter("total_etc_value")?, potion_cost: counter("total_potion_cost")? },
            since: self.conn.query_row("SELECT value FROM settings WHERE key = 'totals_since'", [], |r| r.get(0)).optional()?,
        })
    }

    /// Start the totals again from nothing.
    pub fn reset_totals(&self) -> Result<(), StoreError> {
        self.conn.execute(
            "DELETE FROM settings WHERE key IN ('total_exp', 'total_meso', 'total_etc_value', 'total_potion_cost', 'totals_since')",
            [],
        )?;
        Ok(())
    }

    /// Start the live view at `t`, the grinding time back at 0.
    pub fn set_live_since(&self, t: i64) -> Result<(), StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO settings (key, value) VALUES ('live_since', ?1) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            [t],
        )?;
        tx.execute("INSERT INTO settings (key, value) VALUES ('grind_secs', 0) ON CONFLICT (key) DO UPDATE SET value = 0", [])?;
        tx.commit()?;
        Ok(())
    }

    /// Save a snapshot and its own copy of `rows`, in one transaction. Rows for the same
    /// minute add up: a session that ended mid-minute stored part of the minute in progress.
    pub fn create_snapshot(&self, name: &str, created_at: i64, map_name: Option<&str>, level: Option<u32>, rows: &[MinuteRow]) -> Result<i64, StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        let id = insert_snapshot(&tx, name, created_at, map_name, level, false, rows)?;
        tx.commit()?;
        Ok(id)
    }

    /// Sessions not yet filed into a grind, with their training.
    pub fn unfiled_sessions(&self) -> Result<Vec<SessionSpan>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.map_id, s.started_at, s.ended_at, COALESCE(SUM(m.training_secs), 0)
             FROM sessions s LEFT JOIN minutes m ON m.session_id = s.id
             WHERE s.filed = 0 GROUP BY s.id",
        )?;
        let rows = stmt
            .query_map([], |r| Ok(SessionSpan { id: r.get(0)?, map: r.get(1)?, started_at: r.get(2)?, ended_at: r.get(3)?, training_secs: col_u64(r, 4)? }))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// File a finished grind's sessions, saving it first as an automatic snapshot named `name`
    /// when given. The snapshot runs from the grind's first to its last minute of training, so
    /// the wait that ended the grind isn't in it, and is dated by its end. In one transaction;
    /// returns the snapshot's id.
    pub fn file_grind(&self, sessions: &[i64], name: Option<(&str, Option<&str>)>, level: Option<u32>) -> Result<Option<i64>, StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        let mut id = None;
        if let Some((name, map_name)) = name {
            let mut rows: BTreeMap<i64, MinuteRow> = BTreeMap::new();
            let mut stmt = tx.prepare(
                "SELECT minute_start, exp_gained, training_secs, meso, etc_value, potion_cost, loot_seen FROM minutes WHERE session_id = ?1",
            )?;
            for session in sessions {
                for r in stmt.query_map([session], minute_row)? {
                    let r = r?;
                    let row = rows.entry(r.minute_start).or_insert(MinuteRow { minute_start: r.minute_start, ..Default::default() });
                    row.exp_gained += r.exp_gained;
                    row.training_secs += r.training_secs;
                    row.loot += r.loot;
                    row.loot_seen |= r.loot_seen;
                }
            }
            drop(stmt);
            let rows: Vec<MinuteRow> = rows.into_values().collect();
            let trained = |r: &MinuteRow| r.training_secs > 0;
            if let (Some(first), Some(last)) = (rows.iter().position(trained), rows.iter().rposition(trained)) {
                let rows = &rows[first..=last];
                let ended = rows[rows.len() - 1].minute_start + 60;
                id = Some(insert_snapshot(&tx, name, ended, map_name, level, true, rows)?);
            }
        }
        for session in sessions {
            tx.execute("UPDATE sessions SET filed = 1 WHERE id = ?1", [session])?;
        }
        tx.commit()?;
        Ok(id)
    }

    /// Delete the automatic snapshots that ended before `before`; returns how many.
    pub fn delete_expired_snapshots(&self, before: i64) -> Result<usize, StoreError> {
        Ok(self.conn.execute("DELETE FROM snapshots WHERE auto = 1 AND created_at < ?1", [before])?)
    }

    /// Keep an automatic snapshot for good. `false` when no snapshot has that id.
    pub fn keep_snapshot(&self, id: i64) -> Result<bool, StoreError> {
        Ok(self.conn.execute("UPDATE snapshots SET auto = 0 WHERE id = ?1", [id])? == 1)
    }

    pub fn snapshots(&self) -> Result<Vec<SnapshotRecord>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.name, s.created_at, s.map_name, s.level, MIN(m.minute_start), MAX(m.minute_start) + 60,
                    COALESCE(SUM(m.exp_gained), 0), COALESCE(SUM(m.training_secs), 0),
                    COALESCE(SUM(m.meso), 0), COALESCE(SUM(m.etc_value), 0), COALESCE(SUM(m.potion_cost), 0), s.auto
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
                    loot: loot_at(r, 9)?,
                    auto: r.get(12)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn snapshot_minutes(&self, id: i64) -> Result<Vec<MinuteRow>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT minute_start, exp_gained, training_secs, meso, etc_value, potion_cost, loot_seen
             FROM snapshot_minutes WHERE snapshot_id = ?1 ORDER BY minute_start",
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

/// A snapshot and its own copy of `rows`, inside the caller's transaction.
fn insert_snapshot(tx: &Connection, name: &str, created_at: i64, map_name: Option<&str>, level: Option<u32>, auto: bool, rows: &[MinuteRow]) -> Result<i64, StoreError> {
    tx.execute(
        "INSERT INTO snapshots (name, created_at, map_name, level, auto) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![name, created_at, map_name, level, auto],
    )?;
    let id = tx.last_insert_rowid();
    for r in rows {
        tx.execute(
            "INSERT INTO snapshot_minutes (snapshot_id, minute_start, exp_gained, training_secs, meso, etc_value, potion_cost, loot_seen)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (snapshot_id, minute_start) DO UPDATE SET
               exp_gained = exp_gained + excluded.exp_gained,
               training_secs = training_secs + excluded.training_secs,
               meso = meso + excluded.meso,
               etc_value = etc_value + excluded.etc_value,
               potion_cost = potion_cost + excluded.potion_cost,
               loot_seen = MAX(loot_seen, excluded.loot_seen)",
            params![
                id,
                r.minute_start,
                to_i64(r.exp_gained, "exp_gained")?,
                i64::from(r.training_secs),
                to_i64(r.loot.meso, "meso")?,
                to_i64(r.loot.etc_value, "etc_value")?,
                to_i64(r.loot.potion_cost, "potion_cost")?,
                r.loot_seen,
            ],
        )?;
    }
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_8_and_9_start_maps_afresh_and_keep_sessions_and_minutes() {
        // A version-7 database with a map and a session on it.
        let conn = Connection::open_in_memory().unwrap();
        for sql in [SCHEMA, MIGRATION_2, MIGRATION_3, MIGRATION_4, MIGRATION_5, MIGRATION_6, "PRAGMA user_version = 7;"] {
            conn.execute_batch(sql).unwrap();
        }
        conn.execute_batch(
            "INSERT INTO maps (id, name, signature, first_seen) VALUES (1, 'Unno', x'01', 0);
             INSERT INTO sessions (id, map_id, started_at) VALUES (1, 1, 0);
             INSERT INTO minutes (minute_start, session_id, exp_gained, training_secs) VALUES (0, 1, 100, 60);",
        )
        .unwrap();
        let store = Store::init(conn).unwrap();
        let map_id: Option<i64> = store.conn.query_row("SELECT map_id FROM sessions", [], |r| r.get(0)).unwrap();
        assert_eq!(map_id, None);
        assert_eq!(store.minutes_between(0, 60, None).unwrap(), [MinuteRow { minute_start: 0, exp_gained: 100, training_secs: 60, loot_seen: true, ..Default::default() }]);
        assert!(store.maps().unwrap().is_empty());

        // A second look of a named map belongs to the same map.
        let look = |n| Signature { counts: vec![n], cut: false };
        let id = store.insert_map(Some(("Perion", NamedBy::List)), &look(1), 0).unwrap();
        store.add_signature(store.map_by_name("Perion").unwrap().unwrap(), &look(2)).unwrap();
        assert_eq!(store.maps().unwrap().iter().map(|m| m.id).collect::<Vec<_>>(), [id, id]);
    }

    #[test]
    fn a_locked_map_counts_only_its_own_sessions() {
        let store = Store::open_in_memory().unwrap();
        let look = Signature { counts: vec![1], cut: false };
        let (field, town) = (store.insert_map(Some(("Field", NamedBy::List)), &look, 0).unwrap(), store.insert_map(None, &look, 0).unwrap());
        for (map, exp) in [(field, 100), (town, 900)] {
            let session = store.start_session(Some(map), 0).unwrap();
            store.add_minute(0, session, exp, 60, Loot::default(), true).unwrap();
        }
        assert_eq!(store.minutes_between(0, 60, None).unwrap()[0].exp_gained, 1_000);
        assert_eq!(store.minutes_between(0, 60, Some(field)).unwrap()[0].exp_gained, 100);
    }

    #[test]
    fn prune_keeps_snapshots_recent_minutes_and_unfiled_sessions() {
        let store = Store::open_in_memory().unwrap();
        let map = store.insert_map(Some(("Map", NamedBy::List)), &Signature { counts: vec![1], cut: false }, 0).unwrap();
        let old = store.start_session(Some(map), 0).unwrap();
        store.add_minute(0, old, 100, 60, Loot::default(), true).unwrap();
        store.end_session(old, 60).unwrap();
        let open = store.start_session(None, 6_000).unwrap();
        store.add_minute(6_000, open, 50, 60, Loot::default(), true).unwrap();
        let loot = Loot { meso: 1, etc_value: 2, potion_cost: 3 };
        let row = MinuteRow { minute_start: 0, exp_gained: 100, training_secs: 60, loot, loot_seen: true };
        let snapshot = store.create_snapshot("Saved", 60, None, None, &[row, row]).unwrap();

        // Its grind not yet filed, the old session keeps its minute.
        store.prune(3_000).unwrap();
        assert_eq!(store.minutes_between(0, 10_000, None).unwrap().len(), 2);
        store.file_grind(&[old], None, None).unwrap();
        store.prune(3_000).unwrap();

        assert_eq!(store.minutes_between(0, 10_000, None).unwrap(), [MinuteRow { minute_start: 6_000, exp_gained: 50, training_secs: 60, loot_seen: true, ..Default::default() }]);
        let sessions: i64 = store.conn.query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0)).unwrap();
        assert_eq!(sessions, 1);
        assert_eq!(store.snapshot_minutes(snapshot).unwrap(), [MinuteRow { minute_start: 0, exp_gained: 200, training_secs: 120, loot: Loot { meso: 2, etc_value: 4, potion_cost: 6 }, loot_seen: true }]);
        // The grinding time keeps the pruned minute's training, until a reset.
        assert_eq!(store.grind_secs().unwrap(), 120);
        store.set_live_since(6_000).unwrap();
        assert_eq!(store.grind_secs().unwrap(), 0);
        // So do the totals, through a session reset too, until their own reset.
        assert_eq!(store.totals().unwrap(), Totals { exp: 150, loot: Loot::default(), since: Some(0) });
        store.reset_totals().unwrap();
        assert_eq!(store.totals().unwrap(), Totals::default());
    }

    #[test]
    fn a_grind_is_saved_without_its_idle_ends_and_expires_unless_the_users() {
        let store = Store::open_in_memory().unwrap();
        let session = store.start_session(None, 0).unwrap();
        // Loot before the first kill, two minutes of training, two idle minutes after.
        for (minute, training) in [(0, 0), (60, 60), (120, 30), (180, 0), (240, 0)] {
            store.add_minute(minute, session, 10, training, Loot { meso: 5, ..Loot::default() }, true).unwrap();
        }
        store.end_session(session, 300).unwrap();
        let id = store.file_grind(&[session], Some(("Field", Some("Field"))), Some(30)).unwrap();
        let saved = &store.snapshots().unwrap()[0];
        assert_eq!((Some(saved.id), saved.from, saved.to, saved.created_at, saved.training_secs, saved.auto), (id, Some(60), Some(180), 180, 90, true));
        assert!(store.unfiled_sessions().unwrap().is_empty());

        let mine = store.create_snapshot("Mine", 0, None, None, &[]).unwrap();
        assert_eq!(store.delete_expired_snapshots(10_000).unwrap(), 1);
        assert_eq!(store.snapshots().unwrap().iter().map(|s| s.id).collect::<Vec<_>>(), [mine]);
    }
}
