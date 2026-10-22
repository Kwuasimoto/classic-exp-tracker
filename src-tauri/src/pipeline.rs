//! Glue between capture, readers, tracker and storage. Runs on its own thread; the
//! tracker and readers stay pure, all I/O happens here.

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use log::{error, info, warn};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::capture::{self, CaptureMsg, Control, Regions, Shared};
use crate::geom::{resolution_key, Crop};
use crate::read::exp::{centred, distance_to, from_art, to_art, Glyph, MAX_DISTANCE};
use crate::read::map_strip::Signature;
use crate::read::ocr::MapOcr;
use crate::read::{GlyphKind, LearnedGlyph};
use crate::rates::{self, MinuteRow};
use crate::screen::{FrameRead, Screen};
use crate::store::{Store, StoreError, UnknownGlyph};
use crate::tracker::{Effect, MapId, Sample, SessionKey, Tracker};

pub const TICK_EVENT: &str = "tracker-tick";
const MAX_BACKOFF: Duration = Duration::from_secs(30);
/// Unrecognised shapes kept for the Glyphs page per resolution; more waiting means
/// something other than the readouts is being caught, and the user should look.
const MAX_UNKNOWN_ENTRIES: usize = 50;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", content = "detail", rename_all = "camelCase")]
pub enum ReaderStatus {
    Ok,
    ExpUnreadable,
    /// The EXP readout has glyphs the app doesn't know yet (see the Glyphs page).
    ExpNeedsGlyphs,
    MapUnreadable,
    NotCalibrated,
    CaptureFailed(String),
    StorageFailed(String),
    NotifyFailed(String),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tick {
    pub status: ReaderStatus,
    pub map_name: Option<String>,
    pub exp: Option<u64>,
    pub pct_bp: Option<u32>,
    pub total_est: Option<u64>,
    /// Known once the readings fit exactly one level's table total.
    pub level: Option<u32>,
}

impl Default for Tick {
    fn default() -> Self {
        Self { status: ReaderStatus::NotCalibrated, map_name: None, exp: None, pct_bp: None, total_est: None, level: None }
    }
}

pub struct AppState {
    pub store: Mutex<Store>,
    pub capture: Arc<Shared>,
    /// Last full frame grabbed for the calibration screen (memory only).
    pub calibration_frame: Mutex<Option<Crop>>,
    pub live: RwLock<Tick>,
    /// The tracker's minute in progress, added to the live chart (see commands::get_live_chart).
    /// Written and read only while holding `store`, so a minute is never both stored and live.
    pub live_minute: RwLock<Option<MinuteRow>>,
    /// Where the live view starts (unix seconds); moved forward by a reset. Also in the database.
    pub live_since: RwLock<i64>,
    /// Set by commands, handled by the pipeline thread on its next loop.
    pub end_session_requested: AtomicBool,
    /// Learned or unknown glyphs changed in the database: reload them.
    pub glyphs_changed: AtomicBool,
    /// The resolution whose glyphs are in use (`1920x1080`); `None` before the first frame.
    pub glyph_resolution: RwLock<Option<String>>,
    shutdown: AtomicBool,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl AppState {
    pub fn new(store: Store, live_since: i64) -> Self {
        Self {
            store: Mutex::new(store),
            capture: Arc::new(Shared::default()),
            calibration_frame: Mutex::new(None),
            live: RwLock::new(Tick::default()),
            live_minute: RwLock::new(None),
            live_since: RwLock::new(live_since),
            end_session_requested: AtomicBool::new(false),
            glyphs_changed: AtomicBool::new(false),
            glyph_resolution: RwLock::new(None),
            shutdown: AtomicBool::new(false),
            thread: Mutex::new(None),
        }
    }
}

pub fn spawn(app: AppHandle, state: Arc<AppState>) -> std::io::Result<()> {
    let thread_state = Arc::clone(&state);
    let handle = std::thread::Builder::new()
        .name("exp-pipeline".into())
        .spawn(move || Pipeline::new(app, thread_state).run())?;
    *state.thread.lock().expect("thread slot lock poisoned") = Some(handle);
    Ok(())
}

/// Stop the pipeline, flushing the current minute and closing the open session.
pub fn shutdown(state: &AppState) {
    state.shutdown.store(true, Ordering::Release);
    let handle = state.thread.lock().expect("thread slot lock poisoned").take();
    if let Some(handle) = handle
        && handle.join().is_err() {
            error!("exp-pipeline thread panicked; the last minute may not be saved");
        }
}

struct Pipeline {
    app: AppHandle,
    state: Arc<AppState>,
    tx: SyncSender<CaptureMsg>,
    rx: Receiver<CaptureMsg>,
    control: Option<Control>,
    retry_at: Instant,
    backoff: Duration,
    ocr: Result<MapOcr, String>,
    screen: Screen,
    tracker: Tracker,
    current_map: Option<MapId>,
    /// The resolution the glyphs in use were learned at, and its unknown shapes.
    resolution: Option<String>,
    unknown: Vec<UnknownGlyph>,
    /// Whether the full-list warning was logged since the glyphs were last loaded.
    unknown_full_warned: bool,
    keys: HashMap<SessionKey, i64>,
    pending: Vec<Effect>,
    tried_resolution: Option<(u32, u32)>,
    capture_error: Option<String>,
    storage_error: Option<String>,
    notify_error: Option<String>,
}

impl Pipeline {
    fn new(app: AppHandle, state: Arc<AppState>) -> Self {
        let (tx, rx) = sync_channel(4);
        Self {
            app,
            state,
            tx,
            rx,
            control: None,
            retry_at: Instant::now(),
            backoff: Duration::from_secs(1),
            ocr: MapOcr::new().map_err(|e| e.to_string()),
            screen: Screen::new(),
            tracker: Tracker::new(),
            current_map: None,
            resolution: None,
            unknown: Vec::new(),
            unknown_full_warned: false,
            keys: HashMap::new(),
            pending: Vec::new(),
            tried_resolution: None,
            capture_error: None,
            storage_error: None,
            notify_error: None,
        }
    }

