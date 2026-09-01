use serde::{Deserialize, Serialize};

/// File mention completion behavior for the TUI composer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FileMentionsConfig {
    /// Whether typing `@` enables filesystem-backed file suggestions.
    pub enabled: bool,
    /// Additional gitignore-style path patterns excluded from `@` completion.
    pub ignore: Vec<String>,
}

impl Default for FileMentionsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            ignore: Vec::new(),
        }
    }
}
