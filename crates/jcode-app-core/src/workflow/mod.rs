//! Optional passive workflow observation. Never executes a command or calls a model.
mod artifact;
mod autospec;
mod observer;
mod registry;
mod store;
#[cfg(test)]
mod store_tests;
use observer::observe;
#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct ObservedLifecycle {
    health: crate::bus::WorkflowHealth,
    detail: Option<String>,
}

pub(super) fn display_text(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !matches!(*ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .take(160)
        .collect()
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct ArtifactProgress {
    pub completed: u32,
    pub total: u32,
    pub stage: Option<String>,
    pub activity: Option<String>,
    pub blocked: bool,
}