    fn run(mut self) {
        while !self.state.shutdown.load(Ordering::Acquire) {
            self.handle_requests();
            self.ensure_capture();
            match self.rx.recv_timeout(Duration::from_millis(500)) {
                Ok(CaptureMsg::Regions { t, width, height, exp, map, level }) => self.on_regions(t, width, height, exp, map, level),
                Ok(CaptureMsg::Uncalibrated { width, height }) => self.on_uncalibrated(width, height),
                Ok(CaptureMsg::Closed) => {}
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => unreachable!("the pipeline owns a sender"),
            }
        }
        let fx = self.tracker.finish();
        self.apply(fx);
        if let Some(control) = self.control.take()
            && let Err(e) = control.stop() {
                error!("stopping capture at shutdown: {e:?}");
            }
    }

    /// Requests from commands: end the session (a reset), reload changed glyphs.
    fn handle_requests(&mut self) {
        if self.state.end_session_requested.swap(false, Ordering::AcqRel) {
            info!("session reset by the user");
            let fx = self.tracker.end_session();
            self.apply(fx);
        }
        if self.state.glyphs_changed.swap(false, Ordering::AcqRel)
            && let Some(resolution) = self.resolution.clone()
            && let Err(e) = self.load_glyphs(resolution)
        {
            self.storage_failed("reloading glyphs", e);
        }
    }

    fn ensure_capture(&mut self) {
        if let Some(finished) = self.control.take_if(|c| c.is_finished()) {
            let reason = match finished.into_thread_handle().join() {
                Ok(Ok(())) => "capture ended".to_string(),
                Ok(Err(e)) => format!("{e:?}"),
                Err(_) => "capture thread panicked".to_string(),
            };
            warn!("monitor capture stopped: {reason}; retrying in {}s", self.backoff.as_secs());
            self.capture_error = Some(reason);
            self.schedule_retry();
            self.publish(ReaderStatus::NotCalibrated);
        }
        if self.control.is_some() || Instant::now() < self.retry_at {
            return;
        }
        match capture::start(self.tx.clone(), Arc::clone(&self.state.capture)) {
            Ok(control) => {
                info!("monitor capture started");
                self.control = Some(control);
            }
            Err(e) => {
                warn!("monitor capture failed to start: {e}; retrying in {}s", self.backoff.as_secs());
                self.capture_error = Some(e.to_string());
                self.schedule_retry();
                self.publish(ReaderStatus::NotCalibrated);
            }
        }
    }

