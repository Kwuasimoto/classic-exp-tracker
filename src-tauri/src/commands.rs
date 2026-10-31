//! Tauri commands. Blocking ones run off the main thread via `#[tauri::command(async)]`.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use tauri::{Manager, State};

/// The popout window's label (see `set_popout`).
pub const POPOUT: &str = "popout";

use crate::error::AppError;
use crate::now_unix;
use crate::pipeline::{AppState, Tick};
use crate::rates::{self, Bar, MinuteRow, Totals};
use crate::geom::{boxes_for, Corner};
use crate::items::{self, ItemKind, PotionSlot};
use crate::store::{NamedBy, SnapshotRecord};

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
    let locked = *state.locked_map.read().expect("locked map lock poisoned");
    let store = state.store.lock().expect("store lock poisoned");
    let mut rows = store.minutes_between(from.max(since), to, locked)?;
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
    // Locked, the snapshot is the locked map's, wherever the player is now.
    let map_name = if tick.locked_map.is_some() { tick.locked_name } else { tick.map_name };
    let id = if rows.is_empty() { None } else { Some(store.create_snapshot(name, now, map_name.as_deref(), tick.level, &rows)?) };
    log::info!("snapshot {id:?} saved with {} minute(s)", rows.len());
    Ok(id)
}

/// End the current session and start the live view afresh from this minute (the
/// minute in progress stays in it). Saves nothing; older live data is deleted once
/// the chart no longer needs it.
#[tauri::command(async)]
pub fn reset_session(state: AppStateRef<'_>) -> Result<(), AppError> {
    let since = now_unix().div_euclid(60) * 60;
    let store = state.store.lock().expect("store lock poisoned");
    store.set_live_since(since)?;
    state.grind_secs.store(0, Ordering::Release);
    drop(store);
    *state.live_since.write().expect("live since lock poisoned") = since;
    state.end_session_requested.store(true, Ordering::Release);
    log::info!("reset: live view starts at {since}");
    Ok(())
}

/// Start the totals (EXP and mesos made on every map) again from nothing. The minute in
/// progress, not yet stored, still adds to them when it ends.
#[tauri::command(async)]
pub fn reset_totals(state: AppStateRef<'_>) -> Result<(), AppError> {
    let store = state.store.lock().expect("store lock poisoned");
    store.reset_totals()?;
    *state.totals.write().expect("totals lock poisoned") = Totals::default();
    log::info!("totals reset");
    Ok(())
}

/// The user names a map (one not on the map list, or one named wrongly). OCR never
/// renames it after this.
#[tauri::command(async)]
pub fn rename_map(state: AppStateRef<'_>, map_id: i64, name: String) -> Result<(), AppError> {
    let name = valid_name(&name)?;
    if state.store.lock().expect("store lock poisoned").set_map_name(map_id, name, NamedBy::User)? {
        log::info!("map #{map_id} named {name:?} by the user");
        Ok(())
    } else {
        Err(AppError::NotFound(format!("map {map_id}")))
    }
}

/// Count only `map_id` in the live view (its chart, rates and snapshots) until unlocked.
#[tauri::command(async)]
pub fn lock_map(state: AppStateRef<'_>, map_id: i64) -> Result<(), AppError> {
    let store = state.store.lock().expect("store lock poisoned");
    if !store.has_map(map_id)? {
        return Err(AppError::NotFound(format!("map {map_id}")));
    }
    store.set_locked_map(Some(map_id))?;
    *state.locked_map.write().expect("locked map lock poisoned") = Some(map_id);
    log::info!("map locked: the live view counts only map #{map_id}");
    Ok(())
}

#[tauri::command(async)]
pub fn unlock_map(state: AppStateRef<'_>) -> Result<(), AppError> {
    state.store.lock().expect("store lock poisoned").set_locked_map(None)?;
    *state.locked_map.write().expect("locked map lock poisoned") = None;
    log::info!("map unlocked: the live view counts every map");
    Ok(())
}

/// Turn meso tracking on or off. Off, the inventory and quickslot aren't even cropped.
#[tauri::command(async)]
pub fn set_loot_enabled(state: AppStateRef<'_>, on: bool) -> Result<(), AppError> {
    state.store.lock().expect("store lock poisoned").set_loot_enabled(on)?;
    state.capture.loot_enabled.store(on, Ordering::Release);
    log::info!("meso tracking {}", if on { "on" } else { "off" });
    Ok(())
}

/// Open or close the popout: the rates in a small window pinned above the game. It is left
/// out of screen capture (WDA_EXCLUDEFROMCAPTURE), so it never hides what the reader reads,
/// and opens hidden and unfocused: the popout places itself, then shows (src/popout).
// Async: creating a window from a synchronous command deadlocks on Windows (tauri docs).
#[tauri::command(async)]
pub fn set_popout(app: tauri::AppHandle, open: bool) -> Result<(), AppError> {
    let window_error = |e: tauri::Error| AppError::Window(e.to_string());
    // Tauri only registers the label once the window exists, so two calls at once (React's
    // double effect in dev, a double-click) would both see no popout and build two.
    static OPENING: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one_at_a_time = OPENING.lock().expect("popout lock poisoned");
    match (open, app.get_webview_window(POPOUT)) {
        (true, None) => {
            tauri::WebviewWindowBuilder::new(&app, POPOUT, tauri::WebviewUrl::App("popout.html".into()))
                .title("Classic EXP Tracker rates")
                .inner_size(230.0, 96.0)
                .resizable(false)
                .decorations(false)
                .transparent(true)
                .shadow(false)
                .always_on_top(true)
                .content_protected(true)
                .skip_taskbar(true)
                .focused(false)
                .visible(false)
                .build()
                .map_err(window_error)?;
        }
        (false, Some(window)) => window.close().map_err(window_error)?,
        _ => {}
    }
    log::info!("popout {}", if open { "opened" } else { "closed" });
    Ok(())
}

