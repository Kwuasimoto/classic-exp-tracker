//! Tauri commands. Blocking ones run off the main thread via `#[tauri::command(async)]`.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use tauri::State;

use crate::error::AppError;
use crate::now_unix;
use crate::pipeline::{AppState, Tick};
use crate::rates::{self, Bar, MinuteRow};
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

/// One of the live chart's windows.
fn valid_window(window_minutes: u32) -> Result<(), AppError> {
    if rates::WINDOWS_MINUTES.contains(&window_minutes) {
        Ok(())
    } else {
        Err(AppError::Invalid(format!("window must be one of {:?} minutes, got {window_minutes}", rates::WINDOWS_MINUTES)))
    }
}

/// The stored minutes from `from` (but not before the last reset) to `to`, plus the
/// minute in progress, which isn't stored until it ends. The live minute is read under
/// the store lock: the pipeline swaps it for the stored row under the same lock.
fn live_rows(state: &AppState, from: i64, to: i64) -> Result<Vec<MinuteRow>, AppError> {
    let since = *state.live_since.read().expect("live since lock poisoned");
    let store = state.store.lock().expect("store lock poisoned");
    let mut rows = store.minutes_between(from.max(since), to)?;
    rows.extend(*state.live_minute.read().expect("live minute lock poisoned"));
    Ok(rows)
}

#[tauri::command]
pub fn get_live_snapshot(state: AppStateRef<'_>) -> Tick {
    state.live.read().expect("live lock poisoned").clone()
}

#[tauri::command(async)]
pub fn get_live_chart(state: AppStateRef<'_>, window_minutes: u32) -> Result<Vec<Bar>, AppError> {
    valid_window(window_minutes)?;
    let (first, last) = rates::live_range(now_unix(), window_minutes);
    let rows = live_rows(&state, rates::rows_from(first), last + rates::BUCKET_SECS)?;
    Ok(rates::bars_range(&rows, first, last))
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
    let tick = state.live.read().expect("live lock poisoned").clone();
    let rows = live_rows(&state, first, now)?;
    let store = state.store.lock().expect("store lock poisoned");
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
