//! Reads a chat line's text with a PaddleOCR PP-OCRv5 reader (translator.rs downloads the
//! readers and the ONNX Runtime engine). The chat font is drawn in exact colours, so a line
//! is handed over as black text on white: only its kind's ink, nothing that shows through
//! behind it. Measured on 39 real lines (2026-11-01): 0.4% of characters wrong with the
//! Latin reader, 32 lines exact; about 30 ms a line on 2 threads.

use std::path::Path;

use ort::environment::Environment;
use ort::session::Session;
use ort::value::Tensor;

use super::chat::{is_ink, LineKind};
use crate::geom::Crop;

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

    /// Reads columns `span` (strip columns [start, end)) of band `top`, holding a line of `kind`.
    pub fn read(&mut self, strip: &Crop, top: u32, kind: LineKind, span: (u32, u32)) -> Result<Text, OcrError> {
        let (input, width) = black_on_white(strip, top, kind, span);
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
        let mut text = String::new();
        let mut confidence = 1.0f32;
        let mut previous = 0;
        for step in probs.chunks_exact(classes) {
            let (best, p) = step.iter().copied().enumerate().fold((0, f32::MIN), |b, (i, p)| if p > b.1 { (i, p) } else { b });
            if best != 0 && best != previous {
                match self.chars.get(best - 1) {
                    Some(c) => text.push_str(c),
                    None if space => text.push(' '),
                    None => return Err(OcrError::Dictionary(format!("class {best} outside the dictionary"))),
                }
                confidence = confidence.min(p);
            }
            previous = best;
        }
        Ok(Text { text, confidence })
    }
}

/// The reader's input for a span: its kind's ink black (the bottom rows' fade allowed for),
/// everything else white, padded and doubled to INPUT_H rows, normalised to -1..1 in three
/// equal channels. Returns the pixels and the width.
fn black_on_white(strip: &Crop, top: u32, kind: LineKind, (start, end): (u32, u32)) -> (Vec<f32>, u32) {
    let rows = ROWS.1 - ROWS.0;
    let width = (end - start + 2 * PAD) * SCALE;
    let height = (rows + 2 * PAD) * SCALE;
    debug_assert_eq!(height, INPUT_H);
    let plane = (width * height) as usize;
    let mut input = vec![1.0f32; plane * 3];
    for r in 0..rows {
        for c in 0..end - start {
            let (x, y) = (start + c, top + ROWS.0 + r);
            if y < strip.height && kind.inks().iter().any(|&ink| is_ink(strip, x, y, ink)) {
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
    /// text), read whole with the Latin reader: no worse than measured on 2026-11-01.
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
            let span = (spans.name.map_or(spans.message.0, |n| n.0), spans.message.1);
            let got = reader.read(&strip, top, kind, span).expect("read");
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