/// The corner the user keeps the inventory in. The inventory box moves there now.
#[tauri::command(async)]
pub fn set_inventory_corner(state: AppStateRef<'_>, corner: Corner) -> Result<(), AppError> {
    state.store.lock().expect("store lock poisoned").set_inventory_corner(corner)?;
    *state.capture.inventory_corner.write().expect("corner lock poisoned") = corner;
    let mut regions = state.capture.regions.write().expect("regions lock poisoned");
    if let Some(r) = regions.as_mut()
        && let Some(boxes) = boxes_for(r.width, r.height, corner)
    {
        r.boxes = boxes;
    }
    log::info!("inventory corner set to {corner:?}");
    Ok(())
}

/// An item the app has seen, for the Items panel.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemView {
    id: i64,
    kind: ItemKind,
    name: Option<String>,
    ignored: bool,
    /// From the item list; `None` while unnamed.
    price: Option<u64>,
    /// The patch it was first recognised by: RGB pixels, read::inventory::PATCH wide, row by row.
    patch: Vec<u8>,
}

/// Every item seen, unnamed ones first, each with the first patch it was seen by.
#[tauri::command(async)]
pub fn get_items(state: AppStateRef<'_>) -> Result<Vec<ItemView>, AppError> {
    let patches = state.store.lock().expect("store lock poisoned").item_patches()?;
    let mut items: Vec<ItemView> = Vec::new();
    for p in patches {
        if items.iter().any(|i| i.id == p.item_id) {
            continue;
        }
        let price = p.name.as_deref().and_then(|n| items::find(p.kind, n)).map(|i| i.price);
        items.push(ItemView { id: p.item_id, kind: p.kind, name: p.name, ignored: p.ignored, price, patch: p.patch });
    }
    items.sort_by_key(|i| (i.name.is_some(), i.name.clone()));
    Ok(items)
}

/// The item list's names of `kind`, for naming an item.
#[tauri::command]
pub fn item_names(kind: ItemKind) -> Vec<&'static str> {
    items::listed().iter().filter(|i| i.kind == kind).map(|i| i.name).collect()
}

/// The user names an item from the item list; from now on it counts at the list's price.
#[tauri::command(async)]
pub fn name_item(state: AppStateRef<'_>, item_id: i64, name: String) -> Result<(), AppError> {
    let store = state.store.lock().expect("store lock poisoned");
    let kind = store
        .item_patches()?
        .into_iter()
        .find(|p| p.item_id == item_id)
        .map(|p| p.kind)
        .ok_or_else(|| AppError::NotFound(format!("item {item_id}")))?;
    let listed = items::find(kind, name.trim()).ok_or_else(|| AppError::Invalid(format!("{name:?} isn't a {} item on the item list", kind.as_str())))?;
    let into = store.name_item(item_id, listed.name)?.ok_or_else(|| AppError::NotFound(format!("item {item_id}")))?;
    state.items_changed.store(true, Ordering::Release);
    log::info!("item #{item_id} named {:?} by the user (item #{into}, {} mesos)", listed.name, listed.price);
    Ok(())
}

/// Choose the main HP or MP potion (`None` clears it): only those two potions' use costs
/// mesos. The item must be a Use item the app has seen.
#[tauri::command(async)]
pub fn set_main_potion(state: AppStateRef<'_>, slot: PotionSlot, item_id: Option<i64>) -> Result<(), AppError> {
    let store = state.store.lock().expect("store lock poisoned");
    if let Some(id) = item_id
        && !store.item_patches()?.iter().any(|p| p.item_id == id && p.kind == ItemKind::Use)
    {
        return Err(AppError::NotFound(format!("Use item {id}")));
    }
    store.set_main_potion(slot, item_id)?;
    state.items_changed.store(true, Ordering::Release);
    log::info!("main {slot:?} potion set to {item_id:?}");
    Ok(())
}

/// Stop tracking an item (or track it again): an ignored item is recognised but never
/// counted, and isn't asked to be named.
#[tauri::command(async)]
pub fn ignore_item(state: AppStateRef<'_>, item_id: i64, ignored: bool) -> Result<(), AppError> {
    if !state.store.lock().expect("store lock poisoned").set_item_ignored(item_id, ignored)? {
        return Err(AppError::NotFound(format!("item {item_id}")));
    }
    state.items_changed.store(true, Ordering::Release);
    log::info!("item #{item_id} {}", if ignored { "ignored" } else { "tracked again" });
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

/// Keep an automatic snapshot for good (grinds::KEEP_AUTO_SECS no longer applies).
#[tauri::command(async)]
pub fn keep_snapshot(state: AppStateRef<'_>, snapshot_id: i64) -> Result<(), AppError> {
    if state.store.lock().expect("store lock poisoned").keep_snapshot(snapshot_id)? {
        log::info!("kept snapshot {snapshot_id}");
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
