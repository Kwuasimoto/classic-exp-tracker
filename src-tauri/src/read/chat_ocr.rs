//! Reads a chat line's text with a PaddleOCR PP-OCRv5 reader (translator.rs downloads the
//! readers and the ONNX Runtime engine). The chat font is drawn in exact colours, so a line
//! is handed over as black text on white: only its kind's ink, nothing that shows through
//! behind it. Measured on 39 real lines with the Latin reader, each read in one pass
//! (2026-11-02): 0.4% of characters wrong, 32 lines exact (0.7% with the name and the
//! message read apart); about 30 ms a line on 2 threads.

use std::path::Path;

use ort::environment::Environment;
use ort::session::Session;
use ort::value::Tensor;

use super::chat::{brackets, is_ink, LineKind, BRACKET_W};
use crate::geom::Crop;
use crate::tracker::chat::Part;

/// The readers' input height; a 16 px band padded by 4 and doubled fills it exactly.
const INPUT_H: u32 = 48;
const PAD: u32 = 4;
const SCALE: u32 = 2;
/// The band's rows handed over: +1 to +16 (capitals from +4, descenders to +16).
const ROWS: (u32, u32) = (1, 17);
/// Threads per reader: nearly as fast as 4 (30 against 25 ms a line), leaving the rest
/// of the CPU to the game.
const THREADS: usize = 2;

