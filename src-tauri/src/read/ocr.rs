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
                let text = normalize(&self.recognize(&up)?);
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
    fn normalize_collapses_whitespace_and_case() {
        assert_eq!(normalize("  Victoria   Road\nperion "), "victoria road perion");
    }

    #[test]
    fn title_case_capitalizes_words() {
        assert_eq!(title_case("victoria road perion"), "Victoria Road Perion");
        assert_eq!(title_case(""), "");
    }

    /// Real Windows OCR. Verified 2026-10-15: all four variants read this strip as
    /// "victoria road perion" after normalization.
    #[test]
    fn labels_the_fixture() {
        let ocr = MapOcr::new().expect("Windows OCR available");
        let label = ocr.label(&[fixture("map_victoria_road_perion.png")]).expect("OCR runs");
        assert_eq!(label.as_deref(), Some("Victoria Road Perion"));
    }
}
