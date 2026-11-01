//! Updates from the repository's GitHub releases (tauri.conf.json, plugins.updater): checked 10 s
//! after launch and then every hour, unless the user turned that off, and whenever the user asks.
//! A newer release is downloaded in the background and its signature checked against the public
//! key built into the app; it's installed only when the user chooses Restart, never on its own,
//! so a grind is never cut off.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::error::AppError;
use crate::pipeline::{self, AppState};

/// Sent to every window whenever the update status changes.
pub const UPDATE_EVENT: &str = "update-status";
const FIRST_CHECK_AFTER: Duration = Duration::from_secs(10);
const CHECK_EVERY: Duration = Duration::from_secs(60 * 60);

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    /// Checks every hour by itself (the user's setting).
    pub auto_check: bool,
    pub checking: bool,
    /// When the last check finished (unix seconds), whether it found anything or failed.
    pub checked_at: Option<i64>,
    /// A newer version, downloaded with its signature checked, waiting for Restart.
    pub ready: Option<String>,
    /// Why the last check failed (offline, GitHub unreachable, a bad signature); the next check
    /// that gets through clears it.
    pub error: Option<String>,
}

pub struct Updates {
    status: Mutex<UpdateStatus>,
    /// The downloaded update and its installer, for Restart.
    ready: Mutex<Option<(Update, Vec<u8>)>>,
    /// One check at a time: the hourly one and Check now don't download twice.
    checking: tokio::sync::Mutex<()>,
}

impl Updates {
    pub fn new(auto_check: bool) -> Self {
        Self {
            status: Mutex::new(UpdateStatus { auto_check, ..UpdateStatus::default() }),
            ready: Mutex::new(None),
            checking: tokio::sync::Mutex::new(()),
        }
    }

    pub fn status(&self) -> UpdateStatus {
        self.status.lock().expect("update status lock poisoned").clone()
    }

    fn change(&self, app: &AppHandle, f: impl FnOnce(&mut UpdateStatus)) {
        let status = {
            let mut status = self.status.lock().expect("update status lock poisoned");
            f(&mut status);
            status.clone()
        };
        if let Err(e) = app.emit(UPDATE_EVENT, status) {
            log::warn!("could not send the update status to the UI: {e}");
        }
    }

    pub fn set_auto_check(&self, app: &AppHandle, on: bool) {
        self.change(app, |s| s.auto_check = on);
    }
}

/// The hourly checks, for the app's lifetime.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK_AFTER).await;
        loop {
            if app.state::<Updates>().status().auto_check {
                check(&app).await;
            }
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

/// Looks for a newer release and downloads it; the outcome goes into the status (and the log).
pub async fn check(app: &AppHandle) {
    let updates = app.state::<Updates>();
    let _one = updates.checking.lock().await;
    updates.change(app, |s| s.checking = true);
    let found = find(app, &updates).await;
    if let Err(e) = &found {
        log::warn!("update check failed: {e}");
    }
    updates.change(app, |s| {
        s.checking = false;
        s.checked_at = Some(crate::now_unix());
        s.error = found.err().map(|e| e.to_string());
    });
}

async fn find(app: &AppHandle, updates: &Updates) -> Result<(), tauri_plugin_updater::Error> {
    // The Windows installer replaces the app by exiting it (std::process::exit) without the
    // usual shutdown: save the minute in progress first.
    let state = Arc::clone(app.state::<Arc<AppState>>().inner());
    let updater = app.updater_builder().on_before_exit(move || pipeline::shutdown(&state)).build()?;
    let Some(update) = updater.check().await? else {
        return Ok(());
    };
    if updates.status().ready.as_deref() == Some(update.version.as_str()) {
        return Ok(());
    }
    log::info!("update {} found (running {}); downloading", update.version, update.current_version);
    // Checks the signature: a download that doesn't match is an error, and never kept.
    let installer = update.download(|_, _| {}, || {}).await?;
    log::info!("update {} downloaded and its signature checked; installs on Restart", update.version);
    let version = update.version.clone();
    *updates.ready.lock().expect("update lock poisoned") = Some((update, installer));
    updates.change(app, |s| s.ready = Some(version));
    Ok(())
}

/// Installs the downloaded update. On Windows the installer takes over and the app exits; it
/// starts again once installed.
pub fn install(app: &AppHandle) -> Result<(), AppError> {
    let updates = app.state::<Updates>();
    let ready = updates.ready.lock().expect("update lock poisoned");
    let (update, installer) = ready.as_ref().ok_or_else(|| AppError::NotFound("a downloaded update".into()))?;
    log::info!("installing update {}", update.version);
    // Kept if installing fails, so Restart can try again.
    update.install(installer)?;
    app.restart()
}
