//! Windows built-in OCR, used only to *label* a newly confirmed map. Identity is the
//! pixel signature. Single OCR runs are unreliable on this font, so several upscale
//! variants across the confirming frames vote.

use std::collections::HashMap;

use image::imageops::{resize, FilterType};
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;

use crate::geom::Crop;

const VARIANTS: [(u32, FilterType); 4] = [
    (2, FilterType::Nearest),
    (3, FilterType::Nearest),
    (3, FilterType::CatmullRom),
    (3, FilterType::Triangle),
];

#[derive(Debug, thiserror::Error)]
pub enum OcrError {
    #[error("Windows OCR: {0}")]
    Windows(#[from] windows::core::Error),
    #[error("image too large for OCR")]
    TooLarge,
}

pub struct MapOcr {
    engine: OcrEngine,
}

impl MapOcr {
    pub fn new() -> Result<Self, OcrError> {
        Ok(Self { engine: OcrEngine::TryCreateFromUserProfileLanguages()? })
    }

    /// Most-voted normalized text across all strips and variants, title-cased.
    /// `None` when OCR returns no text at all.
    pub fn label(&self, strips: &[Crop]) -> Result<Option<String>, OcrError> {
        let mut votes: HashMap<String, usize> = HashMap::new();
        for strip in strips {
            let img = strip.to_image();
            for (scale, filter) in VARIANTS {
                let up = resize(&img, img.width() * scale, img.height() * scale, filter);
                let text = numeral_to_number(&normalize(&self.recognize(&up)?));
                if !text.is_empty() {
                    *votes.entry(text).or_default() += 1;
                }
            }
        }
        Ok(votes
            .into_iter()
            .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
            .map(|(text, _)| title_case(&text)))
    }

    fn recognize(&self, img: &image::RgbaImage) -> Result<String, OcrError> {
        let mut bgra = img.as_raw().clone();
        for px in bgra.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
        let writer = DataWriter::new()?;
        writer.WriteBytes(&bgra)?;
        let width = i32::try_from(img.width()).map_err(|_| OcrError::TooLarge)?;
        let height = i32::try_from(img.height()).map_err(|_| OcrError::TooLarge)?;
        let bitmap = SoftwareBitmap::CreateCopyFromBuffer(&writer.DetachBuffer()?, BitmapPixelFormat::Bgra8, width, height)?;
        Ok(self.engine.RecognizeAsync(&bitmap)?.join()?.Text()?.to_string_lossy())
    }
}

pub fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Map names end in a Roman numeral ("The Burnt Land II"), which OCR reads with
/// I, l, 1 and | mixed up ("li", "ll", "1i"). A last word made only of those and
/// v/x, holding at least one letter, that spells a valid numeral up to XXXIX
/// becomes a number: "the burnt land li" → "the burnt land 2". Expects
/// `normalize`d text.
pub fn numeral_to_number(text: &str) -> String {
    let Some((head, last)) = text.rsplit_once(' ') else { return text.to_string() };
    if !last.chars().any(|c| c.is_ascii_alphabetic()) {
        return text.to_string();
    }
    let roman: Option<String> = last
        .chars()
        .map(|c| match c {
            'i' | 'l' | '1' | '|' | '!' => Some('I'),
            'v' => Some('V'),
            'x' => Some('X'),
            _ => None,
        })
        .collect();
    match roman.and_then(|r| roman_value(&r)) {
        Some(n) => format!("{head} {n}"),
        None => text.to_string(),
    }
}

/// Value of a canonical Roman numeral from I to XXXIX; `None` for anything else ("IIII", "VX").
fn roman_value(roman: &str) -> Option<u32> {
    const ONES: [&str; 10] = ["", "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX"];
    (1..40).find(|&n: &u32| format!("{}{}", "X".repeat((n / 10) as usize), ONES[(n % 10) as usize]) == roman)
}

pub fn title_case(text: &str) -> String {
    text.split(' ')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::fixture;

    #[test]
    fn reads_roman_numerals_as_numbers() {
        for (ocr, name) in [
            ("the burnt land li", "the burnt land 2"),
            ("the burnt land ll", "the burnt land 2"),
            ("ant tunnel i", "ant tunnel 1"),
            ("sleepy dungeon iv", "sleepy dungeon 4"),
            ("victoria road perion", "victoria road perion"),
            ("forest 11", "forest 11"),
            ("hill iiii", "hill iiii"),
        ] {
            assert_eq!(numeral_to_number(ocr), name);
        }
    }

    #[test]
    fn labels_the_fixture() {
        let ocr = MapOcr::new().expect("Windows OCR available");
        let label = ocr.label(&[fixture("map_victoria_road_perion.png")]).expect("OCR runs");
        assert_eq!(label.as_deref(), Some("Victoria Road Perion"));
    }
}