    fn schedule_retry(&mut self) {
        self.retry_at = Instant::now() + self.backoff;
        self.backoff = (self.backoff * 2).min(MAX_BACKOFF);
    }

    fn frames_flowing(&mut self) {
        self.capture_error = None;
        self.backoff = Duration::from_secs(1);
    }

    fn on_uncalibrated(&mut self, width: u32, height: u32) {
        self.frames_flowing();
        self.use_glyphs_for(width, height);
        if self.tried_resolution != Some((width, height)) {
            let found = self.state.store.lock().expect("store lock poisoned").calibration(&resolution_key(width, height));
            match found {
                Ok(Some(calibration)) => {
                    info!("loaded calibration for {width}x{height}: {calibration:?}");
                    *self.state.capture.regions.write().expect("regions lock poisoned") =
                        Some(Regions { width, height, calibration });
                    self.tried_resolution = Some((width, height));
                }
                Ok(None) => {
                    info!("no calibration saved for {width}x{height}; waiting for the Calibrate tab");
                    self.tried_resolution = Some((width, height));
                }
                // Not marked as tried, so the next frame retries the lookup.
                Err(e) => self.storage_failed("loading calibration", e),
            }
        }
        self.publish(ReaderStatus::NotCalibrated);
    }

    fn on_regions(&mut self, t: i64, width: u32, height: u32, exp: Crop, map: Crop, level: Option<Crop>) {
        self.frames_flowing();
        self.use_glyphs_for(width, height);
        let FrameRead { exp, exp_unknown, map_readable, new_map, learned_exp, unknown } =
            self.screen.read(t, &exp, map, level, &self.tracker);
        self.save_glyphs(GlyphKind::Exp, &learned_exp);
        self.record_unknown(t, unknown);
        if let Some((signature, strips)) = &new_map {
            self.resolve_map(t, signature, strips);
        }
        let fx = self.tracker.on_sample(Sample { t, exp, map: self.current_map });
        let learned = self.screen.sync_level(&mut self.tracker, exp);
        self.save_glyphs(GlyphKind::Level, &learned);
        // Also publishes the minute in progress, before the tick: the UI reloads the chart on each tick.
        self.apply(fx);
        self.publish(match (exp, map_readable) {
            (None, _) if exp_unknown => ReaderStatus::ExpNeedsGlyphs,
            (None, _) => ReaderStatus::ExpUnreadable,
            (Some(_), false) => ReaderStatus::MapUnreadable,
            (Some(_), true) => ReaderStatus::Ok,
        });
    }

    /// Use this resolution's glyphs when they aren't the ones in use. On a storage error
    /// none are marked in use (nothing is saved under the wrong resolution) and the next
    /// frame retries.
    fn use_glyphs_for(&mut self, width: u32, height: u32) {
        let key = resolution_key(width, height);
        if self.resolution.as_deref() == Some(key.as_str()) {
            return;
        }
        if let Err(e) = self.load_glyphs(key) {
            self.resolution = None;
            *self.state.glyph_resolution.write().expect("glyph resolution lock poisoned") = None;
            self.storage_failed("loading learned glyphs", e);
        }
    }

