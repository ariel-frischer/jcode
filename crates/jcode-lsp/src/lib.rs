pub mod config;
pub mod diagnostics;
pub mod error;
pub mod protocol;
pub mod session;
pub mod transport;
pub mod writethrough;

pub use config::{PartialServerConfig, ResolvedServer, ServerConfig, ServerRegistry};
pub use error::{LspError, Result};
pub use protocol::{Diagnostic, Position, Range, ServerCapabilities};
pub use session::{
    EditFeedback, LspAction, LspSessionManager, SessionId, SessionSnapshot, SessionStatus,
};

#[cfg(test)]
mod config_tests;
#[cfg(test)]
mod protocol_tests;
#[cfg(test)]
mod transport_tests;
