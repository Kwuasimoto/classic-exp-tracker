pub mod capture;
mod commands;
mod error;
pub mod geom;
mod pipeline;
pub mod rates;
pub mod read;
mod screen;
pub mod store;
pub mod tracker;

use std::sync::Arc;

use tauri::Manager;

/// Current UTC time in unix seconds.
pub fn now_unix() -> i64 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock is set after 1970")
        .as_secs();
    i64::try_from(secs).expect("unix seconds fit in i64")
}

/// Open the window centred on a monitor other than the primary one: the game
/// covers the primary monitor, and that is the monitor the app reads. With one
/// monitor the window stays where Windows puts it.
fn move_off_primary_monitor(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    let primary = window.primary_monitor()?;
    let other = window
        .available_monitors()?
        .into_iter()
        .find(|m| primary.as_ref().is_none_or(|p| p.position() != m.position()));
    let Some(other) = other else { return Ok(()) };
    let (origin, area, window_size) = (other.position(), other.size(), window.outer_size()?);
    let centre = |area: u32, window: u32| i32::try_from(area.saturating_sub(window) / 2).unwrap_or(0);
    window.set_position(tauri::PhysicalPosition::new(
        origin.x + centre(area.width, window_size.width),
        origin.y + centre(area.height, window_size.height),
    ))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Release builds abort on panic with no console, so a crash would leave no trace: log it first.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("panic: {info}");
        default_hook(info);
    }));
    tauri::Builder::default()
        // Registered first so a second launch exits before it opens the database or starts a capture.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            let shown = app.get_webview_window("main").map(|w| w.unminimize().and_then(|()| w.set_focus()));
            if let Some(Err(e)) = shown {
                log::warn!("could not bring the window forward for a second launch: {e}");
            }
        }))
        // Text log for debugging: %LOCALAPPDATA%\com.classicexp.tracker\logs, 5 files of 1 MB.
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .max_file_size(1_000_000)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(5))
                .timezone_strategy(tauri_plugin_log::TimezoneStrategy::UseLocal)
                .build(),
        )
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db = dir.join("tracker.db");
            log::info!("Classic EXP Tracker {} starting, database {}", app.package_info().version, db.display());
            let store = store::Store::open(&db)?;
            store.close_open_sessions()?;
            store.prune(rates::keep_from(now_unix()))?;
            let live_since = store.live_since()?;
            let locked_map = store.locked_map()?;
            let state = Arc::new(pipeline::AppState::new(store, live_since, locked_map));
            app.manage(Arc::clone(&state));
            pipeline::spawn(app.handle().clone(), state)?;
            // The window starts hidden (tauri.conf.json) so it doesn't flash on the game's monitor.
            let window = app.get_webview_window("main").ok_or("window \"main\" missing from tauri.conf.json")?;
            if let Err(e) = move_off_primary_monitor(&window) {
                log::warn!("could not move the window off the primary monitor: {e}");
            }
            window.show()?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_live_snapshot,
            commands::get_live_chart,
            commands::save_snapshot,
            commands::reset_session,
            commands::rename_map,
            commands::lock_map,
            commands::unlock_map,
            commands::get_snapshots,
            commands::get_snapshot_chart,
            commands::rename_snapshot,
            commands::delete_snapshot,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                pipeline::shutdown(&app.state::<Arc<pipeline::AppState>>());
            }
        });
}