    /// Use the glyphs learned at `resolution`, and drop the unknown shapes they now read
    /// (learned since they were recorded).
    fn load_glyphs(&mut self, resolution: String) -> Result<(), StoreError> {
        let store = self.state.store.lock().expect("store lock poisoned");
        let (mut exp, mut level) = (Vec::new(), Vec::new());
        for stored in store.glyphs(&resolution)? {
            match stored.kind {
                GlyphKind::Exp => exp.push(stored.glyph),
                GlyphKind::Level => level.push(stored.glyph),
            }
        }
        let (read_now, waiting): (Vec<UnknownGlyph>, Vec<UnknownGlyph>) =
            store.unknown_glyphs(&resolution)?.into_iter().partition(|u| {
                if u.dismissed {
                    return false;
                }
                let learned = match u.kind {
                    GlyphKind::Exp => &exp,
                    GlyphKind::Level => &level,
                };
                GlyphKind::recognises(&from_art(&u.rows), learned)
            });
        for entry in &read_now {
            store.delete_unknown_glyph(entry.id)?;
        }
        drop(store);
        info!(
            "glyphs for {resolution}: {} EXP and {} level learned, {} unknown waiting ({} now readable, removed)",
            exp.len(),
            level.len(),
            waiting.iter().filter(|u| !u.dismissed).count(),
            read_now.len()
        );
        self.screen.set_glyphs(exp, level);
        *self.state.glyph_resolution.write().expect("glyph resolution lock poisoned") = Some(resolution.clone());
        self.resolution = Some(resolution);
        self.unknown = waiting;
        self.unknown_full_warned = false;
        Ok(())
    }

    /// The glyphs are already in use. Once saved, reload: unknown shapes they read are done.
    fn save_glyphs(&mut self, kind: GlyphKind, glyphs: &[LearnedGlyph]) {
        let Some(resolution) = &self.resolution else { return };
        if glyphs.is_empty() {
            return;
        }
        let saved = {
            let store = self.state.store.lock().expect("store lock poisoned");
            glyphs.iter().try_fold(false, |any, glyph| store.add_glyph(resolution, kind, glyph).map(|added| any || added))
        };
        match saved {
            Ok(true) => self.state.glyphs_changed.store(true, Ordering::Release),
            // Every character already has its MAX_GLYPHS_PER_CHAR: in use until the next reload.
            Ok(false) => {}
            Err(e) => self.storage_failed("saving a learned glyph", e),
        }
    }

    /// Keep shapes no glyph matches for the Glyphs page, once each: one close to a
    /// waiting or dismissed entry is the same shape again.
    fn record_unknown(&mut self, t: i64, unknown: Vec<(GlyphKind, Glyph)>) {
        let Some(resolution) = self.resolution.clone() else { return };
        for (kind, glyph) in unknown {
            let waiting = self
                .unknown
                .iter()
                .any(|u| u.kind == kind && distance_to(&glyph, &u.rows).is_some_and(|d| d <= MAX_DISTANCE / 2.0));
            if waiting {
                continue;
            }
            if self.unknown.iter().filter(|u| !u.dismissed).count() >= MAX_UNKNOWN_ENTRIES {
                if !self.unknown_full_warned {
                    warn!("{MAX_UNKNOWN_ENTRIES} unrecognised glyphs already wait on the Glyphs page; not keeping more");
                    self.unknown_full_warned = true;
                }
                return;
            }
            let rows = to_art(&centred(&glyph));
            let added = self.state.store.lock().expect("store lock poisoned").add_unknown_glyph(&resolution, kind, &rows, t);
            match added {
                Ok(id) => {
                    info!("unrecognised {} glyph #{id} kept for the Glyphs page", kind.as_str());
                    self.unknown.push(UnknownGlyph { id, kind, rows, dismissed: false });
                }
                Err(e) => return self.storage_failed("keeping an unrecognised glyph", e),
            }
        }
    }

    fn storage_failed(&mut self, doing: &str, e: impl Display) {
        error!("database error while {doing}: {e}");
        self.storage_error = Some(e.to_string());
    }

    fn resolve_map(&mut self, t: i64, signature: &Signature, strips: &[Crop]) {
        let result = self.find_map(signature).and_then(|found| match found {
            Some(id) => {
                info!("map confirmed: known map #{id}");
                Ok(id)
            }
            None => {
                let name = self.label(strips);
                let id = self.state.store.lock().expect("store lock poisoned").insert_map(&name, signature, t)?;
                info!("map confirmed: new map #{id}, OCR label {name:?}");
                Ok(id)
            }
        });
        match result {
            Ok(id) => self.current_map = Some(id),
            Err(e) => {
                self.storage_failed("looking up or saving a map", e);
                // Re-confirm on upcoming frames so the lookup is retried.
                self.screen.reconfirm_map();
            }
        }
    }

    fn find_map(&self, signature: &Signature) -> Result<Option<MapId>, StoreError> {
        let maps = self.state.store.lock().expect("store lock poisoned").maps()?;
        Ok(maps.into_iter().find(|m| m.signature.matches(signature)).map(|m| m.id))
    }

