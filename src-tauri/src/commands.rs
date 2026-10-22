//! Tauri commands. Blocking ones run off the main thread via `#[tauri::command(async)]`.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use serde::Serialize;
use tauri::{State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

use crate::error::AppError;
use crate::glyph_library::{self, Library, MAX_FILE_BYTES};
use crate::now_unix;
use crate::pipeline::{AppState, Tick};
use crate::rates::{self, Bar};
use crate::read::{GlyphKind, LearnedGlyph};
use crate::store::SnapshotRecord;

type AppStateRef<'a> = State<'a, Arc<AppState>>;

const MAX_NAME_LEN: usize = 80;

/// A snapshot name: trimmed, 1–MAX_NAME_LEN characters.
fn valid_name(name: &str) -> Result<&str, AppError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME_LEN {
        return Err(AppError::Invalid(format!("name must be 1–{MAX_NAME_LEN} characters")));
    }
    Ok(name)
}

/// The live chart windows: 2H and 3H.
fn valid_window(window_minutes: u32) -> Result<(), AppError> {
    if matches!(window_minutes, 120 | rates::MAX_WINDOW_MINUTES) {
        Ok(())
    } else {
        Err(AppError::Invalid(format!("window must be 120 or 180 minutes, got {window_minutes}")))
    }
}

#[tauri::command]
pub fn get_live_snapshot(state: AppStateRef<'_>) -> Tick {
    state.live.read().expect("live lock poisoned").clone()
}

