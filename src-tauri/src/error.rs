use std::fmt;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub enum AppError {
    Io(String),
    Process(String),
    Json(String),
    Pdf(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Io(msg) => write!(f, "IO error: {}", msg),
            AppError::Process(msg) => write!(f, "Process error: {}", msg),
            AppError::Json(msg) => write!(f, "JSON error: {}", msg),
            AppError::Pdf(msg) => write!(f, "PDF error: {}", msg),
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::Io(err.to_string())
    }
}

impl From<String> for AppError {
    fn from(err: String) -> Self {
        AppError::Process(err)
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        AppError::Json(err.to_string())
    }
}

impl From<lopdf::Error> for AppError {
    fn from(err: lopdf::Error) -> Self {
        AppError::Pdf(err.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
