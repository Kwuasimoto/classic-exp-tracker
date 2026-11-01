//! Error type crossing the IPC boundary.

use serde::Serialize;

use crate::credential::CredentialError;
use crate::gemini::GeminiError;
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
    #[error("update: {0}")]
    Update(String),
}

impl From<tauri_plugin_updater::Error> for AppError {
    fn from(e: tauri_plugin_updater::Error) -> Self {
        AppError::Update(e.to_string())
    }
}

impl From<StoreError> for AppError {
    fn from(e: StoreError) -> Self {
        AppError::Storage(e.to_string())
    }
}

impl From<TranslatorError> for AppError {
    fn from(e: TranslatorError) -> Self {
        AppError::Translator(e.to_string())
    }
}

impl From<GeminiError> for AppError {
    fn from(e: GeminiError) -> Self {
        AppError::Translator(e.to_string())
    }
}

impl From<CredentialError> for AppError {
    fn from(e: CredentialError) -> Self {
        AppError::Translator(e.to_string())
    }
}
