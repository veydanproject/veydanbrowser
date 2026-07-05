// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

use serde::Serialize;

#[derive(Debug, thiserror::Error, Serialize)]
#[serde(tag = "code", content = "message", rename_all = "snake_case")]
pub enum AppError {
    #[error("Database error: {0}")]
    Db(String),
    #[error("IO error: {0}")]
    Io(String),
    #[error("Browser error: {0}")]
    Browser(String),
    #[error("Proxy error: {0}")]
    Proxy(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("{0}")]
    Other(String),
}

/// Standard result type for all Tauri commands.
pub type CmdResult<T> = Result<T, AppError>;

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        Self::Db(e.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        Self::Other(e.to_string())
    }
}

impl From<zip::result::ZipError> for AppError {
    fn from(e: zip::result::ZipError) -> Self {
        Self::Io(e.to_string())
    }
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        Self::Other(e.to_string())
    }
}

impl AppError {
    pub fn db(e: impl std::fmt::Display) -> Self {
        Self::Db(e.to_string())
    }
    pub fn io(e: impl std::fmt::Display) -> Self {
        Self::Io(e.to_string())
    }
    pub fn browser(e: impl std::fmt::Display) -> Self {
        Self::Browser(e.to_string())
    }
    pub fn proxy(e: impl std::fmt::Display) -> Self {
        Self::Proxy(e.to_string())
    }
    pub fn not_found(e: impl std::fmt::Display) -> Self {
        Self::NotFound(e.to_string())
    }
    pub fn other(e: impl std::fmt::Display) -> Self {
        Self::Other(e.to_string())
    }
}
