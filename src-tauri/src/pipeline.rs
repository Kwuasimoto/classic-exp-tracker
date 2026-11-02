//! Glue between capture, readers, tracker and storage. Runs on its own thread; the
//! tracker and readers stay pure, all I/O happens here.

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use log::{error, info, warn};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::capture::{self, CaptureMsg, Control, LootCrops, Regions, Shared};
use crate::loot::LootReader;
use crate::geom::{boxes_for, resolution_key, Corner, Crop, NATIVE};
use crate::grinds;
use crate::read::exp::ExpReading;
use crate::read::map_strip::Signature;
use crate::read::ocr::MapOcr;
use crate::read::LearnedGlyph;
use crate::rates::{self, Loot, MinuteRow, Totals};
use crate::screen::{FrameRead, Screen};
use crate::read::map_names::{match_name, NameMatch};
use crate::store::{NamedBy, Store, StoreError};
use crate::tracker::{Effect, MapId, Sample, SessionKey, Tracker};

pub const TICK_EVENT: &str = "tracker-tick";
const MAX_BACKOFF: Duration = Duration::from_secs(30);
/// Reads of a map without a name per visit, NAME_RETRY_SECS apart (see resolve_map).
const NAME_RETRIES: u32 = 4;
const NAME_RETRY_SECS: i64 = 15;
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", content = "detail", rename_all = "camelCase")]
pub enum ReaderStatus {
    Ok,
    ExpUnreadable,
    MapUnreadable,
    /// Neither the EXP box nor the minimap has shown any text for a minute: the game isn't
    /// full screen on the main display (the only one captured).
    GameNotFound,
    /// No frame read yet.
    Starting,
    /// The screen isn't 16:9 (its resolution): the boxes and the EXP reader need it.
    ScreenNotSupported(String),
    CaptureFailed(String),
    StorageFailed(String),
    NotifyFailed(String),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tick {
    pub status: ReaderStatus,
    /// The map on screen, once confirmed, and its name: `None` for a map not on the map
    /// list that the user hasn't named ("Unrecognised map").
    pub map_id: Option<i64>,
    pub map_name: Option<String>,
    /// In a shop (the minimap is collapsed): shown instead of the map.
    pub shopping: bool,
    /// The map the live view counts alone, and its name (`None` while unnamed).
    pub locked_map: Option<i64>,
    pub locked_name: Option<String>,
    /// EXP into the level: read off the counter, or (`exp_exact` false) worked out from the
    /// percent while the readout shows only that (read::exp::ExpReading).
    pub exp: Option<u64>,
    pub exp_exact: bool,
    pub pct_bp: Option<u32>,
    pub total_est: Option<u64>,
    /// Known once the readings fit exactly one level's table total.
    pub level: Option<u32>,
    pub loot: LootStatus,
    /// Training time since the live view started (seconds), across maps: the popout's clock.
    pub grind_secs: u64,
    /// EXP and mesos made on every map since the user last reset the totals.
    pub totals: Totals,
}

/// Meso tracking, as the UI shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LootStatus {
    pub enabled: bool,
    /// The corner the inventory should be in, and whether its title is there.
    pub corner: Corner,
    pub found: bool,
    /// The inventory's meso total reads, and its Etc tab is open.
    pub inventory: bool,
    /// The meso total as last read.
    pub meso_total: Option<u64>,
    pub etc_tab: bool,
    /// The quickslot reads.
    pub quickslot: bool,
    /// Items seen that the user hasn't named or ignored yet (they count nothing until named).
    pub unnamed_items: u32,
    /// The Etc items picked up since the app started or the session was reset, highest
    /// value first.
    pub picked_up: Vec<PickedUp>,
    /// The main HP and MP items, the only potions whose use costs mesos.
    pub main_hp: Option<MainPotion>,
    pub main_mp: Option<MainPotion>,
    /// The quickslot layout read this frame; `None` when the quickslot didn't read.
    pub quickslot_layout: Option<QuickslotLayout>,
    /// The Use items on the quickslot when it last read, in slot order: the potions the user
    /// can choose as main HP or MP.
    pub quickslot_pots: Vec<QuickslotPot>,
    /// When the inventory was last seen with everything made counted (unix seconds): the
    /// loot rates are held at that moment while it's closed (rates::bars_range).
    pub seen_at: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum QuickslotLayout {
    /// The fan of four slots (the game's default).
    Controller,
    /// The 4×2 keys.
    Keyboard,
}

/// A Use item on the quickslot and how many of it are there.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickslotPot {
    pub item_id: i64,
    /// `None` while the item isn't named.
    pub name: Option<String>,
    pub count: u32,
    /// The item's patch (RGB, read::inventory::PATCH wide, row by row).
    pub patch: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MainPotion {
    pub item_id: i64,
    /// `None` while the item isn't named.
    pub name: Option<String>,
}

/// One Etc item's pickups, for the Mesos page's list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickedUp {
    pub item_id: i64,
    /// `None` while the item isn't named.
    pub name: Option<String>,
    pub count: u32,
    /// At its NPC price when each was counted: 0 for those picked up while unnamed.
    pub value: u64,
    /// The item's icon patch (RGB, read::inventory::PATCH wide, row by row).
    pub patch: Vec<u8>,
}