#[derive(Debug, thiserror::Error)]
pub enum OcrError {
    #[error("couldn't load the OCR engine: {0}")]
    Load(#[from] ort::LoadDynamicError),
    #[error("OCR engine: {0}")]
    Ort(#[from] ort::Error),
    #[error("OCR engine options: {0}")]
    Builder(String),
    #[error("reader files: {0}")]
    Io(#[from] std::io::Error),
    #[error("reader dictionary: {0}")]
    Dictionary(String),
}

/// ONNX Runtime, loaded once from the downloaded DLL.
pub struct Engine {
    env: Environment,
}

impl Engine {
    pub fn load(dll: &Path) -> Result<Engine, OcrError> {
        Ok(Engine { env: ort::init_from(dll)?.with_name("chat-reader").build()? })
    }
}

/// One PaddleOCR reader: its model and its dictionary (index i + 1 of the output is entry i;
/// 0 is CTC's blank).
pub struct Reader {
    session: Session,
    chars: Vec<String>,
}

/// A line's text and how sure the reader was: the lowest probability of any character.
#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    pub text: String,
    pub confidence: f32,
}

/// A whole message as read.
#[derive(Debug)]
pub struct Line {
    /// "hh:mm" when the player has the game's timestamps on and its digits read as a time.
    pub time: Option<String>,
    /// From the first part; empty on notices and crate announcements.
    pub sender: String,
    /// The parts joined without a space: the game wraps mid-word.
    pub text: String,
    /// The lowest of any character's.
    pub confidence: f32,
}

impl Reader {
    /// The reader in `dir` (inference.onnx and inference.yml, as downloaded).
    pub fn load(engine: &Engine, dir: &Path) -> Result<Reader, OcrError> {
        let chars = dictionary(&std::fs::read_to_string(dir.join("inference.yml"))?)?;
        let session = Session::builder(&engine.env)?
            .with_intra_threads(THREADS)
            .map_err(|e| OcrError::Builder(e.to_string()))?
            .commit_from_file(dir.join("inference.onnx"))?;
        Ok(Reader { session, chars })
    }

    /// Reads a message of `kind` from its parts (tracker::chat).
    pub fn read_line(&mut self, strip: &Crop, kind: LineKind, parts: &[Part]) -> Result<Line, OcrError> {
        let mut line = Line { time: None, sender: String::new(), text: String::new(), confidence: 1.0 };
        for part in parts {
            if let Some((start, end)) = part.time {
                // Only the digits and ':' between the brackets, each 2 px wide.
                let r = self.read(strip, part.top, kind.inks(), (start + 2, end - 2))?;
                line.time = clock(&r.text);
                if line.time.is_none() {
                    log::debug!("chat reader: a timestamp read as {:?}", r.text);
                }
            }
            let (start, end) = part.message;
            // The sender and the message in one pass, which reads better than apart: what's
            // left of the message's first column (less half the gap after the ':') is the
            // sender, its ':' dropped.
            let (chars, confidence) = self.read_marked(strip, part.top, kind, (part.name.map_or(start, |n| n.0), end))?;
            line.confidence = line.confidence.min(confidence);
            let boundary = start as f32 - MESSAGE_SLACK;
            let (head, message): (Vec<_>, Vec<_>) = chars.into_iter().partition(|&(_, x)| x < boundary);
            if part.name.is_some() {
                line.sender = joined(head).trim().trim_end_matches(':').trim_end().to_string();
            }
            line.text.push_str(joined(message).trim());
        }
        Ok(line)
    }

    /// Reads columns `span` of band `top`, holding a line of `kind`, in one pass (letters are
    /// read better with their neighbours), then sets each item name's bracket where its pixels
    /// are (chat::brackets): kept if the reader read it; otherwise the character read there,
    /// if it's one a bracket is misread as ("g]" reads as "gl"), becomes the bracket; with
    /// none there, the bracket is added. Each character comes with its column.
    fn read_marked(&mut self, strip: &Crop, top: u32, kind: LineKind, span: (u32, u32)) -> Result<(Vec<(String, f32)>, f32), OcrError> {
        let (mut chars, confidence) = self.read_at(strip, top, kind.inks(), span)?;
        for b in brackets(strip, top, kind, span) {
            let x = b.x as f32 + BRACKET_W as f32 / 2.0;
            let bracket = if b.open { "[" } else { "]" }.to_string();
            let nearest = |chars: &[(String, f32)], reach: f32, like: &[&str]| {
                chars
                    .iter()
                    .enumerate()
                    .filter(|(_, (c, cx))| (cx - x).abs() <= reach && like.contains(&c.as_str()))
                    .min_by(|a, b| (a.1 .1 - x).abs().total_cmp(&(b.1 .1 - x).abs()))
                    .map(|(i, _)| i)
            };
            if nearest(&chars, READ_BRACKET_REACH, &[bracket.as_str()]).is_some() {
                continue;
            }
            match nearest(&chars, BRACKET_REACH, &BRACKET_LIKE) {
                Some(i) => chars[i] = (bracket, x),
                None => {
                    let at = chars.partition_point(|&(_, cx)| cx < x);
                    chars.insert(at, (bracket, x));
                }
            }
        }
        Ok((chars, confidence))
    }

    /// Reads columns `span` (strip columns [start, end)) of band `top`: the pixels in `inks`.
    pub fn read(&mut self, strip: &Crop, top: u32, inks: &[[u8; 3]], span: (u32, u32)) -> Result<Text, OcrError> {
        let (chars, confidence) = self.read_at(strip, top, inks, span)?;
        Ok(Text { text: chars.into_iter().map(|(c, _)| c).collect(), confidence })
    }

    /// Each character read, with the strip column at its middle, and the lowest confidence.
    fn read_at(&mut self, strip: &Crop, top: u32, inks: &[[u8; 3]], span: (u32, u32)) -> Result<(Vec<(String, f32)>, f32), OcrError> {
        let (input, width) = black_on_white(strip, top, inks, span);
        let tensor = Tensor::from_array(([1usize, 3, INPUT_H as usize, width as usize], input))?;
        let outputs = self.session.run(ort::inputs![tensor])?;
        let (shape, probs) = outputs[0].try_extract_tensor::<f32>()?;
        let classes = usize::try_from(*shape.last().unwrap_or(&0)).unwrap_or(0);
        // The output has the dictionary, CTC's blank and (when the model reads spaces) a space.
        let space = match classes.checked_sub(self.chars.len()) {
            Some(1) => false,
            Some(2) => true,
            _ => return Err(OcrError::Dictionary(format!("{} classes for {} characters", classes, self.chars.len()))),
        };
        let steps = probs.len() / classes.max(1);
        // Input columns per output step, and a step's middle as a strip column.
        let stride = width as f32 / steps.max(1) as f32;
        let column = |first: usize, last: usize| span.0 as f32 + ((first + last + 1) as f32 * stride / 2.0) / SCALE as f32 - PAD as f32;
        // (character, its first step, its last step)
        let mut read: Vec<(String, usize, usize)> = Vec::new();
        let mut confidence = 1.0f32;
        let mut previous = 0;
        for (i, step) in probs.chunks_exact(classes).enumerate() {
            let (best, p) = step.iter().copied().enumerate().fold((0, f32::MIN), |b, (i, p)| if p > b.1 { (i, p) } else { b });
            if best != 0 && best == previous {
                if let Some(last) = read.last_mut() {
                    last.2 = i;
                }
            } else if best != 0 {
                let c = match self.chars.get(best - 1) {
                    Some(c) => c.clone(),
                    None if space => " ".to_string(),
                    None => return Err(OcrError::Dictionary(format!("class {best} outside the dictionary"))),
                };
                read.push((c, i, i));
                confidence = confidence.min(p);
            }
            previous = best;
        }
        Ok((read.into_iter().map(|(c, first, last)| (c, column(first, last))).collect(), confidence))
    }
}

/// "hh:mm" from a timestamp's digits as read ("11: 12" too), or `None` if they aren't a time.
fn clock(read: &str) -> Option<String> {
    let s: String = read.chars().filter(|c| !c.is_whitespace()).collect();
    let (h, m) = s.split_once(':')?;
    let digits = |d: &str, len: std::ops::RangeInclusive<usize>| len.contains(&d.len()) && d.bytes().all(|b| b.is_ascii_digit());
    if !(digits(h, 1..=2) && digits(m, 2..=2)) || h.parse::<u32>().ok()? > 23 || m.parse::<u32>().ok()? > 59 {
        return None;
    }
    Some(format!("{h:0>2}:{m}"))
}

/// How far left of the message's first column a character can be placed and still be the
/// message's: a character's column is its CTC steps' middle (a step is 4 px), and the ':'
/// before a message is 5 or more px left of it.
const MESSAGE_SLACK: f32 = 2.5;

/// What the reader has read an item name's bracket as, and how far (in columns) from the
/// bracket's middle it may be placed: a character's column is its CTC steps' middle.
const BRACKET_LIKE: [&str; 12] = ["[", "]", "l", "I", "|", "1", "i", "j", "(", ")", "{", "}"];
const BRACKET_REACH: f32 = 3.0;
/// A bracket the reader did read was placed up to 4.4 columns from its pixels (55 item names
/// in the watch frames, 2026-11-02).
const READ_BRACKET_REACH: f32 = 5.0;

fn joined(chars: Vec<(String, f32)>) -> String {
    chars.into_iter().map(|(c, _)| c).collect()
}

/// The reader's input for a span: `inks` black (the bottom rows' fade allowed for),
/// everything else white, padded and doubled to INPUT_H rows, normalised to -1..1 in three
/// equal channels. Returns the pixels and the width.
fn black_on_white(strip: &Crop, top: u32, inks: &[[u8; 3]], (start, end): (u32, u32)) -> (Vec<f32>, u32) {
    let rows = ROWS.1 - ROWS.0;
    let width = (end - start + 2 * PAD) * SCALE;
    let height = (rows + 2 * PAD) * SCALE;
    debug_assert_eq!(height, INPUT_H);
    let plane = (width * height) as usize;
    let mut input = vec![1.0f32; plane * 3];
    for r in 0..rows {
        for c in 0..end - start {
            let (x, y) = (start + c, top + ROWS.0 + r);
            if y < strip.height && inks.iter().any(|&ink| is_ink(strip, x, y, ink)) {
                for dy in 0..SCALE {
                    for dx in 0..SCALE {
                        let i = (((PAD + r) * SCALE + dy) * width + (PAD + c) * SCALE + dx) as usize;
                        for channel in 0..3 {
                            input[channel * plane + i] = -1.0;
                        }
                    }
                }
            }
        }
    }
    (input, width)
}

/// The reader's characters from its inference.yml: the `character_dict` list that ends the
/// file, each entry plain or single-quoted ('' for a quote). Checked against a YAML parser on
/// all four readers (2026-11-01).
fn dictionary(yml: &str) -> Result<Vec<String>, OcrError> {
    let mut lines = yml.lines().skip_while(|l| l.trim_end() != "  character_dict:");
    if lines.next().is_none() {
        return Err(OcrError::Dictionary("no character_dict".into()));
    }
    let chars: Vec<String> = lines
        .map_while(|l| l.strip_prefix("  - "))
        .map(|v| {
            let v = v.trim_end_matches([' ', '\r']);
            match v.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
                Some(quoted) => quoted.replace("''", "'"),
                None => v.to_string(),
            }
        })
        .collect();
    if chars.is_empty() {
        return Err(OcrError::Dictionary("empty character_dict".into()));
    }
    Ok(chars)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read::chat::{classify, classify_closed, split, CLOSED_BAND, STRIP};

    #[test]
    fn reads_a_timestamp_as_a_time() {
        assert_eq!(clock("11:10").as_deref(), Some("11:10"));
        assert_eq!(clock("11: 12").as_deref(), Some("11:12"));
        assert_eq!(clock("9:05").as_deref(), Some("09:05"));
        assert_eq!(clock("Notice"), None);
        assert_eq!(clock("11:1O"), None);
        assert_eq!(clock("25:00"), None);
    }

    #[test]
    fn reads_the_dictionary_entries() {
        let yml = "PostProcess:\n  name: CTCLabelDecode\n  character_dict:\n  - '0'\n  - a\n  - ''''\n  - '\"'\n  - 　\n";
        assert_eq!(dictionary(yml).unwrap(), ["0", "a", "'", "\"", "　"]);
        assert!(dictionary("Global:\n  x: 1\n").is_err());
    }

    fn levenshtein(a: &str, b: &str) -> usize {
        let b: Vec<char> = b.chars().collect();
        let mut row: Vec<usize> = (0..=b.len()).collect();
        for (i, ca) in a.chars().enumerate() {
            let mut diag = row[0];
            row[0] = i + 1;
            for (j, &cb) in b.iter().enumerate() {
                let next = (row[j + 1] + 1).min(row[j] + 1).min(diag + usize::from(ca != cb));
                diag = row[j + 1];
                row[j + 1] = next;
            }
        }
        row[b.len()]
    }

    /// The 39 lines typed out from the frames (truth.txt next to them: file, band's screen y,
    /// text), read as the chat reader does (name and message in one pass, items' brackets set
    /// from their pixels) with the Latin reader: no worse than measured on 2026-11-02.
    #[test]
    #[ignore = "needs CHAT_FRAMES and the downloaded chat reader"]
    fn chat_ocr_matches_the_truth() {
        let frames = std::path::PathBuf::from(std::env::var("CHAT_FRAMES").expect("CHAT_FRAMES"));
        let models = std::path::PathBuf::from(std::env::var("LOCALAPPDATA").expect("LOCALAPPDATA")).join("com.classicexp.tracker/translator");
        let engine = Engine::load(&models.join("ocr-engine/onnxruntime.dll")).expect("engine");
        let mut reader = Reader::load(&engine, &models.join("ocr-reader-latin")).expect("reader");
        let truth = std::fs::read_to_string(frames.join("truth.txt")).expect("truth.txt");
        let (mut errors, mut chars, mut exact, mut lines) = (0, 0, 0, 0);
        let started = std::time::Instant::now();
        for line in truth.lines() {
            let mut fields = line.splitn(3, ' ');
            let (Some(file), Some(y), Some(want)) = (fields.next(), fields.next(), fields.next()) else { continue };
            let img = image::open(frames.join(file)).expect("frame").to_rgba8();
            let strip = Crop::from_image(&img).sub(STRIP).expect("1366x768");
            let top = y.parse::<u32>().expect("y") - STRIP.y;
            let kind = if top == CLOSED_BAND { classify_closed(&strip) } else { classify(&strip, top) }.expect("a line");
            let spans = split(&strip, top, kind).expect("readable").expect("spans");
            let part = Part { top, time: spans.time, name: spans.name, message: spans.message };
            let line = reader.read_line(&strip, kind, &[part]).expect("read");
            let text = if spans.name.is_some() { format!("{} : {}", line.sender, line.text) } else { line.text };
            let got = Text { text, confidence: line.confidence };
            let e = levenshtein(&got.text, want);
            if e > 0 {
                println!("{file} {y} ({:.2}): {want:?} read {:?}", got.confidence, got.text);
            }
            errors += e;
            chars += want.chars().count();
            exact += usize::from(e == 0);
            lines += 1;
        }
        let cer = errors as f64 / chars as f64;
        println!("CER {:.3}% exact {exact}/{lines}, {:?} a line", cer * 100.0, started.elapsed() / lines as u32);
        assert_eq!(lines, 39);
        assert!(cer <= 0.004 && exact >= 32, "CER {cer:.4}, exact {exact}");
    }
}
