//! Tauri commands. Blocking ones run off the main thread via `#[tauri::command(async)]`.

use std::io::Cursor;
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
use crate::store::SessionRecord;
use crate::tracker::MapId;

type AppStateRef<'a> = State<'a, Arc<AppState>>;

const MAX_MAP_NAME_LEN: usize = 80;

#[tauri::command]
pub fn get_live_snapshot(state: AppStateRef<'_>) -> Tick {
    state.live.read().expect("live lock poisoned").clone()
}

#[tauri::command(async)]
pub fn get_live_chart(state: AppStateRef<'_>, window_minutes: u32) -> Result<Vec<Bar>, AppError> {
    if !matches!(window_minutes, 120 | 180) {
        return Err(AppError::Invalid(format!("window must be 120 or 180 minutes, got {window_minutes}")));
    }
    let (first, last) = rates::live_range(now_unix(), window_minutes);
    let rows = state
        .store
        .lock()
        .expect("store lock poisoned")
        .minutes_between(rates::rows_from(first), last + rates::BUCKET_SECS)?;
    Ok(rates::bars_range(&rows, first, last))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MapSummary {
    map_id: MapId,
    name: String,
    sessions: u32,
    training_secs: u64,
    exp_gained: u64,
    avg_rate_per_hour: Option<f64>,
    best_hour_rate: Option<f64>,
}

#[tauri::command(async)]
pub fn get_map_summaries(state: AppStateRef<'_>) -> Result<Vec<MapSummary>, AppError> {
    let store = state.store.lock().expect("store lock poisoned");
    store
        .map_totals()?
        .into_iter()
        .map(|t| {
            let rows = store.map_minutes(t.map_id)?;
            Ok(MapSummary {
                map_id: t.map_id,
                name: t.name,
                sessions: t.sessions,
                training_secs: t.training_secs,
                exp_gained: t.exp_gained,
                avg_rate_per_hour: (t.training_secs > 0).then(|| t.exp_gained as f64 * 3600.0 / t.training_secs as f64),
                best_hour_rate: rates::best_hour_rate(&rows),
            })
        })
        .collect()
}

#[tauri::command(async)]
pub fn rename_map(state: AppStateRef<'_>, map_id: MapId, name: String) -> Result<(), AppError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_MAP_NAME_LEN {
        return Err(AppError::Invalid(format!("map name must be 1–{MAX_MAP_NAME_LEN} characters")));
    }
    if state.store.lock().expect("store lock poisoned").rename_map(map_id, name)? {
        Ok(())
    } else {
        Err(AppError::NotFound(format!("map {map_id}")))
    }
}

#[tauri::command(async)]
pub fn get_sessions(state: AppStateRef<'_>, map_id: Option<MapId>) -> Result<Vec<SessionRecord>, AppError> {
    Ok(state.store.lock().expect("store lock poisoned").sessions(map_id)?)
}

#[tauri::command(async)]
pub fn get_session_chart(state: AppStateRef<'_>, session_id: i64) -> Result<Vec<Bar>, AppError> {
    let rows = state.store.lock().expect("store lock poisoned").session_minutes(session_id)?;
    let (Some(first), Some(last)) = (rows.first(), rows.last()) else {
        return Ok(Vec::new());
    };
    Ok(rates::bars_range(&rows, rates::bucket_start(first.minute_start), rates::bucket_start(last.minute_start)))
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