impl Default for Tick {
    fn default() -> Self {
        Self {
            status: ReaderStatus::Starting,
            map_id: None,
            map_name: None,
            shopping: false,
            locked_map: None,
            locked_name: None,
            exp: None,
            exp_exact: false,
            pct_bp: None,
            total_est: None,
            level: None,
            loot: LootStatus::default(),
            grind_secs: 0,
            totals: Totals::default(),
        }
    }
}

pub struct AppState {
    pub store: Mutex<Store>,
    pub capture: Arc<Shared>,
    pub live: RwLock<Tick>,
    /// The tracker's minute in progress, added to the live chart (see commands::get_live_chart).
    /// Written and read only while holding `store`, so a minute is never both stored and live.
    pub live_minute: RwLock<Option<MinuteRow>>,
    /// Where the live view starts (unix seconds); moved forward by a reset. Also in the database.
    pub live_since: RwLock<i64>,
    /// The stored grinding time (Store::grind_secs), without the minute in progress. Changed
    /// only while holding `store`, with the database.
    pub grind_secs: AtomicU64,
    /// The stored totals (Store::totals), without the minute in progress. Changed only while
    /// holding `store`, with the database.
    pub totals: RwLock<Totals>,
    /// The map the live view counts alone (the user locked it); also in the database.
    pub locked_map: RwLock<Option<MapId>>,
    /// Set by commands, handled by the pipeline thread on its next loop.
    pub end_session_requested: AtomicBool,
    /// The user named or ignored an item, or chose a main potion: the pipeline reloads its items.
    pub items_changed: AtomicBool,
    shutdown: AtomicBool,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl AppState {
    pub fn new(store: Store, live_since: i64, grind_secs: u64, totals: Totals, locked_map: Option<MapId>, loot_enabled: bool, corner: Corner) -> Self {
        Self {
            store: Mutex::new(store),
            capture: Arc::new(Shared {
                loot_enabled: AtomicBool::new(loot_enabled),
                inventory_corner: RwLock::new(corner),
                ..Shared::default()
            }),
            live: RwLock::new(Tick::default()),
            live_minute: RwLock::new(None),
            live_since: RwLock::new(live_since),
            grind_secs: AtomicU64::new(grind_secs),
            totals: RwLock::new(totals),
            locked_map: RwLock::new(locked_map),
            end_session_requested: AtomicBool::new(false),
            items_changed: AtomicBool::new(false),
            shutdown: AtomicBool::new(false),
            thread: Mutex::new(None),
        }
    }
}

pub fn spawn(app: AppHandle, state: Arc<AppState>, chat: SyncSender<(i64, Crop)>) -> std::io::Result<()> {
    let thread_state = Arc::clone(&state);
    let handle = std::thread::Builder::new()
        .name("exp-pipeline".into())
        .spawn(move || Pipeline::new(app, thread_state, chat).run())?;
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
    /// The chat reader's thread (chat_reader.rs): each chat strip the capture takes.
    chat: SyncSender<(i64, Crop)>,
    control: Option<Control>,
    retry_at: Instant,
    backoff: Duration,
    ocr: Result<MapOcr, String>,
    screen: Screen,
    tracker: Tracker,
    current_map: Option<MapId>,
    /// Reads of the current map's name this visit, and when the next is due (it has none yet).
    name_retries: u32,
    retry_name_at: Option<i64>,
    /// The minimap is collapsed: the player is in a shop.
    shopping: bool,
    loot: LootReader,
    loot_status: LootStatus,
    /// The resolution the level digits in use were learned at.
    resolution: Option<String>,
    keys: HashMap<SessionKey, i64>,
    pending: Vec<Effect>,
    /// The minute grinds were last checked for being over (see `file_grinds`).
    grinds_checked: Option<i64>,
    /// The last resolution whose boxes (or lack of them) were logged.
    logged_resolution: Option<(u32, u32)>,
    capture_error: Option<String>,
    storage_error: Option<String>,
    notify_error: Option<String>,
}

impl Pipeline {
    fn new(app: AppHandle, state: Arc<AppState>, chat: SyncSender<(i64, Crop)>) -> Self {
        let (tx, rx) = sync_channel(4);
        Self {
            app,
            state,
            tx,
            rx,
            chat,
            control: None,
            retry_at: Instant::now(),
            backoff: Duration::from_secs(1),
            ocr: MapOcr::new().map_err(|e| e.to_string()),
            screen: Screen::new(),
            tracker: Tracker::new(),
            current_map: None,
            name_retries: 0,
            retry_name_at: None,
            shopping: false,
            loot: LootReader::default(),
            loot_status: LootStatus::default(),
            resolution: None,
            keys: HashMap::new(),
            pending: Vec::new(),
            grinds_checked: None,
            logged_resolution: None,
            capture_error: None,
            storage_error: None,
            notify_error: None,
        }
    }

