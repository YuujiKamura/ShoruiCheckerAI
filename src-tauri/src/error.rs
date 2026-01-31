use std::fmt;

#[derive(Debug)]
pub enum AppError {
    Io(String),
    Process(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Io(message) => write!(f, "IO error: {}", message),
            AppError::Process(message) => write!(f, "Process error: {}", message),
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

pub type AppResult<T> = Result<T, AppError>;
