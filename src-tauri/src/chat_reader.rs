//! The chat reader's own thread: each chat box the capture takes (only while the
//! translator is on), in native pixels (read::chat::native_strip), goes through the chat tracker (tracker::chat), and each new message is
//! read with the OCR (read::chat_ocr), logged, kept for the page and sent to it. A line's
//! English translation is kept with it, so every window shows the same one. Reading takes
//! about 30 ms a line, so it runs apart from the EXP pipeline: a strip that arrives while the
//! last is still being read is dropped, and the next comes a second later.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::Mutex;

use log::{debug, info, warn};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::capture::ChatCrop;
use crate::read::chat::{native_strip, LineKind, Megaphone};
use crate::read::chat_ocr::{Engine, OcrError, Reader};
use crate::tracker::chat::ChatTracker;

pub const CHAT_EVENT: &str = "chat-line";
/// A line's English translation changed: started, done or failed.
pub const ENGLISH_EVENT: &str = "chat-english";
/// Lines kept for the page.
const KEPT: usize = 200;

/// A message as read: the sender empty on notices and crate announcements.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatLine {
    /// Unique among the lines read since the app started.
    pub id: u64,
    pub t: i64,
    /// The game's own "hh:mm", when the player has its timestamps on.
    pub time: Option<String>,
    pub kind: &'static str,
    pub channel: Option<u8>,
    pub sender: String,
    pub text: String,
    /// The reader's lowest probability of any character in the line.
    pub confidence: f32,
    /// Its English translation, once the user has asked for one.
    pub english: Option<English>,
}

/// A line's English translation (commands::translate_line).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum English {
    Busy,
    Done { text: String, model: &'static str },
    Failed { error: String },
}

/// What [`ChatLog::begin_english`] found.
pub enum Begin {
    /// The line is now Busy; translate this text.
    Started(String),
    /// Its translation is already being made.
    AlreadyBusy,
    /// No such line, or it's no longer kept.
    Gone,
}

/// The latest lines read, newest last (managed state, for the chat_lines command).
#[derive(Default)]
pub struct ChatLog(Mutex<VecDeque<ChatLine>>);

impl ChatLog {
    pub fn lines(&self) -> Vec<ChatLine> {
        self.0.lock().expect("chat log lock poisoned").iter().cloned().collect()
    }

    fn push(&self, line: ChatLine) {
        let mut lines = self.0.lock().expect("chat log lock poisoned");
        lines.push_back(line);
        if lines.len() > KEPT {
            lines.pop_front();
        }
    }

    /// Mark line `id` Busy, unless it's already being translated.
    pub fn begin_english(&self, id: u64) -> Begin {
        let mut lines = self.0.lock().expect("chat log lock poisoned");
        match lines.iter_mut().find(|l| l.id == id) {
            None => Begin::Gone,
            Some(line) if line.english == Some(English::Busy) => Begin::AlreadyBusy,
            Some(line) => {
                line.english = Some(English::Busy);
                Begin::Started(line.text.clone())
            }
        }
    }

    /// Set line `id`'s translation (nothing if it's no longer kept) and send it to every window.
    pub fn set_english(&self, app: &AppHandle, id: u64, english: English) -> tauri::Result<()> {
        if let Some(line) = self.0.lock().expect("chat log lock poisoned").iter_mut().find(|l| l.id == id) {
            line.english = Some(english.clone());
        }
        app.emit(ENGLISH_EVENT, EnglishChange { id, english })
    }
}

#[derive(Clone, Serialize)]
struct EnglishChange {
    id: u64,
    english: English,
}

fn kind_name(kind: LineKind) -> &'static str {
    match kind {
        LineKind::Normal => "normal",
        LineKind::Party => "party",
        LineKind::Notice => "notice",
        LineKind::Crate => "crate",
        LineKind::Friend => "friend",
        LineKind::Megaphone(Megaphone::Pink) => "megaphone-pink",
        LineKind::Megaphone(Megaphone::Gold) => "megaphone-gold",
        LineKind::Megaphone(Megaphone::Teal) => "megaphone-teal",
        LineKind::Megaphone(Megaphone::Channel) => "megaphone-channel",
    }
}