    fn run(mut self) {
        while !self.state.shutdown.load(Ordering::Acquire) {
            self.handle_requests();
            let minute = crate::now_unix().div_euclid(60);
            if self.grinds_checked != Some(minute) {
                self.grinds_checked = Some(minute);
                self.file_grinds(false);
            }
            self.ensure_capture();
            match self.rx.recv_timeout(Duration::from_millis(500)) {
                Ok(CaptureMsg::Regions { t, width, height, exp, map, level, bar, loot, chat }) => {
                    if let Some(strip) = chat {
                        self.send_to_chat_reader(t, strip);
                    }
                    self.on_regions(t, (width, height), exp, map, level, &bar, loot.as_deref())
                }
                Ok(CaptureMsg::NoBoxes { width, height }) => self.on_new_screen(width, height),
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

    /// Requests from commands: end the session (a reset), an item named.
    fn handle_requests(&mut self) {
        if self.state.items_changed.swap(false, Ordering::AcqRel) {
            self.loot.reload();
        }
        if self.state.end_session_requested.swap(false, Ordering::AcqRel) {
            info!("session reset by the user");
            self.loot.clear_pickups();
            let fx = self.tracker.end_session();
            self.apply(fx);
            // A reset says the grind is done: save it now rather than after the wait.
            self.file_grinds(true);
        }
    }

    /// Save the grinds that are over as automatic snapshots (or drop them, too short), and
    /// delete the automatic snapshots past their keep. Once a minute, at startup and on a reset.
    fn file_grinds(&mut self, all: bool) {
        let now = crate::now_unix();
        let level = self.tracker.level();
        let store = self.state.store.lock().expect("store lock poisoned");
        let result = (|| -> Result<(), StoreError> {
            for grind in grinds::finished(&store.unfiled_sessions()?, now, all) {
                if !grind.worth_saving() {
                    store.file_grind(&grind.sessions, None, None)?;
                    continue;
                }
                let map_name = grind.map.map(|m| store.map_name(m)).transpose()?.flatten().map(|(name, _)| name);
                let name = map_name.as_deref().unwrap_or("Unrecognised map");
                let id = store.file_grind(&grind.sessions, Some((name, map_name.as_deref())), level)?;
                info!("grind on {name} over ({}s training, sessions {:?}): snapshot {id:?}", grind.training_secs, grind.sessions);
            }
            let expired = store.delete_expired_snapshots(now - grinds::KEEP_AUTO_SECS)?;
            if expired > 0 {
                info!("deleted {expired} automatic snapshot(s) past their keep");
            }
            Ok(())
        })();
        drop(store);
        if let Err(e) = result {
            self.storage_failed("saving finished grinds as snapshots", e);
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
            self.publish(ReaderStatus::Starting);
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
                self.publish(ReaderStatus::Starting);
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

    /// A frame at a new resolution: place the boxes for it, or say why there are none.
    fn on_new_screen(&mut self, width: u32, height: u32) {
        self.frames_flowing();
        self.use_glyphs_for(width, height);
        let corner = *self.state.capture.inventory_corner.read().expect("corner lock poisoned");
        let boxes = boxes_for(width, height, corner);
        if self.logged_resolution != Some((width, height)) {
            match boxes {
                Some(boxes) => info!("boxes for {width}x{height} scaled from {}x{}: {boxes:?}", NATIVE.0, NATIVE.1),
                None => warn!(
                    "{width}x{height} is not 16:9; EXP can only be read at a 16:9 resolution with the game full screen"
                ),
            }
            self.logged_resolution = Some((width, height));
        }
        match boxes {
            Some(boxes) => {
                *self.state.capture.regions.write().expect("regions lock poisoned") = Some(Regions { width, height, boxes });
                self.publish(ReaderStatus::Starting);
            }
            None => self.publish(ReaderStatus::ScreenNotSupported(resolution_key(width, height))),
        }
    }

    /// Hand a chat strip to the chat reader. It reads only at 1366x768 for now (its
    /// measurements are native); busy, it drops the strip and takes the next.
    fn send_to_chat_reader(&mut self, t: i64, strip: Crop) {
        if (strip.width, strip.height) != (crate::read::chat::STRIP.w, crate::read::chat::STRIP.h) {
            return;
        }
        match self.chat.try_send((t, strip)) {
            Ok(()) | Err(TrySendError::Full(_)) => {}
            Err(TrySendError::Disconnected(_)) => warn!("chat reader stopped; the chat isn't read"),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn on_regions(&mut self, t: i64, (width, height): (u32, u32), exp: Crop, map: Crop, level: Crop, bar: &Crop, loot: Option<&LootCrops>) {
        self.frames_flowing();
        self.use_glyphs_for(width, height);
        if self.retry_name_at.is_some_and(|at| t >= at) {
            self.retry_name_at = None;
            self.screen.reconfirm_map();
        }
        let FrameRead { exp, map_readable, new_map, shopping, game_hidden } = self.screen.read(t, &exp, map, level, bar);
        if let Some((signature, strips)) = &new_map {
            self.resolve_map(t, signature, strips);
        }
        if shopping != self.shopping {
            info!("{}", if shopping { "minimap collapsed: in a shop" } else { "minimap back: left the shop" });
            self.shopping = shopping;
        }
        // In a shop the tracker sees no map (time there isn't training); the map from
        // before is kept, so coming back out to it carries on.
        let map = if shopping { None } else { self.current_map };
        let read = self.loot.on_frame(&self.state.store.lock().expect("store lock poisoned"), t, loot, shopping);
        let (loot, loot_seen) = match read {
            Ok((loot, seen, status)) => {
                self.loot_status = status;
                (loot, seen)
            }
            Err(e) => {
                self.storage_failed("saving a new item", e);
                (Loot::default(), false)
            }
        };
        let fx = self.tracker.on_sample(Sample { t, exp, map, loot, loot_seen });
        let learned = self.screen.sync_level(&mut self.tracker, exp);
        self.save_glyphs(&learned);
        // Also publishes the minute in progress, before the tick: the UI reloads the chart on each tick.
        self.apply(fx);
        self.publish(match (exp, map_readable || shopping) {
            (None, _) if game_hidden => ReaderStatus::GameNotFound,
            (None, _) => ReaderStatus::ExpUnreadable,
            (Some(_), false) => ReaderStatus::MapUnreadable,
            (Some(_), true) => ReaderStatus::Ok,
        });
    }

    /// Use this resolution's level digits when they aren't the ones in use. On a storage error
    /// none are marked in use (nothing is saved under the wrong resolution) and the next
    /// frame retries.
    fn use_glyphs_for(&mut self, width: u32, height: u32) {
        let key = resolution_key(width, height);
        if self.resolution.as_deref() == Some(key.as_str()) {
            return;
        }
        if let Err(e) = self.load_glyphs(key) {
            self.resolution = None;
            self.storage_failed("loading learned glyphs", e);
        }
    }

    /// Use the level digits learned at `resolution`.
    fn load_glyphs(&mut self, resolution: String) -> Result<(), StoreError> {
        let level: Vec<LearnedGlyph> =
            self.state.store.lock().expect("store lock poisoned").glyphs(&resolution)?;
        let mut heights: Vec<usize> = level.iter().map(|g| g.rows.len()).collect();
        heights.sort_unstable();
        heights.dedup();
        info!("level digits for {resolution}: {} learned ({heights:?} rows tall)", level.len());
        self.screen.set_level_glyphs(level);
        self.resolution = Some(resolution);
        Ok(())
    }

    /// Save digits just learned (already in use by the level box).
    fn save_glyphs(&mut self, glyphs: &[LearnedGlyph]) {
        let Some(resolution) = &self.resolution else { return };
        let saved = {
            let store = self.state.store.lock().expect("store lock poisoned");
            glyphs.iter().try_for_each(|glyph| store.add_glyph(resolution, glyph).map(drop))
        };
        if let Err(e) = saved {
            self.storage_failed("saving a learned glyph", e);
        }
    }

    /// A map's name for the UI; `None` without a map, while it has no name, or on a
    /// storage error (logged).
    fn name_of(&mut self, map: Option<MapId>) -> Option<String> {
        let found = self.state.store.lock().expect("store lock poisoned").map_name(map?);
        match found {
            Ok(name) => name.map(|(name, _)| name),
            Err(e) => {
                self.storage_failed("loading a map name", e);
                None
            }
        }
    }

    fn storage_failed(&mut self, doing: &str, e: impl Display) {
        error!("database error while {doing}: {e}");
        self.storage_error = Some(e.to_string());
    }

    /// A map confirmed on screen: find it by any of its looks, or by the list name its
    /// strip reads as, or save it. A map without a name is read again up to NAME_RETRIES
    /// times this visit: OCR often reads a name cut short once and whole a moment later.
    fn resolve_map(&mut self, t: i64, signature: &Signature, strips: &[Crop]) {
        match self.place_map(t, signature, strips) {
            Ok((id, named)) => {
                if self.current_map != Some(id) {
                    self.name_retries = 0;
                }
                self.current_map = Some(id);
                self.retry_name_at = (!named && self.name_retries < NAME_RETRIES).then(|| {
                    self.name_retries += 1;
                    t + NAME_RETRY_SECS
                });
            }
            Err(e) => {
                self.storage_failed("looking up or saving a map", e);
                // Tried again later, not on the next frames: each try runs OCR.
                self.retry_name_at = Some(t + NAME_RETRY_SECS);
            }
        }
    }

    /// The map and whether it has a name. Only a map without one is read with OCR, and
    /// it's named only from the map list (read::map_names); the user's names stay.
    fn place_map(&self, t: i64, signature: &Signature, strips: &[Crop]) -> Result<(MapId, bool), StoreError> {
        let known = self.find_map(signature)?;
        if let Some(id) = known
            && let Some((name, _)) = self.state.store.lock().expect("store lock poisoned").map_name(id)?
        {
            info!("map confirmed: {name:?} (#{id})");
            return Ok((id, true));
        }
        let ocr = self.read_name(strips);
        let listed = match ocr.as_deref().map(match_name) {
            Some(NameMatch::Listed(name)) => Some(name),
            _ => None,
        };
        let store = self.state.store.lock().expect("store lock poisoned");
        Ok(match (known, listed) {
            (Some(id), Some(name)) => match store.map_by_name(name)? {
                Some(other) if other != id => {
                    store.move_looks(id, other)?;
                    info!("unrecognised map #{id} reads as {name:?} (OCR {ocr:?}): it is map #{other}");
                    let mut locked = self.state.locked_map.write().expect("locked map lock poisoned");
                    if *locked == Some(id) {
                        store.set_locked_map(Some(other))?;
                        *locked = Some(other);
                        info!("map lock moved to map #{other} with its looks");
                    }
                    (other, true)
                }
                _ => {
                    store.set_map_name(id, name, NamedBy::List)?;
                    info!("unrecognised map #{id} reads as {name:?} (OCR {ocr:?}): named");
                    (id, true)
                }
            },
            (Some(id), None) => {
                info!("map confirmed: unrecognised map #{id} (OCR {ocr:?} is not on the map list)");
                (id, false)
            }
            (None, Some(name)) => match store.map_by_name(name)? {
                Some(id) => {
                    store.add_signature(id, signature)?;
                    info!("map confirmed: {name:?} (#{id}), seen in a new look (OCR {ocr:?})");
                    (id, true)
                }
                None => {
                    let id = store.insert_map(Some((name, NamedBy::List)), signature, t)?;
                    info!("map confirmed: new map #{id} {name:?} (OCR {ocr:?})");
                    (id, true)
                }
            },
            (None, None) => {
                let id = store.insert_map(None, signature, t)?;
                info!("map confirmed: new unrecognised map #{id} (OCR {ocr:?} is not on the map list)");
                (id, false)
            }
        })
    }

    fn find_map(&self, signature: &Signature) -> Result<Option<MapId>, StoreError> {
        let maps = self.state.store.lock().expect("store lock poisoned").maps()?;
        Ok(maps.into_iter().find(|m| m.signature.matches(signature)).map(|m| m.id))
    }

    /// What OCR reads on the strips; `None` when it reads nothing or fails (logged).
    fn read_name(&self, strips: &[Crop]) -> Option<String> {
        let ocr = match &self.ocr {
            Ok(ocr) => ocr,
            Err(e) => {
                warn!("map name not read: OCR unavailable: {e}");
                return None;
            }
        };
        ocr.label(strips).unwrap_or_else(|e| {
            warn!("map name not read: OCR failed: {e}");
            None
        })
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
                Effect::Minute { minute_start, key, exp_gained, training_secs, loot, loot_seen } => {
                    let id = *self.keys.get(&key).expect("SessionStarted is applied before its minutes");
                    store
                        .add_minute(minute_start, id, exp_gained, training_secs, loot, loot_seen)
                        .map(|()| {
                            self.state.grind_secs.fetch_add(u64::from(training_secs), Ordering::AcqRel);
                            self.state.totals.write().expect("totals lock poisoned").add(minute_start, exp_gained, loot);
                            info!("session #{id} minute saved: +{exp_gained} EXP, {training_secs}s training, {loot:?}");
                        })
                }
            };
            if let Err(e) = result {
                failure = Some(e);
                break;
            }
            done += 1;
        }
        let locked = *self.state.locked_map.read().expect("locked map lock poisoned");
        *self.state.live_minute.write().expect("live minute lock poisoned") = self.tracker.current_minute(locked);
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
        let locked_map = *self.state.locked_map.read().expect("locked map lock poisoned");
        let map_name = self.name_of(self.current_map);
        let locked_name = self.name_of(locked_map);
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
        let live_minute = self.tracker.current_minute(None);
        let mut totals = *self.state.totals.read().expect("totals lock poisoned");
        if let Some(m) = live_minute {
            totals.add(m.minute_start, m.exp_gained, m.loot);
        }
        let tick = Tick {
            status,
            map_id: self.current_map,
            map_name,
            shopping: self.shopping,
            locked_map,
            locked_name,
            exp: baseline.and_then(|r| r.exp_in_level(self.tracker.total_estimate())),
            exp_exact: matches!(baseline, Some(ExpReading::Counter { .. })),
            pct_bp: baseline.map(ExpReading::pct_bp),
            total_est: self.tracker.total_estimate(),
            level: self.tracker.level(),
            loot: LootStatus {
                enabled: self.state.capture.loot_enabled.load(Ordering::Acquire),
                corner: *self.state.capture.inventory_corner.read().expect("corner lock poisoned"),
                ..self.loot_status.clone()
            },
            grind_secs: self.state.grind_secs.load(Ordering::Acquire) + live_minute.map_or(0, |m| u64::from(m.training_secs)),
            totals,
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
