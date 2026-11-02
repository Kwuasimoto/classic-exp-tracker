//! New chat lines out of each frame's chat strip: whole messages only (every band of them
//! uncovered, wrapped parts joined), each read once. A message is known by a fingerprint of
//! its text pixels, so scrolling back up or a repeated spam line isn't read again; it's read
//! only once it has looked the same in two frames in a row, so nothing drawn over the chat
//! for a moment gets read. No I/O.

use std::collections::{HashSet, VecDeque};

use crate::geom::Crop;
use crate::read::chat::{self, LineKind, Toggle};
use crate::read::ReadError;

/// Fingerprints remembered: about 15–20 minutes of busy chat. Something seen again after
/// that is read once more, which costs a translation and nothing else.
pub const REMEMBERED: usize = 1000;

/// One band of a message: where its sender's name is (the first band only) and its text, as
/// strip column ranges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Part {
    pub top: u32,
    pub name: Option<(u32, u32)>,
    pub message: (u32, u32),
}

/// A message to read: its kind, its channel (megaphones with a badge) and its bands, the
/// first holding the sender. The game wraps at a fixed width, often mid-word, so the parts'
/// texts are joined without a space.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub kind: LineKind,
    pub channel: Option<u8>,
    pub parts: Vec<Part>,
    pub fingerprint: u64,
}

/// Why something in the frame wasn't read, for the log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Skipped {
    /// A window over the input bar or the toggle button (or no game on screen).
    Hidden,
    /// Something drawn over part of a message's band.
    Covered { top: u32, from: u32, to: u32 },
    /// A wrapped part whose first band has scrolled out of the box.
    Fragment { top: u32 },
    /// A band the reader couldn't split (an unknown badge digit).
    Unreadable { top: u32, error: ReadError },
    /// A line in a colour not known yet.
    UnknownColour { top: u32 },
    /// The bottom message reaches the wrap edge: it may go on below the box.
    MaybeCut { top: u32 },
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Observed {
    /// Messages never read before, oldest first.
    pub new: Vec<Message>,
    pub skipped: Vec<Skipped>,
}

#[derive(Default)]
pub struct ChatTracker {
    seen: VecDeque<u64>,
    seen_set: HashSet<u64>,
    /// The whole messages in the previous frame: one must be seen twice in a row.
    previous: HashSet<u64>,
}

/// Where the '[' of "[Notice]" is: a bar at screen x 293 from band row +4 to +16, serifs at
/// x 294 on rows +4 and +16. Rows +4 to +15 are checked: row +16 of the bottom band is
/// below the box. A notice band without it is a wrapped part (notices wrap without the
/// indent other lines get).
fn starts_with_bracket(strip: &Crop, top: u32, ink: [u8; 3]) -> bool {
    let x = 293 - chat::STRIP.x;
    (4..=15).all(|r| chat::is_ink(strip, x, top + r, ink))
        && chat::is_ink(strip, x + 1, top + 4, ink)
        && !(5..=15).any(|r| chat::is_ink(strip, x + 1, top + r, ink))
}

/// The game wraps a line at a fixed width: a wrapped message's first band runs to screen
/// x 813–815 (every one measured). A bottom message whose last band reaches this may go on
/// below the box (the chat is scrolled up), so it waits until it's pushed up.
const WRAP_EDGE: u32 = 805 - chat::STRIP.x;

/// FNV-1a over the message's kind and every text pixel of its bands (rows +2 to +15, the
/// fade allowed for): the same message gives the same fingerprint wherever it sits. Row +16
/// (a descender's last row) is left out: on the bottom band it's below the box.
fn fingerprint(strip: &Crop, kind: LineKind, tops: &[u32]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |v: u32| {
        for b in v.to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    };
    feed(match kind {
        LineKind::Normal => 1,
        LineKind::Party => 2,
        LineKind::Notice => 3,
        LineKind::Crate => 4,
        LineKind::Megaphone(m) => 10 + m as u32,
    });
    for (i, &top) in tops.iter().enumerate() {
        for r in 2..=15 {
            for x in chat::TEXT_X.0..chat::TEXT_X.1 {
                if kind.inks().iter().any(|&ink| chat::is_ink(strip, x, top + r, ink)) {
                    feed((i as u32) << 24 | r << 16 | x);
                }
            }
        }
    }
    h
}

