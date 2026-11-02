//! Updates from the repository's GitHub releases (tauri.conf.json, plugins.updater): checked 10 s
//! after launch and then every hour, unless the user turned that off, and whenever the user asks.
//! A check fetches only the release's latest.json. Nothing else is downloaded until the user clicks
//! Update: then the installer is downloaded, its signature checked against the public key built
//! into the app, and installed, in that order. Never on its own, so a grind is never cut off.
//!
//! latest.json's notes are the patch notes (patch-notes.json, put there by the Release workflow):
//! the user sees what changed in every version between theirs and the newest before updating.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
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
    /// A newer version on GitHub, not downloaded until the user clicks Update.
    pub available: Option<String>,
    /// What changed in each version after the running one, up to `available`, newest first;
    /// empty when its release has no patch notes (before 0.12.0).
    pub notes: Vec<VersionNotes>,
    /// Downloading and installing it (the user clicked Update).
    pub installing: bool,
    /// Why the last check failed (offline, GitHub unreachable, a bad signature); the next check
    /// that gets through clears it.
    pub error: Option<String>,
}

/// One version's patch notes; each section a list of short paragraphs.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionNotes {
    pub version: String,
    pub date: String,
    #[serde(default)]
    pub new: Vec<String>,
    #[serde(default)]
    pub convenience: Vec<String>,
    #[serde(default)]
    pub technical: Vec<String>,
    #[serde(default)]
    pub fixes: Vec<String>,
}

#[derive(Deserialize)]
struct PatchNotes {
    versions: Vec<VersionNotes>,
}

/// latest.json isn't covered by the update's signature: its notes are only shown, as plain
/// text, and these bound how much of them is.
const MAX_VERSIONS: usize = 15;
const MAX_ITEMS: usize = 30;
const MAX_ITEM_CHARS: usize = 600;

/// The notes of each version after `current`, up to and including `latest`, newest first.
/// Release notes that aren't patch notes (releases before 0.12.0), and entries that aren't
/// versions, give none.
pub fn notes_between(body: Option<&str>, current: &str, latest: &str) -> Vec<VersionNotes> {
    let (Ok(current), Ok(latest)) = (semver::Version::parse(current), semver::Version::parse(latest)) else { return Vec::new() };
    let Some(Ok(notes)) = body.map(serde_json::from_str::<PatchNotes>) else { return Vec::new() };
    let mut between: Vec<(semver::Version, VersionNotes)> = notes
        .versions
        .into_iter()
        .filter_map(|n| semver::Version::parse(&n.version).ok().map(|v| (v, n)))
        .filter(|(v, _)| *v > current && *v <= latest)
        .collect();
    between.sort_by(|a, b| b.0.cmp(&a.0));
    let trim = |items: Vec<String>| -> Vec<String> { items.into_iter().take(MAX_ITEMS).map(|i| i.chars().take(MAX_ITEM_CHARS).collect()).collect() };
    between
        .into_iter()
        .take(MAX_VERSIONS)
        .map(|(_, n)| VersionNotes {
            date: n.date.chars().take(20).collect(),
            new: trim(n.new),
            convenience: trim(n.convenience),
            technical: trim(n.technical),
            fixes: trim(n.fixes),
            version: n.version,
        })
        .collect()
}

pub struct Updates {
    status: Mutex<UpdateStatus>,
    /// The newer release the last check found, for Update.
    available: Mutex<Option<Update>>,
    /// One check or install at a time.
    busy: tokio::sync::Mutex<()>,
}

impl Updates {
    pub fn new(auto_check: bool) -> Self {
        Self {
            status: Mutex::new(UpdateStatus { auto_check, ..UpdateStatus::default() }),
            available: Mutex::new(None),
            busy: tokio::sync::Mutex::new(()),
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

/// Looks for a newer release; the outcome goes into the status (and the log).
pub async fn check(app: &AppHandle) {
    let updates = app.state::<Updates>();
    let _one = updates.busy.lock().await;
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
    let update = updater.check().await?;
    if let Some(update) = &update {
        log::info!("update {} available (running {})", update.version, update.current_version);
    }
    let version = update.as_ref().map(|u| u.version.clone());
    let notes = update.as_ref().map_or_else(Vec::new, |u| notes_between(u.body.as_deref(), &u.current_version, &u.version));
    // None clears one found earlier, if that release was taken down.
    *updates.available.lock().expect("update lock poisoned") = update;
    updates.change(app, |s| {
        s.available = version;
        s.notes = notes;
    });
    Ok(())
}

/// Downloads the newer release, checks its signature and installs it (the user clicked Update).
/// On Windows the installer takes over and the app exits; it starts again once installed.
pub async fn install(app: &AppHandle) -> Result<(), AppError> {
    let updates = app.state::<Updates>();
    let _one = updates.busy.lock().await;
    let update = updates.available.lock().expect("update lock poisoned").clone();
    let update = update.ok_or_else(|| AppError::NotFound("a newer version".into()))?;
    updates.change(app, |s| s.installing = true);
    log::info!("downloading update {}", update.version);
    // Checks the signature: a download that doesn't match is an error, and never installed.
    let installed = match update.download(|_, _| {}, || {}).await {
        Ok(installer) => {
            log::info!("update {} downloaded and its signature checked; installing", update.version);
            update.install(installer)
        }
        Err(e) => Err(e),
    };
    // Still available if it failed, so Update can try again.
    if let Err(e) = installed {
        updates.change(app, |s| s.installing = false);
        log::warn!("update {} failed: {e}", update.version);
        return Err(e.into());
    }
    app.restart()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_the_notes_between_the_running_version_and_the_newest() {
        let body = r#"{"versions":[
            {"version":"0.14.0","date":"2026-12-01","new":["c"]},
            {"version":"0.13.0","date":"2026-11-20","technical":["b"],"fixes":["b2"]},
            {"version":"0.12.0","date":"2026-11-02","new":["a"]}]}"#;
        let versions = |notes: Vec<VersionNotes>| notes.into_iter().map(|n| n.version).collect::<Vec<_>>();
        assert_eq!(versions(notes_between(Some(body), "0.12.0", "0.14.0")), ["0.14.0", "0.13.0"]);
        assert_eq!(versions(notes_between(Some(body), "0.11.4", "0.12.0")), ["0.12.0"]);
        let thirteen = &notes_between(Some(body), "0.12.0", "0.13.0")[0];
        assert_eq!((thirteen.technical.as_slice(), thirteen.fixes.as_slice(), thirteen.new.len()), (&["b".to_string()][..], &["b2".to_string()][..], 0));
        // Releases before patch notes had a plain sentence there.
        assert!(notes_between(Some("Release notes are added once the build is attested."), "0.11.3", "0.11.4").is_empty());
        assert!(notes_between(None, "0.12.0", "0.13.0").is_empty());
        // Bounded: a long item is cut.
        let long = format!(r#"{{"versions":[{{"version":"0.13.0","date":"x","new":["{}"]}}]}}"#, "a".repeat(5000));
        assert_eq!(notes_between(Some(&long), "0.12.0", "0.13.0")[0].new[0].len(), MAX_ITEM_CHARS);
    }
}