/// Start the thread; strips sent to the returned channel are read in turn. `engine` is the
/// downloaded ONNX Runtime DLL, `latin` the Latin reader's folder: both are loaded with the
/// first strip (the translator is on only once they're downloaded).
pub fn spawn(app: AppHandle, engine: PathBuf, latin: PathBuf) -> std::io::Result<SyncSender<(i64, ChatCrop)>> {
    let (tx, rx) = sync_channel(1);
    std::thread::Builder::new().name("chat-reader".into()).spawn(move || run(&app, &rx, &engine, &latin))?;
    Ok(tx)
}

fn run(app: &AppHandle, rx: &Receiver<(i64, ChatCrop)>, engine: &Path, latin: &Path) {
    let mut tracker = ChatTracker::default();
    // Loaded with the first strip; a failure is logged once and nothing is read after it.
    let mut loaded: Option<Result<(Engine, Reader), OcrError>> = None;
    let mut last_skipped = Vec::new();
    let mut next_id = 0u64;
    let mut unreadable = false;
    while let Ok((t, chat)) = rx.recv() {
        let models = loaded.get_or_insert_with(|| {
            let result = Engine::load(engine).and_then(|e| Reader::load(&e, latin).map(|r| (e, r)));
            match &result {
                Ok(_) => info!("chat reader: OCR engine and Latin reader loaded"),
                Err(e) => warn!("chat reader: couldn't load the OCR ({e}); the chat won't be read until the app restarts"),
            }
            result
        });
        let Ok((_, reader)) = models.as_mut() else { continue };
        // Every 16:9 box holds its native pixels, so this is a bug if it happens: logged
        // once a run.
        let Some(strip) = native_strip(&chat.crop, &chat.at) else {
            if !unreadable {
                warn!("chat reader: couldn't take the chat box ({}x{}, {:?}) back to native pixels", chat.crop.width, chat.crop.height, chat.at);
                unreadable = true;
            }
            continue;
        };
        unreadable = false;
        let observed = tracker.observe(&strip);
        // What's skipped repeats every frame while it lasts: logged when it changes.
        if observed.skipped != last_skipped {
            debug!("chat reader: skipped {:?}", observed.skipped);
            last_skipped = observed.skipped;
        }
        for message in observed.new {
            match reader.read_line(&strip, message.kind, &message.parts) {
                Ok(read) => {
                    next_id += 1;
                    let line = ChatLine {
                        id: next_id,
                        t,
                        time: read.time,
                        kind: kind_name(message.kind),
                        channel: message.channel,
                        sender: read.sender,
                        text: read.text,
                        confidence: read.confidence,
                        english: None,
                    };
                    info!(
                        "chat: {}{}{} [{:.2}] {:?}: {:?}",
                        line.time.as_ref().map_or(String::new(), |t| format!("{t} ")),
                        line.kind,
                        line.channel.map_or(String::new(), |c| format!(" CH{c:02}")),
                        line.confidence,
                        line.sender,
                        line.text
                    );
                    if let Err(e) = app.emit(CHAT_EVENT, &line) {
                        warn!("chat reader: couldn't send a line to the page: {e}");
                    }
                    app.state::<ChatLog>().push(line);
                }
                Err(e) => warn!("chat reader: couldn't read a {:?} line: {e}", message.kind),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_translated_once_at_a_time() {
        let log = ChatLog::default();
        let line = |id| ChatLine { id, t: 0, time: None, kind: "normal", channel: None, sender: "a".into(), text: "hola".into(), confidence: 1.0, english: None };
        log.push(line(1));
        assert!(matches!(log.begin_english(2), Begin::Gone));
        assert!(matches!(log.begin_english(1), Begin::Started(t) if t == "hola"));
        assert!(matches!(log.begin_english(1), Begin::AlreadyBusy));
        assert_eq!(log.lines()[0].english, Some(English::Busy));
    }
}
