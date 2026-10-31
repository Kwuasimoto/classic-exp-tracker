//! Error type crossing the IPC boundary.

use serde::Serialize;

use crate::store::StoreError;
use crate::translator::TranslatorError;

#[derive(Debug, thiserror::Error, Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "camelCase")]
pub enum AppError {
    #[error("storage: {0}")]
    Storage(String),
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("window: {0}")]
    Window(String),
    #[error("translator: {0}")]
    Translator(String),
}

impl From<StoreError> for AppError {
    fn from(e: StoreError) -> Self {
        AppError::Storage(e.to_string())
    }
}

impl From<TranslatorError> for AppError {
    fn from(e: TranslatorError) -> Self {
        match e {
            TranslatorError::UnknownLanguage(_) => AppError::NotFound(e.to_string()),
            _ => AppError::Translator(e.to_string()),
        }
    }
}
