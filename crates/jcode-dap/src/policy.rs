use crate::error::{DapError, Result};
use serde::Deserialize;
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionTier {
    ReadOnly,
    ProcessControl,
    Evaluate,
    MemoryWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Sessions,
    Status,
    Launch,
    Attach,
    SetBreakpoint,
    RemoveBreakpoint,
    Continue,
    Pause,
    StepOver,
    StepIn,
    StepOut,
    Threads,
    StackTrace,
    Scopes,
    Variables,
    Evaluate,
    Output,
    Stop,
    Disconnect,
    ReadMemory,
    Modules,
    WriteMemory,
}

impl Action {
    pub fn tier(self) -> PermissionTier {
        match self {
            Self::Sessions
            | Self::Status
            | Self::Threads
            | Self::StackTrace
            | Self::Scopes
            | Self::Variables
            | Self::Output
            | Self::Modules => PermissionTier::ReadOnly,
            Self::ReadMemory => PermissionTier::ReadOnly,
            Self::Evaluate => PermissionTier::Evaluate,
            Self::WriteMemory => PermissionTier::MemoryWrite,
            Self::Launch
            | Self::Attach
            | Self::SetBreakpoint
            | Self::RemoveBreakpoint
            | Self::Continue
            | Self::Pause
            | Self::StepOver
            | Self::StepIn
            | Self::StepOut
            | Self::Stop
            | Self::Disconnect => PermissionTier::ProcessControl,
        }
    }
    pub fn required_capability(self) -> Option<&'static str> {
        match self {
            Self::WriteMemory => Some("supportsWriteMemoryRequest"),
            Self::Modules => Some("supportsModulesRequest"),
            Self::ReadMemory => Some("supportsReadMemoryRequest"),
            Self::SetBreakpoint | Self::RemoveBreakpoint => None,
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DapPolicy {
    pub allow_process_control: bool,
    pub allow_evaluate: bool,
    pub allow_memory_write: bool,
    pub request_timeout: Duration,
    pub startup_timeout: Duration,
    pub max_output_bytes: usize,
}

impl Default for DapPolicy {
    fn default() -> Self {
        Self {
            allow_process_control: true,
            allow_evaluate: false,
            allow_memory_write: false,
            request_timeout: Duration::from_secs(30),
            startup_timeout: Duration::from_secs(10),
            max_output_bytes: 128 * 1024,
        }
    }
}

impl DapPolicy {
    pub fn from_toml_layers(layers: &[&str]) -> Result<Self> {
        let mut policy = Self::default();
        for layer in layers {
            let file: PolicyFile =
                toml::from_str(layer).map_err(|error| DapError::Config(error.to_string()))?;
            if let Some(permissions) = file.permissions {
                permissions.apply_all(&mut policy);
            }
        }
        Ok(policy)
    }

    pub fn load(cwd: &Path) -> Result<Self> {
        let (user, project) = crate::config::AdapterRegistry::load_scoped_config_layers(cwd)?;
        let mut policy = Self::default();
        if let Some(user) = user {
            let file: PolicyFile =
                toml::from_str(&user).map_err(|error| DapError::Config(error.to_string()))?;
            if let Some(permissions) = file.permissions {
                permissions.apply_all(&mut policy);
            }
        }
        for project in project {
            let file: PolicyFile =
                toml::from_str(&project).map_err(|error| DapError::Config(error.to_string()))?;
            if let Some(permissions) = file.permissions {
                permissions.apply_narrowing(&mut policy);
            }
        }
        Ok(policy)
    }

    pub fn check(&self, action: Action) -> Result<()> {
        match action.tier() {
            PermissionTier::ReadOnly => Ok(()),
            PermissionTier::ProcessControl if self.allow_process_control => Ok(()),
            PermissionTier::Evaluate if self.allow_evaluate => Ok(()),
            PermissionTier::MemoryWrite if self.allow_memory_write => Ok(()),
            tier => Err(DapError::Permission(format!("{tier:?} action is disabled"))),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct PolicyFile {
    permissions: Option<PermissionConfig>,
}

#[derive(Debug, Default, Deserialize)]
struct PermissionConfig {
    allow_process_control: Option<bool>,
    allow_evaluate: Option<bool>,
    allow_memory_write: Option<bool>,
    request_timeout_ms: Option<u64>,
    startup_timeout_ms: Option<u64>,
    max_output_bytes: Option<usize>,
}

impl PermissionConfig {
    fn apply_all(self, policy: &mut DapPolicy) {
        if let Some(value) = self.allow_process_control {
            policy.allow_process_control = value;
        }
        if let Some(value) = self.allow_evaluate {
            policy.allow_evaluate = value;
        }
        if let Some(value) = self.allow_memory_write {
            policy.allow_memory_write = value;
        }
        if let Some(value) = self.request_timeout_ms {
            policy.request_timeout = Duration::from_millis(value.max(1));
        }
        if let Some(value) = self.startup_timeout_ms {
            policy.startup_timeout = Duration::from_millis(value.max(1));
        }
        if let Some(value) = self.max_output_bytes {
            policy.max_output_bytes = value.max(1);
        }
    }

    fn apply_narrowing(self, policy: &mut DapPolicy) {
        if self.allow_process_control == Some(false) {
            policy.allow_process_control = false;
        }
        if self.allow_evaluate == Some(false) {
            policy.allow_evaluate = false;
        }
        if self.allow_memory_write == Some(false) {
            policy.allow_memory_write = false;
        }
        if let Some(value) = self.request_timeout_ms {
            policy.request_timeout = policy
                .request_timeout
                .min(Duration::from_millis(value.max(1)));
        }
        if let Some(value) = self.startup_timeout_ms {
            policy.startup_timeout = policy
                .startup_timeout
                .min(Duration::from_millis(value.max(1)));
        }
        if let Some(value) = self.max_output_bytes {
            policy.max_output_bytes = policy.max_output_bytes.min(value.max(1));
        }
    }
}