#[tauri::command(async)]
pub fn get_live_chart(state: AppStateRef<'_>, window_minutes: u32) -> Result<Vec<Bar>, AppError> {
    valid_window(window_minutes)?;
    let (first, last) = rates::live_range(now_unix(), window_minutes);
    // The live view starts at the last reset.
    let since = *state.live_since.read().expect("live since lock poisoned");
    let store = state.store.lock().expect("store lock poisoned");
    let mut rows = store.minutes_between(rates::rows_from(first).max(since), last + rates::BUCKET_SECS)?;
    // The minute in progress isn't stored until it ends; count it so the rate is live. Read under
    // the store lock: the pipeline swaps it for the stored row under the same lock.
    rows.extend(*state.live_minute.read().expect("live minute lock poisoned"));
    drop(store);
    Ok(rates::bars_range(&rows, first, last))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearnedEntry {
    id: i64,
    kind: GlyphKind,
    ch: char,
    art: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnknownEntry {
    id: i64,
    kind: GlyphKind,
    art: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Glyphs {
    /// The resolution shown; `None` until the first frame arrives.
    resolution: Option<String>,
    /// The resolution whose glyphs are reading the screen now.
    in_use: Option<String>,
    /// Every resolution with a library, smallest first, and the one in use.
    resolutions: Vec<String>,
    learned: Vec<LearnedEntry>,
    unknown: Vec<UnknownEntry>,
    /// Level digits with no glyph yet, e.g. "479".
    missing_level: String,
}

/// `<width>x<height>`, or an error naming the bad value.
fn valid_resolution(resolution: &str) -> Result<(u32, u32), AppError> {
    glyph_library::parse_resolution(resolution).ok_or_else(|| AppError::Invalid(format!("{resolution:?} is not a screen resolution")))
}

/// Smallest screen first: `1920x1080` before `2560x1440`.
fn by_size(resolutions: &mut [String]) {
    let size = |r: &String| r.split_once('x').map(|(w, h)| (w.parse::<u32>().unwrap_or(0), h.parse::<u32>().unwrap_or(0)));
    resolutions.sort_by_key(size);
}

/// The glyph library of `resolution` (the one in use when `None`), for the Glyphs page.
#[tauri::command(async)]
pub fn get_glyphs(state: AppStateRef<'_>, resolution: Option<String>) -> Result<Glyphs, AppError> {
    if let Some(r) = &resolution {
        valid_resolution(r)?;
    }
    let in_use = state.glyph_resolution.read().expect("glyph resolution lock poisoned").clone();
    let resolution = resolution.or_else(|| in_use.clone());
    let store = state.store.lock().expect("store lock poisoned");
    let mut resolutions = store.glyph_resolutions()?;
    resolutions.extend([in_use.clone(), resolution.clone()].into_iter().flatten());
    by_size(&mut resolutions);
    resolutions.dedup();
    let mut learned = Vec::new();
    let mut unknown = Vec::new();
    if let Some(resolution) = &resolution {
        learned.extend(store.glyphs(resolution)?.into_iter().map(|g| LearnedEntry { id: g.id, kind: g.kind, ch: g.glyph.ch, art: g.glyph.rows }));
        unknown.extend(
            store.unknown_glyphs(resolution)?.into_iter().filter(|u| !u.dismissed).map(|u| UnknownEntry { id: u.id, kind: u.kind, art: u.rows }),
        );
    }
    let missing_level = GlyphKind::Level.alphabet().chars().filter(|&c| !learned.iter().any(|g| g.ch == c)).collect();
    Ok(Glyphs { resolution, in_use, resolutions, learned, unknown, missing_level })
}

/// Save `resolution`'s learned level digits to a file the user picks. Returns its path, or `None` if the user cancelled.
#[tauri::command(async)]
pub fn export_glyphs(window: WebviewWindow, state: AppStateRef<'_>, resolution: String) -> Result<Option<String>, AppError> {
    valid_resolution(&resolution)?;
    let glyphs = state.store.lock().expect("store lock poisoned").glyphs(&resolution)?;
    if glyphs.is_empty() {
        return Err(AppError::Invalid(format!("nothing to export: {resolution} has no level digits learned")));
    }
    let count = glyphs.len();
    let json = serde_json::to_string_pretty(&Library::new(&resolution, glyphs.into_iter().map(|g| (g.kind, g.glyph))))
        .map_err(|e| AppError::File(format!("writing the library: {e}")))?;
    let picked = window
        .dialog()
        .file()
        .set_parent(&window)
        .set_title("Export config")
        .add_filter("Classic EXP Tracker config", &["json"])
        .set_file_name("classic-exp-tracker-config.json")
        .blocking_save_file();
    let Some(picked) = picked else { return Ok(None) };
    let path = picked.into_path().map_err(|e| AppError::File(format!("the chosen location isn't a file path: {e}")))?;
    std::fs::write(&path, json).map_err(|e| AppError::File(format!("saving {}: {e}", path.display())))?;
    log::info!("exported {resolution}'s {count} level digit(s) to {}", path.display());
    Ok(Some(path.display().to_string()))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Imported {
    resolution: String,
    /// Glyphs added; the rest were already in the library.
    added: usize,
    total: usize,
}

/// Import a config file the user picks: its level digits are added to its resolution.
/// `None` if the user cancelled.
#[tauri::command(async)]
pub fn import_glyphs(window: WebviewWindow, state: AppStateRef<'_>) -> Result<Option<Imported>, AppError> {
    let picked = window
        .dialog()
        .file()
        .set_parent(&window)
        .set_title("Import config")
        .add_filter("Classic EXP Tracker config", &["json"])
        .blocking_pick_file();
    let Some(picked) = picked else { return Ok(None) };
    let path = picked.into_path().map_err(|e| AppError::File(format!("the chosen file isn't a file path: {e}")))?;
    let size = std::fs::metadata(&path).map_err(|e| AppError::File(format!("reading {}: {e}", path.display())))?.len();
    if size > MAX_FILE_BYTES {
        return Err(AppError::Invalid(format!("{} is {size} bytes; a glyph library is far smaller", path.display())));
    }
    let json = std::fs::read_to_string(&path).map_err(|e| AppError::File(format!("reading {}: {e}", path.display())))?;
    let library = Library::parse(&json).map_err(AppError::Invalid)?;
    let had_boxes = library.has_boxes();
    // EXP glyphs in files from before 0.3.0 are skipped: EXP is read from the game's font.
    let (glyphs, exp_glyphs): (Vec<_>, Vec<_>) = library.glyphs.into_iter().partition(|g| g.kind == GlyphKind::Level);
    let glyphs: Vec<(GlyphKind, LearnedGlyph)> = glyphs.into_iter().map(|g| (g.kind, LearnedGlyph { ch: g.ch, rows: g.art })).collect();
    valid_resolution(&library.resolution)?;
    let added = state.store.lock().expect("store lock poisoned").import_glyphs(&library.resolution, &glyphs)?;
    if had_boxes {
        // Since 0.4.0 the boxes are placed automatically (geom::boxes_for).
        log::info!("calibration boxes in {} ignored: the app places its boxes itself", path.display());
    }
    log::info!(
        "imported {added} of {} level glyph(s) for {} from {} ({} EXP glyph(s) from an older version skipped)",
        glyphs.len(),
        library.resolution,
        path.display(),
        exp_glyphs.len()
    );
    state.glyphs_changed.store(true, Ordering::Release);
    Ok(Some(Imported { resolution: library.resolution, added, total: glyphs.len() }))
}

/// `typed` as one character of `kind`'s readout, or an error listing them.
fn readout_char(kind: GlyphKind, typed: &str) -> Result<char, AppError> {
    let alphabet = kind.alphabet();
    let mut chars = typed.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if alphabet.contains(c) => Ok(c),
        _ => Err(AppError::Invalid(format!("type one of {}", alphabet.chars().map(String::from).collect::<Vec<_>>().join(" ")))),
    }
}

/// The user names an unknown glyph: it's learned as `ch`.
#[tauri::command(async)]
pub fn assign_glyph(state: AppStateRef<'_>, id: i64, ch: String) -> Result<(), AppError> {
    let store = state.store.lock().expect("store lock poisoned");
    let entry = store.unknown_glyph(id)?.ok_or_else(|| AppError::NotFound(format!("unknown glyph {id}")))?;
    let ch = readout_char(entry.kind, &ch)?;
    if !store.assign_unknown_glyph(id, ch)? {
        return Err(AppError::NotFound(format!("unknown glyph {id}")));
    }
    drop(store);
    log::info!("unknown {} glyph #{id} named {ch:?} by the user", entry.kind.as_str());
    state.glyphs_changed.store(true, Ordering::Release);
    Ok(())
}

/// The user corrects a learned glyph's character.
#[tauri::command(async)]
pub fn relabel_glyph(state: AppStateRef<'_>, id: i64, ch: String) -> Result<(), AppError> {
    let store = state.store.lock().expect("store lock poisoned");
    let kind = store.glyph_kind(id)?.ok_or_else(|| AppError::NotFound(format!("glyph {id}")))?;
    let ch = readout_char(kind, &ch)?;
    store.relabel_glyph(id, ch)?;
    drop(store);
    log::info!("learned {} glyph #{id} changed to {ch:?} by the user", kind.as_str());
    state.glyphs_changed.store(true, Ordering::Release);
    Ok(())
}

/// Forget a learned glyph (named wrongly, or learned from something else).
#[tauri::command(async)]
pub fn forget_glyph(state: AppStateRef<'_>, id: i64) -> Result<(), AppError> {
    if !state.store.lock().expect("store lock poisoned").forget_glyph(id)? {
        return Err(AppError::NotFound(format!("glyph {id}")));
    }
    log::info!("learned glyph #{id} forgotten by the user");
    state.glyphs_changed.store(true, Ordering::Release);
    Ok(())
}

/// Hide an unknown glyph that isn't part of a readout (something over the box); the
/// shape isn't recorded again.
#[tauri::command(async)]
pub fn dismiss_glyph(state: AppStateRef<'_>, id: i64) -> Result<(), AppError> {
    if !state.store.lock().expect("store lock poisoned").dismiss_unknown_glyph(id)? {
        return Err(AppError::NotFound(format!("unknown glyph {id}")));
    }
    log::info!("unknown glyph #{id} dismissed by the user");
    state.glyphs_changed.store(true, Ordering::Release);
    Ok(())
}

/// Save what the live chart shows for `window_minutes` (from the last reset, with the
/// minute in progress) as a snapshot named `name`. Returns its id, or `None` when there
/// was nothing to save.
#[tauri::command(async)]
pub fn save_snapshot(state: AppStateRef<'_>, name: String, window_minutes: u32) -> Result<Option<i64>, AppError> {
    let name = valid_name(&name)?;
    valid_window(window_minutes)?;
    let now = now_unix();
    let (first, _) = rates::live_range(now, window_minutes);
    let since = *state.live_since.read().expect("live since lock poisoned");
    let tick = state.live.read().expect("live lock poisoned").clone();
    let store = state.store.lock().expect("store lock poisoned");
    let mut rows = store.minutes_between(first.max(since), now)?;
    // Read under the store lock, as in get_live_chart.
    rows.extend(*state.live_minute.read().expect("live minute lock poisoned"));
    let id = if rows.is_empty() { None } else { Some(store.create_snapshot(name, now, tick.map_name.as_deref(), tick.level, &rows)?) };
    log::info!("snapshot {id:?} saved with {} minute(s)", rows.len());
    Ok(id)
}

/// End the current session and start the live view afresh from this minute (the
/// minute in progress stays in it). Saves nothing; older live data is deleted once
/// the chart no longer needs it.
#[tauri::command(async)]
pub fn reset_session(state: AppStateRef<'_>) -> Result<(), AppError> {
    let since = now_unix().div_euclid(60) * 60;
    state.store.lock().expect("store lock poisoned").set_live_since(since)?;
    *state.live_since.write().expect("live since lock poisoned") = since;
    state.end_session_requested.store(true, Ordering::Release);
    log::info!("reset: live view starts at {since}");
    Ok(())
}

#[tauri::command(async)]
pub fn get_snapshots(state: AppStateRef<'_>) -> Result<Vec<SnapshotRecord>, AppError> {
    Ok(state.store.lock().expect("store lock poisoned").snapshots()?)
}

#[tauri::command(async)]
pub fn get_snapshot_chart(state: AppStateRef<'_>, snapshot_id: i64) -> Result<Vec<Bar>, AppError> {
    let rows = state.store.lock().expect("store lock poisoned").snapshot_minutes(snapshot_id)?;
    let (Some(first), Some(last)) = (rows.first(), rows.last()) else {
        return Ok(Vec::new());
    };
    Ok(rates::bars_range(&rows, rates::bucket_start(first.minute_start), rates::bucket_start(last.minute_start)))
}

#[tauri::command(async)]
pub fn rename_snapshot(state: AppStateRef<'_>, snapshot_id: i64, name: String) -> Result<(), AppError> {
    let name = valid_name(&name)?;
    if state.store.lock().expect("store lock poisoned").rename_snapshot(snapshot_id, name)? {
        Ok(())
    } else {
        Err(AppError::NotFound(format!("snapshot {snapshot_id}")))
    }
}

#[tauri::command(async)]
pub fn delete_snapshot(state: AppStateRef<'_>, snapshot_id: i64) -> Result<(), AppError> {
    if state.store.lock().expect("store lock poisoned").delete_snapshot(snapshot_id)? {
        log::info!("deleted snapshot {snapshot_id}");
        Ok(())
    } else {
        Err(AppError::NotFound(format!("snapshot {snapshot_id}")))
    }
}
