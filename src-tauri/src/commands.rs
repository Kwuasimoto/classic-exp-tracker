//! Tauri commands. Blocking ones run off the main thread via `#[tauri::command(async)]`.

use std::io::Cursor;
use std::sync::atomic::Ordering;
use std::sync::mpsc::sync_channel;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use serde::Serialize;
use tauri::State;

use crate::capture::Regions;
use crate::error::AppError;
use crate::geom::{resolution_key, Calibration, Crop, Rect};
use crate::now_unix;
use crate::pipeline::{AppState, Tick};
use crate::rates::{self, Bar};
use crate::read::exp::{read_exp, ExpReading};
use crate::read::map_strip::read_strip;
use crate::read::level;
use crate::read::ocr::MapOcr;
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
pub struct CalibrationFrame {
    width: u32,
    height: u32,
    png_data_url: String,
    current: Option<Calibration>,
}

#[tauri::command(async)]
pub fn get_calibration_frame(state: AppStateRef<'_>) -> Result<CalibrationFrame, AppError> {
    let (tx, rx) = sync_channel::<Crop>(1);
    *state.capture.full_frame_request.lock().expect("frame request lock poisoned") = Some(tx);
    let frame = rx
        .recv_timeout(Duration::from_secs(3))
        .map_err(|_| AppError::Capture("no frame from screen capture within 3 s".into()))?;
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(frame.to_image())
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| AppError::Capture(format!("encoding preview: {e}")))?;
    let current = state.store.lock().expect("store lock poisoned").calibration(&resolution_key(frame.width, frame.height))?;
    let result = CalibrationFrame {
        width: frame.width,
        height: frame.height,
        png_data_url: format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(&png)),
        current,
    };
    *state.calibration_frame.lock().expect("calibration frame lock poisoned") = Some(frame);
    Ok(result)
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Preview<T> {
    Ok { value: T },
    Failed { reason: String },
}

#[derive(Serialize)]
pub struct RegionPreview {
    exp: Preview<ExpReading>,
    map: Preview<String>,
    /// `None` when no level box is drawn.
    level: Option<Preview<u32>>,
}

fn grabbed_frame(state: &AppState) -> Result<Crop, AppError> {
    state
        .calibration_frame
        .lock()
        .expect("calibration frame lock poisoned")
        .clone()
        .ok_or_else(|| AppError::Invalid("grab a frame first".into()))
}

#[tauri::command(async)]
pub fn preview_regions(state: AppStateRef<'_>, exp: Rect, map: Rect, level: Option<Rect>) -> Result<RegionPreview, AppError> {
    let frame = grabbed_frame(&state)?;
    let outside = || "box is outside the frame".to_string();
    let exp = match frame.sub(exp).ok_or_else(outside).and_then(|c| read_exp(&c).map_err(|e| e.to_string())) {
        Ok(value) => Preview::Ok { value },
        Err(reason) => Preview::Failed { reason },
    };
    let map = match frame.sub(map).ok_or_else(outside).and_then(|c| {
        read_strip(&c).map_err(|e| e.to_string())?;
        MapOcr::new()
            .and_then(|ocr| ocr.label(std::slice::from_ref(&c)))
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "map strip found, but OCR read no text".to_string())
    }) {
        Ok(value) => Preview::Ok { value },
        Err(reason) => Preview::Failed { reason },
    };
    let learned = state.store.lock().expect("store lock poisoned").level_glyphs()?;
    let level_preview = level.map(|rect| {
        match frame.sub(rect).ok_or_else(outside).and_then(|c| {
            let glyphs = level::segment(&c).map_err(|e| e.to_string())?;
            level::read_level(&glyphs, &learned).ok_or_else(|| {
                format!("{} digit(s) found, not all learned yet; the app learns them while you play", glyphs.len())
            })
        }) {
            Ok(value) => Preview::Ok { value },
            Err(reason) => Preview::Failed { reason },
        }
    });
    Ok(RegionPreview { exp, map, level: level_preview })
}

#[tauri::command(async)]
pub fn save_calibration(state: AppStateRef<'_>, exp: Rect, map: Rect, level: Option<Rect>) -> Result<(), AppError> {
    let frame = grabbed_frame(&state)?;
    let (width, height) = (frame.width, frame.height);
    if !exp.fits_in(width, height) || !map.fits_in(width, height) || level.is_some_and(|l| !l.fits_in(width, height)) {
        return Err(AppError::Invalid(format!("boxes must lie inside the {width}x{height} frame")));
    }
    let calibration = Calibration { exp, map, level };
    state.store.lock().expect("store lock poisoned").save_calibration(&resolution_key(width, height), &calibration)?;
    log::info!("calibration saved for {width}x{height}: {calibration:?}");
    *state.capture.regions.write().expect("regions lock poisoned") = Some(Regions { width, height, calibration });
    Ok(())
}

/// Forget the learned level digits; the pipeline relearns them while you play.
#[tauri::command(async)]
pub fn forget_level_digits(state: AppStateRef<'_>) -> Result<(), AppError> {
    let removed = state.store.lock().expect("store lock poisoned").clear_level_glyphs()?;
    state.forget_digits_requested.store(true, Ordering::Release);
    log::info!("forgot {removed} learned level digit(s)");
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
