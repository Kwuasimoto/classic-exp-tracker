//! Error type crossing the IPC boundary.

use serde::Serialize;

use crate::store::StoreError;

#[derive(Debug, thiserror::Error, Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "camelCase")]
pub enum AppError {
    #[error("storage: {0}")]
    Storage(String),
    #[error("capture: {0}")]
    Capture(String),
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("not found: {0}")]
    NotFound(String),
    /// Reading or writing a file the user picked (glyph library export/import).
    #[error("file: {0}")]
    File(String),
}

impl From<StoreError> for AppError {
    fn from(e: StoreError) -> Self {
        AppError::Storage(e.to_string())
    }
}
