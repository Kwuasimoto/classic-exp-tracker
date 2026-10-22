//! Tauri commands. Blocking ones run off the main thread via `#[tauri::command(async)]`.

use std::io::Cursor;
use std::sync::atomic::Ordering;
use std::sync::mpsc::sync_channel;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use serde::Serialize;
use tauri::{State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

use crate::capture::Regions;
use crate::error::AppError;
use crate::geom::{resolution_key, Calibration, Crop, Rect};
use crate::glyph_library::{self, Library, MAX_FILE_BYTES};
use crate::now_unix;
use crate::pipeline::{AppState, Tick};
use crate::rates::{self, Bar};
use crate::read::exp::{read_exp, ExpReading};
use crate::read::glyphs::{BUILT_IN_RESOLUTION, REFERENCES};
use crate::read::level;
use crate::read::map_strip::read_strip;
use crate::read::ocr::MapOcr;
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
    let glyphs = state.store.lock().expect("store lock poisoned").glyphs(&resolution_key(frame.width, frame.height))?;
    let learned = |kind: GlyphKind| glyphs.iter().filter(|g| g.kind == kind).map(|g| g.glyph.clone()).collect::<Vec<_>>();
    let (learned_exp, learned_level) = (learned(GlyphKind::Exp), learned(GlyphKind::Level));
    let exp = match frame.sub(exp).ok_or_else(outside).and_then(|c| read_exp(&c, &learned_exp).map_err(|e| e.to_string())) {
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
    let level_preview = level.map(|rect| {
        match frame.sub(rect).ok_or_else(outside).and_then(|c| {
            let glyphs = level::segment(&c).map_err(|e| e.to_string())?;
            level::read_level(&glyphs, &learned_level).ok_or_else(|| {
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearnedEntry {
    /// `None` for a built-in glyph, which can't be forgotten.
    id: Option<i64>,
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
    /// Every resolution with a library, smallest first: built in, learned, or in use.
    resolutions: Vec<String>,
    learned: Vec<LearnedEntry>,
    unknown: Vec<UnknownEntry>,
    /// Characters of each readout with no glyph yet, e.g. "479".
    missing_exp: String,
    missing_level: String,
}

/// `<width>x<height>`, or an error naming the bad value.
fn valid_resolution(resolution: &str) -> Result<(), AppError> {
    if glyph_library::valid_resolution(resolution) {
        Ok(())
    } else {
        Err(AppError::Invalid(format!("{resolution:?} is not a screen resolution")))
    }
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
    resolutions.extend([Some(BUILT_IN_RESOLUTION.to_string()), in_use.clone(), resolution.clone()].into_iter().flatten());
    by_size(&mut resolutions);
    resolutions.dedup();
    let mut learned = Vec::new();
    let mut unknown = Vec::new();
    if let Some(resolution) = &resolution {
        if resolution == BUILT_IN_RESOLUTION {
            learned.extend(REFERENCES.iter().map(|r| LearnedEntry {
                id: None,
                kind: GlyphKind::Exp,
                ch: r.ch,
                art: r.rows.iter().map(|row| (*row).to_string()).collect(),
            }));
        }
        learned.extend(store.glyphs(resolution)?.into_iter().map(|g| LearnedEntry { id: Some(g.id), kind: g.kind, ch: g.glyph.ch, art: g.glyph.rows }));
        unknown.extend(
            store.unknown_glyphs(resolution)?.into_iter().filter(|u| !u.dismissed).map(|u| UnknownEntry { id: u.id, kind: u.kind, art: u.rows }),
        );
    }
    let missing = |kind: GlyphKind| kind.alphabet().chars().filter(|&c| !learned.iter().any(|g| g.kind == kind && g.ch == c)).collect();
    let (missing_exp, missing_level) = (missing(GlyphKind::Exp), missing(GlyphKind::Level));
    Ok(Glyphs { resolution, in_use, resolutions, learned, unknown, missing_exp, missing_level })
}

/// Save the glyphs learned at `resolution` to a file the user picks. Returns its path,
/// or `None` if the user cancelled. Built-in glyphs aren't included: every copy has them.
#[tauri::command(async)]
pub fn export_glyphs(window: WebviewWindow, state: AppStateRef<'_>, resolution: String) -> Result<Option<String>, AppError> {
    valid_resolution(&resolution)?;
    let glyphs = state.store.lock().expect("store lock poisoned").glyphs(&resolution)?;
    if glyphs.is_empty() {
        return Err(AppError::Invalid(format!("no glyphs have been learned at {resolution} yet")));
    }
    let count = glyphs.len();
    let json = serde_json::to_string_pretty(&Library::new(&resolution, glyphs.into_iter().map(|g| (g.kind, g.glyph))))
        .map_err(|e| AppError::File(format!("writing the library: {e}")))?;
    let picked = window
        .dialog()
        .file()
        .set_parent(&window)
        .set_title("Export glyph library")
        .add_filter("Glyph library", &["json"])
        .set_file_name(format!("glyphs-{resolution}.json"))
        .blocking_save_file();
    let Some(picked) = picked else { return Ok(None) };
    let path = picked.into_path().map_err(|e| AppError::File(format!("the chosen location isn't a file path: {e}")))?;
    std::fs::write(&path, json).map_err(|e| AppError::File(format!("saving {}: {e}", path.display())))?;
    log::info!("exported {count} glyph(s) learned at {resolution} to {}", path.display());
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

/// Add the glyphs from a library file the user picks to its resolution's library.
/// `None` if the user cancelled.
#[tauri::command(async)]
pub fn import_glyphs(window: WebviewWindow, state: AppStateRef<'_>) -> Result<Option<Imported>, AppError> {
    let picked = window
        .dialog()
        .file()
        .set_parent(&window)
        .set_title("Import glyph library")
        .add_filter("Glyph library", &["json"])
        .blocking_pick_file();
    let Some(picked) = picked else { return Ok(None) };
    let path = picked.into_path().map_err(|e| AppError::File(format!("the chosen file isn't a file path: {e}")))?;
    let size = std::fs::metadata(&path).map_err(|e| AppError::File(format!("reading {}: {e}", path.display())))?.len();
    if size > MAX_FILE_BYTES {
        return Err(AppError::Invalid(format!("{} is {size} bytes; a glyph library is far smaller", path.display())));
    }
    let json = std::fs::read_to_string(&path).map_err(|e| AppError::File(format!("reading {}: {e}", path.display())))?;
    let library = Library::parse(&json).map_err(AppError::Invalid)?;
    let glyphs: Vec<(GlyphKind, LearnedGlyph)> =
        library.glyphs.into_iter().map(|g| (g.kind, LearnedGlyph { ch: g.ch, rows: g.art })).collect();
    let added = state.store.lock().expect("store lock poisoned").import_glyphs(&library.resolution, &glyphs)?;
    log::info!("imported {added} of {} glyph(s) for {} from {}", glyphs.len(), library.resolution, path.display());
    state.glyphs_changed.store(true, Ordering::Release);
    Ok(Some(Imported { resolution: library.resolution, added, total: glyphs.len() }))
}

/// The user names an unknown glyph: it's learned as `ch`.
#[tauri::command(async)]
pub fn assign_glyph(state: AppStateRef<'_>, id: i64, ch: String) -> Result<(), AppError> {
    let store = state.store.lock().expect("store lock poisoned");
    let entry = store.unknown_glyph(id)?.ok_or_else(|| AppError::NotFound(format!("unknown glyph {id}")))?;
    let alphabet = entry.kind.alphabet();
    let mut chars = ch.chars();
    let ch = match (chars.next(), chars.next()) {
        (Some(c), None) if alphabet.contains(c) => c,
        _ => return Err(AppError::Invalid(format!("type one of {}", alphabet.chars().map(String::from).collect::<Vec<_>>().join(" ")))),
    };
    if !store.assign_unknown_glyph(id, ch)? {
        return Err(AppError::NotFound(format!("unknown glyph {id}")));
    }
    drop(store);
    log::info!("unknown {} glyph #{id} named {ch:?} by the user", entry.kind.as_str());
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
