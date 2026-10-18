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
use crate::read::level::LevelGlyph;
use crate::read::map_strip::Signature;
use crate::read::ocr::MapOcr;
use crate::rates::{self, MinuteRow};
use crate::screen::Screen;
use crate::store::{Store, StoreError};
use crate::tracker::{Effect, MapId, Sample, SessionKey, Tracker};

pub const TICK_EVENT: &str = "tracker-tick";
const MAX_BACKOFF: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", content = "detail", rename_all = "camelCase")]
pub enum ReaderStatus {
    Ok,
    ExpUnreadable,
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
    pub forget_digits_requested: AtomicBool,
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
            forget_digits_requested: AtomicBool::new(false),
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
            keys: HashMap::new(),
            pending: Vec::new(),
            tried_resolution: None,
            capture_error: None,
            storage_error: None,
            notify_error: None,
        }
    }

    fn run(mut self) {
        let learned = self.state.store.lock().expect("store lock poisoned").level_glyphs();
        match learned {
            Ok(digits) => {
                info!("{} learned level digit(s) loaded", digits.len());
                self.screen.set_level_digits(digits);
            }
            Err(e) => self.storage_failed("loading learned level digits", e),
        }
        while !self.state.shutdown.load(Ordering::Acquire) {
            self.handle_requests();
            self.ensure_capture();
            match self.rx.recv_timeout(Duration::from_millis(500)) {
                Ok(CaptureMsg::Regions { t, exp, map, level }) => self.on_regions(t, exp, map, level),
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

    /// Requests from commands: end the session (a reset), forget learned level digits.
    fn handle_requests(&mut self) {
        if self.state.end_session_requested.swap(false, Ordering::AcqRel) {
            info!("session reset by the user");
            let fx = self.tracker.end_session();
            self.apply(fx);
        }
        if self.state.forget_digits_requested.swap(false, Ordering::AcqRel) {
            info!("learned level digits forgotten; relearning from the next steady level");
            self.screen.set_level_digits(Vec::new());
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

    fn on_regions(&mut self, t: i64, exp: Crop, map: Crop, level: Option<Crop>) {
        self.frames_flowing();
        let read = self.screen.read(t, &exp, map, level);
        if let Some((signature, strips)) = &read.new_map {
            self.resolve_map(t, signature, strips);
        }
        let fx = self.tracker.on_sample(Sample { t, exp: read.exp, map: self.current_map });
        let learned = self.screen.sync_level(&mut self.tracker, read.exp);
        self.save_level_digits(&learned);
        // Also publishes the minute in progress, before the tick: the UI reloads the chart on each tick.
        self.apply(fx);
        self.publish(match (read.exp, read.map_readable) {
            (None, _) => ReaderStatus::ExpUnreadable,
            (Some(_), false) => ReaderStatus::MapUnreadable,
            (Some(_), true) => ReaderStatus::Ok,
        });
    }

    /// The digits are already in use; if saving fails, they're relearned next run.
    fn save_level_digits(&mut self, digits: &[LevelGlyph]) {
        if digits.is_empty() {
            return;
        }
        let saved = {
            let store = self.state.store.lock().expect("store lock poisoned");
            digits.iter().try_for_each(|glyph| store.add_level_glyph(glyph))
        };
        if let Err(e) = saved {
            self.storage_failed("saving a learned level digit", e);
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
                    info!("session #{id} started on map #{map}");
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
