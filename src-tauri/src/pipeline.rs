//! Glue between capture, readers, tracker and storage. Runs on its own thread; the
//! tracker and readers stay pure, all I/O happens here.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::capture::{self, CaptureMsg, Control, Regions, Shared};
use crate::geom::{resolution_key, Crop};
use crate::read::exp::read_exp;
use crate::read::map_strip::{read_strip, Signature};
use crate::read::ocr::MapOcr;
use crate::store::{Store, StoreError};
use crate::tracker::{ConfirmOutcome, Effect, MapConfirmer, MapId, Sample, SessionKey, Tracker};

pub const TICK_EVENT: &str = "tracker-tick";
pub const MINUTE_EVENT: &str = "tracker-minute";
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
}

impl Default for Tick {
    fn default() -> Self {
        Self { status: ReaderStatus::NotCalibrated, map_name: None, exp: None, pct_bp: None, total_est: None }
    }
}

pub struct AppState {
    pub store: Mutex<Store>,
    pub capture: Arc<Shared>,
    /// Last full frame grabbed for the calibration screen (memory only).
    pub calibration_frame: Mutex<Option<Crop>>,
    pub live: RwLock<Tick>,
    shutdown: AtomicBool,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl AppState {
    pub fn new(store: Store) -> Self {
        Self {
            store: Mutex::new(store),
            capture: Arc::new(Shared::default()),
            calibration_frame: Mutex::new(None),
            live: RwLock::new(Tick::default()),
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
            eprintln!("exp-pipeline thread panicked; the last minute may not be saved");
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
    confirmer: MapConfirmer,
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
            confirmer: MapConfirmer::new(),
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
        while !self.state.shutdown.load(Ordering::Acquire) {
            self.ensure_capture();
            match self.rx.recv_timeout(Duration::from_millis(500)) {
                Ok(CaptureMsg::Regions { t, exp, map }) => self.on_regions(t, exp, map),
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
                eprintln!("stopping capture at shutdown: {e:?}");
            }
    }

    fn ensure_capture(&mut self) {
        if let Some(finished) = self.control.take_if(|c| c.is_finished()) {
            self.capture_error = Some(match finished.into_thread_handle().join() {
                Ok(Ok(())) => "capture ended".to_string(),
                Ok(Err(e)) => format!("{e:?}"),
                Err(_) => "capture thread panicked".to_string(),
            });
            self.schedule_retry();
            self.publish(ReaderStatus::NotCalibrated);
        }
        if self.control.is_some() || Instant::now() < self.retry_at {
            return;
        }
        match capture::start(self.tx.clone(), Arc::clone(&self.state.capture)) {
            Ok(control) => self.control = Some(control),
            Err(e) => {
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
            self.tried_resolution = Some((width, height));
            let found = self.state.store.lock().expect("store lock poisoned").calibration(&resolution_key(width, height));
            match found {
                Ok(Some(calibration)) => {
                    *self.state.capture.regions.write().expect("regions lock poisoned") =
                        Some(Regions { width, height, calibration });
                }
                Ok(None) => {}
                Err(e) => self.storage_error = Some(e.to_string()),
            }
        }
        self.publish(ReaderStatus::NotCalibrated);
    }

    fn on_regions(&mut self, t: i64, exp_crop: Crop, map_crop: Crop) {
        self.frames_flowing();
        let exp = read_exp(&exp_crop).ok();
        let mut reader = if exp.is_some() { ReaderStatus::Ok } else { ReaderStatus::ExpUnreadable };
        match read_strip(&map_crop) {
            Ok(strip) => {
                if let ConfirmOutcome::Changed { signature, strips } = self.confirmer.feed(strip.signature, map_crop) {
                    self.resolve_map(t, &signature, &strips);
                }
            }
            Err(_) if reader == ReaderStatus::Ok => reader = ReaderStatus::MapUnreadable,
            Err(_) => {}
        }
        let fx = self.tracker.on_sample(Sample { t, exp, map: self.current_map });
        let minute_written = fx.iter().any(|e| matches!(e, Effect::Minute { .. }));
        self.apply(fx);
        self.publish(reader);
        if minute_written
            && let Err(e) = self.app.emit(MINUTE_EVENT, ()) {
                self.notify_error = Some(e.to_string());
            }
    }

    fn resolve_map(&mut self, t: i64, signature: &Signature, strips: &[Crop]) {
        let result = self.find_map(signature).and_then(|found| match found {
            Some(id) => Ok(id),
            None => {
                let name = self.label(strips);
                self.state.store.lock().expect("store lock poisoned").insert_map(&name, signature, t)
            }
        });
        match result {
            Ok(id) => self.current_map = Some(id),
            Err(e) => {
                self.storage_error = Some(e.to_string());
                // Re-confirm on upcoming frames so the lookup is retried.
                self.confirmer = MapConfirmer::new();
            }
        }
    }

    fn find_map(&self, signature: &Signature) -> Result<Option<MapId>, StoreError> {
        let maps = self.state.store.lock().expect("store lock poisoned").maps()?;
        Ok(maps.into_iter().find(|m| m.signature.matches(signature)).map(|m| m.id))
    }

    /// OCR label for a new map. Failures produce a visible, renameable name.
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
    fn apply(&mut self, fx: Vec<Effect>) {
        self.pending.extend(fx);
        if self.pending.is_empty() {
            return;
        }
        let store = self.state.store.lock().expect("store lock poisoned");
        let mut done = 0;
        for effect in &self.pending {
            let result = match *effect {
                Effect::SessionStarted { key, map, t } => store.start_session(map, t).map(|id| {
                    self.keys.insert(key, id);
                }),
                Effect::SessionEnded { key, t } => {
                    let id = *self.keys.get(&key).expect("SessionStarted is applied before its SessionEnded");
                    store.end_session(id, t)
                }
                Effect::Minute { minute_start, key, exp_gained, training_secs } => {
                    let id = *self.keys.get(&key).expect("SessionStarted is applied before its minutes");
                    store.add_minute(minute_start, id, exp_gained, training_secs)
                }
            };
            match result {
                Ok(()) => done += 1,
                Err(e) => {
                    self.storage_error = Some(e.to_string());
                    break;
                }
            }
        }
        drop(store);
        self.pending.drain(..done);
        if self.pending.is_empty() {
            self.storage_error = None;
        }
    }

    fn publish(&mut self, reader: ReaderStatus) {
        let map_name = match self.current_map {
            Some(id) => match self.state.store.lock().expect("store lock poisoned").map_name(id) {
                Ok(name) => name,
                Err(e) => {
                    self.storage_error = Some(e.to_string());
                    None
                }
            },
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
        };
        *self.state.live.write().expect("live lock poisoned") = tick.clone();
        match self.app.emit(TICK_EVENT, &tick) {
            Ok(()) => self.notify_error = None,
            Err(e) => self.notify_error = Some(e.to_string()),
        }
    }
}
