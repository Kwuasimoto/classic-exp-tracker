//! The chat reader's own thread: each chat strip the capture takes (only while the
//! translator is on) goes through the chat tracker (tracker::chat), and each new message is
//! read with the OCR (read::chat_ocr), logged, kept for the page and sent to it. Reading takes
//! about 30 ms a line, so it runs apart from the EXP pipeline: a strip that arrives while the
//! last is still being read is dropped, and the next comes a second later.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::Mutex;

use log::{debug, info, warn};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::geom::Crop;
use crate::read::chat::{LineKind, Megaphone};
use crate::read::chat_ocr::{Engine, OcrError, Reader};
use crate::tracker::chat::{ChatTracker, Message};

pub const CHAT_EVENT: &str = "chat-line";
/// Lines kept for the page.
const KEPT: usize = 200;

/// A message as read: the sender empty on notices and crate announcements.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatLine {
    pub t: i64,
    pub kind: &'static str,
    pub channel: Option<u8>,
    pub sender: String,
    pub text: String,
    /// The reader's lowest probability of any character in the line.
    pub confidence: f32,
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
}

fn kind_name(kind: LineKind) -> &'static str {
    match kind {
        LineKind::Normal => "normal",
        LineKind::Party => "party",
        LineKind::Notice => "notice",
        LineKind::Crate => "crate",
        LineKind::Megaphone(Megaphone::Pink) => "megaphone-pink",
        LineKind::Megaphone(Megaphone::Gold) => "megaphone-gold",
        LineKind::Megaphone(Megaphone::Teal) => "megaphone-teal",
        LineKind::Megaphone(Megaphone::Channel) => "megaphone-channel",
    }
}

/// Start the thread; strips sent to the returned channel are read in turn. `engine` is the
/// downloaded ONNX Runtime DLL, `latin` the Latin reader's folder: both are loaded with the
/// first strip (the translator is on only once they're downloaded).
pub fn spawn(app: AppHandle, engine: PathBuf, latin: PathBuf) -> std::io::Result<SyncSender<(i64, Crop)>> {
    let (tx, rx) = sync_channel(1);
    std::thread::Builder::new().name("chat-reader".into()).spawn(move || run(&app, &rx, &engine, &latin))?;
    Ok(tx)
}

fn run(app: &AppHandle, rx: &Receiver<(i64, Crop)>, engine: &Path, latin: &Path) {
    let mut tracker = ChatTracker::default();
    // Loaded with the first strip; a failure is logged once and nothing is read after it.
    let mut loaded: Option<Result<(Engine, Reader), OcrError>> = None;
    let mut last_skipped = Vec::new();
    while let Ok((t, strip)) = rx.recv() {
        let models = loaded.get_or_insert_with(|| {
            let result = Engine::load(engine).and_then(|e| Reader::load(&e, latin).map(|r| (e, r)));
            match &result {
                Ok(_) => info!("chat reader: OCR engine and Latin reader loaded"),
                Err(e) => warn!("chat reader: couldn't load the OCR ({e}); the chat won't be read until the app restarts"),
            }
            result
        });
        let Ok((_, reader)) = models.as_mut() else { continue };
        let observed = tracker.observe(&strip);
        // What's skipped repeats every frame while it lasts: logged when it changes.
        if observed.skipped != last_skipped {
            debug!("chat reader: skipped {:?}", observed.skipped);
            last_skipped = observed.skipped;
        }
        for message in observed.new {
            match read(reader, &strip, &message) {
                Ok((sender, text, confidence)) => {
                    let line = ChatLine { t, kind: kind_name(message.kind), channel: message.channel, sender, text, confidence };
                    info!(
                        "chat: {}{} [{:.2}] {:?}: {:?}",
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

/// The sender (from the first part) and the text (the parts joined: the game wraps mid-word).
fn read(reader: &mut Reader, strip: &Crop, message: &Message) -> Result<(String, String, f32), OcrError> {
    let mut sender = String::new();
    let mut text = String::new();
    let mut confidence = 1.0f32;
    for part in &message.parts {
        if let Some(span) = part.name {
            let r = reader.read(strip, part.top, message.kind, span)?;
            confidence = confidence.min(r.confidence);
            sender = r.text;
        }
        let r = reader.read(strip, part.top, message.kind, part.message)?;
        confidence = confidence.min(r.confidence);
        text.push_str(&r.text);
    }
    Ok((sender, text, confidence))
}
