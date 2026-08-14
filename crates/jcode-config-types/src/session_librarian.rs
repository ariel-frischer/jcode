use serde::{Deserialize, Serialize};

/// Optional persisted route and hard-budget overrides for the session librarian.
///
/// Values remain strings at this persistence boundary so the owning resolver can
/// distinguish omitted settings from explicit empty, malformed, non-positive,
/// or otherwise unsupported values and return an actionable validation error.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct SessionLibrarianConfig {
    /// Provider route used independently from the active session provider.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Model used independently from the active session model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Provider reasoning effort for librarian generation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    /// Maximum admitted provider input tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_input_tokens: Option<String>,
    /// Maximum generated provider output tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<String>,
    /// Maximum provider requests per invocation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_requests: Option<String>,
    /// Maximum approved provider cost in exact decimal USD text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_cost_usd: Option<String>,
    /// Wall-clock deadline for one invocation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadline_seconds: Option<String>,
}

impl SessionLibrarianConfig {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}
