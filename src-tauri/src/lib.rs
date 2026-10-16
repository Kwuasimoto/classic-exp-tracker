pub mod capture;
mod commands;
mod error;
pub mod geom;
mod pipeline;
pub mod rates;
pub mod read;
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Text log for debugging: %LOCALAPPDATA%\com.mapleexp.tracker\logs, 5 files of 1 MB.
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
            log::info!("Maple EXP Tracker {} starting, database {}", app.package_info().version, db.display());
            let store = store::Store::open(&db)?;
            store.close_open_sessions()?;
            let state = Arc::new(pipeline::AppState::new(store));
            app.manage(Arc::clone(&state));
            pipeline::spawn(app.handle().clone(), state)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_live_snapshot,
            commands::get_live_chart,
            commands::get_map_summaries,
            commands::rename_map,
            commands::get_sessions,
            commands::get_session_chart,
            commands::get_calibration_frame,
            commands::preview_regions,
            commands::save_calibration,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                pipeline::shutdown(&app.state::<Arc<pipeline::AppState>>());
            }
        });
}