impl ChatTracker {
    pub fn observe(&mut self, strip: &Crop) -> Observed {
        let mut out = Observed::default();
        let bands: Vec<chat::Band> = match chat::toggle(strip) {
            Toggle::Hidden => {
                self.previous.clear();
                out.skipped.push(Skipped::Hidden);
                return out;
            }
            Toggle::Open => {
                let walk = chat::walk(strip);
                if let chat::Stop::Unknown { top } = walk.stop {
                    out.skipped.push(Skipped::UnknownColour { top });
                }
                walk.bands
            }
            Toggle::Closed => chat::classify_closed(strip)
                .map(|kind| chat::Band { top: chat::CLOSED_BAND, kind, covered: None })
                .into_iter()
                .collect(),
        };
        // Oldest (top) band first: a message's first band, then its wrapped parts below it.
        let mut messages: Vec<(Message, bool)> = Vec::new();
        let mut open: Option<(Message, bool)> = None;
        for band in bands.iter().rev() {
            let (whole, spans) = match chat::split(strip, band.top, band.kind) {
                Ok(Some(spans)) => (true, Some(spans)),
                Ok(None) => (false, None),
                Err(error) => {
                    out.skipped.push(Skipped::Unreadable { top: band.top, error });
                    (false, None)
                }
            };
            let whole = whole && band.covered.is_none();
            if let Some((from, to)) = band.covered {
                out.skipped.push(Skipped::Covered { top: band.top, from, to });
            }
            let continued = spans.is_some_and(|s| s.continued)
                || (band.kind == LineKind::Notice && !starts_with_bracket(strip, band.top, band.kind.inks()[0]));
            let part = spans.map(|s| Part { top: band.top, name: s.name, message: s.message });
            if continued {
                match open.as_mut() {
                    Some((m, ok)) if m.kind == band.kind => {
                        *ok &= whole;
                        if let Some(p) = part {
                            m.parts.push(p);
                        }
                    }
                    _ => {
                        if let Some(done) = open.take() {
                            messages.push(done);
                        }
                        out.skipped.push(Skipped::Fragment { top: band.top });
                    }
                }
                continue;
            }
            if let Some(done) = open.take() {
                messages.push(done);
            }
            let channel = match band.kind {
                LineKind::Megaphone(_) => chat::badge(strip, band.top).ok().flatten().map(|b| b.channel),
                _ => None,
            };
            let parts = part.into_iter().collect();
            open = Some((Message { kind: band.kind, channel, parts, fingerprint: 0 }, whole));
        }
        if let Some(done) = open.take() {
            messages.push(done);
        }
        let mut current = HashSet::new();
        let bottom = chat::band_top(0);
        for (mut message, whole) in messages {
            if !whole || message.parts.is_empty() {
                continue;
            }
            let last = message.parts[message.parts.len() - 1];
            if last.top == bottom && last.message.1 >= WRAP_EDGE {
                out.skipped.push(Skipped::MaybeCut { top: last.top });
                continue;
            }
            let tops: Vec<u32> = message.parts.iter().map(|p| p.top).collect();
            message.fingerprint = fingerprint(strip, message.kind, &tops);
            current.insert(message.fingerprint);
            if self.previous.contains(&message.fingerprint) && !self.seen_set.contains(&message.fingerprint) {
                self.remember(message.fingerprint);
                out.new.push(message);
            }
        }
        self.previous = current;
        out
    }

    fn remember(&mut self, fingerprint: u64) {
        self.seen.push_back(fingerprint);
        self.seen_set.insert(fingerprint);
        if self.seen.len() > REMEMBERED
            && let Some(old) = self.seen.pop_front()
        {
            self.seen_set.remove(&old);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(name: &str) -> Crop {
        let dir = std::env::var("CHAT_FRAMES").expect("set CHAT_FRAMES to the chat frames folder");
        let img = image::open(std::path::Path::new(&dir).join(name)).unwrap_or_else(|e| panic!("{name}: {e}")).to_rgba8();
        Crop::from_image(&img).sub(chat::STRIP).expect("1366x768 frame")
    }

    #[test]
    fn remembers_only_the_latest_fingerprints() {
        let mut t = ChatTracker::default();
        for f in 0..REMEMBERED as u64 + 5 {
            t.remember(f);
        }
        assert_eq!(t.seen.len(), REMEMBERED);
        assert!(!t.seen_set.contains(&4) && t.seen_set.contains(&5));
    }

    /// The 30 frames a second apart, chat scrolled up and back down midway: every message
    /// is read once, none again on the way back down or on a second pass.
    #[test]
    #[ignore = "needs CHAT_FRAMES"]
    fn chat_reads_each_message_once() {
        let frames: Vec<Crop> = (1..=30).map(|i| strip(&format!("seq/f{i:02}.png"))).collect();
        let mut t = ChatTracker::default();
        let mut read = Vec::new();
        for (i, f) in frames.iter().enumerate() {
            let o = t.observe(f);
            println!("f{:02}: {} new, skipped {:?}", i + 1, o.new.len(), o.skipped);
            read.extend(o.new.into_iter().map(|m| m.fingerprint));
        }
        let distinct: HashSet<u64> = read.iter().copied().collect();
        assert_eq!(distinct.len(), read.len(), "a message read twice");
        // Every message in the frames, checked against the pictures with glyph-collect
        // chat-replay (2026-11-01): NMLM's spam twice (two different posts), the rest once.
        assert_eq!(read.len(), 32);
        for f in &frames {
            assert_eq!(t.observe(f).new, Vec::new(), "nothing new on a second pass");
        }
    }

    /// The 20 frames with windows moved over the chat, 3 s apart: only messages whole in two
    /// frames in a row are read (5), and nothing while the input bar is covered.
    #[test]
    #[ignore = "needs CHAT_FRAMES"]
    fn chat_reads_nothing_covered() {
        let mut t = ChatTracker::default();
        let mut read = Vec::new();
        for i in 1..=20 {
            let o = t.observe(&strip(&format!("cover/c{i:02}.png")));
            if (11..=18).contains(&i) {
                assert_eq!(o.skipped, vec![Skipped::Hidden], "c{i:02}");
            }
            read.extend(o.new.into_iter().map(|m| (i, m.channel)));
        }
        assert_eq!(read, vec![(2, Some(3)), (3, Some(13)), (20, Some(9)), (20, None), (20, Some(31))]);
    }
}
