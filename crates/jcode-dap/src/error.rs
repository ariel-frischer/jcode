use std::fmt;

#[derive(Debug)]
pub enum DapError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Config(String),
    Protocol(String),
    Timeout(String),
    Cancelled,
    Permission(String),
    Unsupported(String),
    AdapterExited(String),
    Session(String),
}

pub type Result<T> = std::result::Result<T, DapError>;

impl fmt::Display for DapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "DAP I/O error: {error}"),
            Self::Json(error) => write!(f, "DAP JSON error: {error}"),
            Self::Config(message) => write!(f, "DAP configuration error: {message}"),
            Self::Protocol(message) => write!(f, "DAP protocol error: {message}"),
            Self::Timeout(message) => write!(f, "DAP timeout: {message}"),
            Self::Cancelled => write!(f, "DAP operation cancelled"),
            Self::Permission(message) => write!(f, "DAP permission denied: {message}"),
            Self::Unsupported(message) => write!(f, "DAP operation unsupported: {message}"),
            Self::AdapterExited(message) => write!(f, "DAP adapter exited: {message}"),
            Self::Session(message) => write!(f, "DAP session error: {message}"),
        }
    }
}

impl std::error::Error for DapError {}

impl From<std::io::Error> for DapError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for DapError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}
