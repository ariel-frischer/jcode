pub mod config;
pub mod error;
pub mod policy;
pub mod protocol;
pub mod session;
pub mod transport;

pub use config::{AdapterConfig, AdapterRegistry, ResolvedAdapter, TransportMode};
pub use error::{DapError, Result};
pub use policy::{Action, DapPolicy, PermissionTier};
pub use protocol::{DapEventMessage, DapRequestMessage, DapResponseMessage};
pub use session::{
    AttachRequest, DapSessionManager, LaunchRequest, SessionId, SessionSnapshot, SessionStatus,
};

#[cfg(test)]
mod config_tests;
#[cfg(test)]
mod policy_tests;
#[cfg(test)]
mod protocol_tests;
#[cfg(test)]
mod session_tests;
#[cfg(test)]
mod transport_tests;