    /// OCR label for a new map. Failures produce a visible name.
    fn label(&self, strips: &[Crop]) -> String {
        match &self.ocr {
            Ok(ocr) => match ocr.label(strips) {
                Ok(Some(name)) => name,
                Ok(None) => "Unnamed map".to_string(),
                Err(e) => format!("Unnamed map (OCR failed: {e})"),
            },
            Err(e) => format!("Unnamed map (OCR unavailable: {e})"),
        }
    }

    /// Apply effects in order; on a storage error keep the rest queued for the next call.
    /// Then publish the minute in progress, under the same store lock.
    fn apply(&mut self, fx: Vec<Effect>) {
        self.pending.extend(fx);
        let store = self.state.store.lock().expect("store lock poisoned");
        let mut done = 0;
        let mut failure = None;
        for effect in &self.pending {
            let result = match *effect {
                Effect::SessionStarted { key, map, t } => store.start_session(map, t).map(|id| {
                    match map {
                        Some(map) => info!("session #{id} started on map #{map}"),
                        None => info!("session #{id} started before a map was confirmed"),
                    }
                    self.keys.insert(key, id);
                }),
                Effect::SessionEnded { key, t } => {
                    let id = *self.keys.get(&key).expect("SessionStarted is applied before its SessionEnded");
                    // Nothing refers to a session after it ends (see tracker::Effect).
                    store.end_session(id, t).map(|()| {
                        self.keys.remove(&key);
                        info!("session #{id} ended");
                    })
                }
                Effect::Minute { minute_start, key, exp_gained, training_secs } => {
                    let id = *self.keys.get(&key).expect("SessionStarted is applied before its minutes");
                    store
                        .add_minute(minute_start, id, exp_gained, training_secs)
                        .map(|()| info!("session #{id} minute saved: +{exp_gained} EXP, {training_secs}s training"))
                }
            };
            if let Err(e) = result {
                failure = Some(e);
                break;
            }
            done += 1;
        }
        *self.state.live_minute.write().expect("live minute lock poisoned") = self.tracker.current_minute();
        // Only the live chart reads stored minutes: delete those it can no longer show.
        let saved_minute = self.pending[..done].iter().any(|e| matches!(e, Effect::Minute { .. }));
        let pruned = if saved_minute { store.prune(rates::keep_from(crate::now_unix())) } else { Ok(()) };
        drop(store);
        if let Some(e) = failure {
            self.storage_failed("saving tracker results (kept queued for retry)", e);
        }
        self.pending.drain(..done);
        if self.pending.is_empty() {
            self.storage_error = None;
        }
        if let Err(e) = pruned {
            self.storage_failed("deleting live data the chart no longer shows", e);
        }
    }

    fn publish(&mut self, reader: ReaderStatus) {
        let found = self.current_map.map(|id| self.state.store.lock().expect("store lock poisoned").map_name(id));
        let map_name = match found {
            Some(Ok(name)) => name,
            Some(Err(e)) => {
                self.storage_failed("loading the map name", e);
                None
            }
            None => None,
        };
        let status = if let Some(e) = &self.capture_error {
            ReaderStatus::CaptureFailed(e.clone())
        } else if let Some(e) = &self.storage_error {
            ReaderStatus::StorageFailed(e.clone())
        } else if let Some(e) = &self.notify_error {
            ReaderStatus::NotifyFailed(e.clone())
        } else {
            reader
        };
        let baseline = self.tracker.baseline();
        let tick = Tick {
            status,
            map_name,
            exp: baseline.map(|r| r.exp),
            pct_bp: baseline.map(|r| r.pct_bp),
            total_est: self.tracker.total_estimate(),
            level: self.tracker.level(),
        };
        *self.state.live.write().expect("live lock poisoned") = tick.clone();
        match self.app.emit(TICK_EVENT, &tick) {
            Ok(()) => self.notify_error = None,
            Err(e) => {
                warn!("sending {TICK_EVENT} to the UI failed: {e}");
                self.notify_error = Some(e.to_string());
            }
        }
    }
}
