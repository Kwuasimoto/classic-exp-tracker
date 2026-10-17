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
use crate::read::exp::{read_exp, Glyph};
use crate::read::level::{self, LevelGlyph};
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
    confirmer: MapConfirmer,
    tracker: Tracker,
    current_map: Option<MapId>,
    keys: HashMap<SessionKey, i64>,
    pending: Vec<Effect>,
    tried_resolution: Option<(u32, u32)>,
    capture_error: Option<String>,
    storage_error: Option<String>,
    notify_error: Option<String>,
    /// The level box's last pixels, its digits, and the level they read as.
    /// Re-read only when the pixels change (about once per level).
    level_crop: Option<Crop>,
    level_glyphs: Vec<Glyph>,
    screen_level: Option<u32>,
    /// Level digits learned so far (also in the database), and whether the
    /// current level box was already tried for learning.
    learned_digits: Vec<LevelGlyph>,
    learn_tried: bool,
    /// Whether the previous frame's readers failed; failures are logged when this flips.
    exp_failing: bool,
    map_failing: bool,
    minute: MinuteStats,
}

/// Reader counts for one wall-clock minute, logged as one line when the minute ends.
#[derive(Default)]
struct MinuteStats {
    minute: i64,
    frames: u32,
    exp_unreadable: u32,
    map_unreadable: u32,
    last_exp_error: Option<String>,
    last_map_error: Option<String>,
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
            level_crop: None,
            level_glyphs: Vec::new(),
            screen_level: None,
            learned_digits: Vec::new(),
            learn_tried: false,
            exp_failing: false,
            map_failing: false,
            minute: MinuteStats::default(),
        }
    }

    fn run(mut self) {
        let learned = self.state.store.lock().expect("store lock poisoned").level_glyphs();
        match learned {
            Ok(digits) => {
                info!("{} learned level digit(s) loaded", digits.len());
                self.learned_digits = digits;
            }
            Err(e) => self.storage_failed("loading learned level digits", e),
        }
        while !self.state.shutdown.load(Ordering::Acquire) {
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
            self.tried_resolution = Some((width, height));
            let found = self.state.store.lock().expect("store lock poisoned").calibration(&resolution_key(width, height));
            match found {
                Ok(Some(calibration)) => {
                    info!("loaded calibration for {width}x{height}: {calibration:?}");
                    *self.state.capture.regions.write().expect("regions lock poisoned") =
                        Some(Regions { width, height, calibration });
                }
                Ok(None) => info!("no calibration saved for {width}x{height}; waiting for the Calibrate tab"),
                Err(e) => self.storage_failed("loading calibration", e),
            }
        }
        self.publish(ReaderStatus::NotCalibrated);
    }

    fn on_regions(&mut self, t: i64, exp_crop: Crop, map_crop: Crop, level_crop: Option<Crop>) {
        self.frames_flowing();
        self.count_frame(t);
        let exp = match read_exp(&exp_crop) {
            Ok(r) => {
                if self.exp_failing {
                    info!("EXP readable again: {} [{}.{:02}%]", r.exp, r.pct_bp / 100, r.pct_bp % 100);
                }
                Some(r)
            }
            Err(e) => {
                if !self.exp_failing {
                    warn!("EXP unreadable: {e}");
                }
                self.minute.exp_unreadable += 1;
                self.minute.last_exp_error = Some(e.to_string());
                None
            }
        };
        self.exp_failing = exp.is_none();
        let mut reader = if exp.is_some() { ReaderStatus::Ok } else { ReaderStatus::ExpUnreadable };
        match read_strip(&map_crop) {
            Ok(strip) => {
                if self.map_failing {
                    info!("map strip readable again");
                }
                self.map_failing = false;
                if let ConfirmOutcome::Changed { signature, strips } = self.confirmer.feed(strip.signature, map_crop) {
                    self.resolve_map(t, &signature, &strips);
                }
            }
            Err(e) => {
                if !self.map_failing {
                    warn!("map strip unreadable: {e}");
                }
                self.map_failing = true;
                self.minute.map_unreadable += 1;
                self.minute.last_map_error = Some(e.to_string());
                if reader == ReaderStatus::Ok {
                    reader = ReaderStatus::MapUnreadable;
                }
            }
        }
        self.read_level_box(level_crop);
        let fx = self.tracker.on_sample(Sample { t, exp, map: self.current_map });
        self.learn_level_digits();
        if let Some(level) = self.screen_level {
            self.tracker.use_screen_level(level);
        }
        let minute_written = fx.iter().any(|e| matches!(e, Effect::Minute { .. }));
        self.apply(fx);
        self.publish(reader);
        if minute_written
            && let Err(e) = self.app.emit(MINUTE_EVENT, ()) {
                warn!("sending {MINUTE_EVENT} to the UI failed: {e}");
                self.notify_error = Some(e.to_string());
            }
    }

    fn read_level_box(&mut self, crop: Option<Crop>) {
        if crop == self.level_crop {
            return;
        }
        self.level_glyphs = crop.as_ref().and_then(|c| level::segment(c).ok()).unwrap_or_default();
        let read = level::read_level(&self.level_glyphs, &self.learned_digits);
        if read.is_some() && read != self.screen_level {
            info!("level box reads {read:?}");
        }
        self.screen_level = read;
        self.level_crop = crop;
        self.learn_tried = false;
    }

    /// Self-teaching level reader: when the level box shows digits it can't read
    /// yet but the tracker knows the level from the EXP readings, store the new
    /// digits labelled from that level.
    fn learn_level_digits(&mut self) {
        let Some(level) = self.tracker.level() else { return };
        if self.screen_level.is_some() || self.level_glyphs.is_empty() || self.learn_tried {
            return;
        }
        self.learn_tried = true;
        let new = match level::learn(&self.level_glyphs, &self.learned_digits, level) {
            Ok(new) => new,
            Err(why) => {
                warn!("not learning level digits: {why}");
                return;
            }
        };
        for glyph in new {
            let saved = self.state.store.lock().expect("store lock poisoned").add_level_glyph(&glyph);
            match saved {
                Ok(()) => {
                    info!("learned level digit {:?} from level {level}", glyph.ch);
                    self.learned_digits.push(glyph);
                }
                Err(e) => return self.storage_failed("saving a learned level digit", e),
            }
        }
        self.screen_level = level::read_level(&self.level_glyphs, &self.learned_digits);
        info!("level box reads {:?} after learning", self.screen_level);
    }

    /// Count a frame; at each new wall-clock minute, log the previous minute's counts
    /// and its latest reader errors.
    fn count_frame(&mut self, t: i64) {
        let minute = t.div_euclid(60);
        if minute != self.minute.minute {
            let m = &self.minute;
            if m.frames > 0 {
                let mut line = format!(
                    "minute summary: {} frames, EXP unreadable {}, map unreadable {}",
                    m.frames, m.exp_unreadable, m.map_unreadable
                );
                if let Some(e) = &m.last_exp_error {
                    line += &format!("; last EXP error: {e}");
                }
                if let Some(e) = &m.last_map_error {
                    line += &format!("; last map error: {e}");
                }
                info!("{line}");
            }
            self.minute = MinuteStats { minute, ..MinuteStats::default() };
        }
        self.minute.frames += 1;
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
        let mut failure = None;
        for effect in &self.pending {
            let result = match *effect {
                Effect::SessionStarted { key, map, t } => store.start_session(map, t).map(|id| {
                    info!("session #{id} started on map #{map}");
                    self.keys.insert(key, id);
                }),
                Effect::SessionEnded { key, t } => {
                    let id = *self.keys.get(&key).expect("SessionStarted is applied before its SessionEnded");
                    store.end_session(id, t).map(|()| info!("session #{id} ended"))
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
        drop(store);
        if let Some(e) = failure {
            self.storage_failed("saving tracker results (kept queued for retry)", e);
        }
        self.pending.drain(..done);
        if self.pending.is_empty() {
            self.storage_error = None;
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
