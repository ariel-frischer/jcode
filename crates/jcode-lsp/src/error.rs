use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LspError {
    Io(String),
    Json(String),
    Protocol(String),
    Config(String),
    Server(String),
    Timeout(String),
    Unsupported(String),
    Cancelled,
}

pub type Result<T> = std::result::Result<T, LspError>;

impl Display for LspError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(message) => write!(f, "I/O error: {message}"),
            Self::Json(message) => write!(f, "JSON error: {message}"),
            Self::Protocol(message) => write!(f, "LSP protocol error: {message}"),
            Self::Config(message) => write!(f, "LSP configuration error: {message}"),
            Self::Server(message) => write!(f, "LSP server error: {message}"),
            Self::Timeout(message) => write!(f, "LSP timeout: {message}"),
            Self::Unsupported(message) => write!(f, "LSP capability unavailable: {message}"),
            Self::Cancelled => f.write_str("LSP request cancelled"),
        }
    }
}

impl std::error::Error for LspError {}

impl From<std::io::Error> for LspError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

impl From<serde_json::Error> for LspError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error.to_string())
    }
}

impl From<toml::de::Error> for LspError {
    fn from(error: toml::de::Error) -> Self {
        Self::Config(error.to_string())
    }
}

impl From<toml::ser::Error> for LspError {
    fn from(error: toml::ser::Error) -> Self {
        Self::Config(error.to_string())
    }
}
