//! Optional passive workflow observation. Never executes a command or calls a model.
mod artifact;
mod native;
pub(crate) use native::NativeSample;
mod autospec;
mod observer;
mod registry;
mod store;
pub(crate) use registry::ObserveInput;
pub(crate) use store::WorkflowStore;
#[cfg(test)]
mod native_tests;
#[cfg(test)]
mod store_tests;
use observer::observe;
#[cfg(test)]
mod tests;

/// Canonical process-local store shared by tools and the passive server monitor.
/// Disabled observation never resolves storage paths or reads persisted state.
pub(crate) fn global() -> anyhow::Result<&'static WorkflowStore> {
    use std::sync::OnceLock;
    static STORE: OnceLock<Result<WorkflowStore, String>> = OnceLock::new();
    let config = &crate::config::config().workflow;
    if !config.enabled {
        anyhow::bail!("workflow observation is disabled");
    }
    STORE
        .get_or_init(|| {
            let open = || {
                WorkflowStore::open(
                    crate::storage::jcode_dir()?.join("workflow/registry.json"),
                    config.clone(),
                )
            };
            open().map_err(|_: anyhow::Error| {
                "Workflow registry unavailable or owned by another daemon; use that daemon or preserve registry.json before repair. Never delete a live registry.lock".into()
            })
        })
        .as_ref()
        .map_err(|message| anyhow::anyhow!(message.clone()))
}

pub(crate) fn now_seconds() -> u64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(elapsed) => elapsed.as_secs(),
        Err(_) => {
            crate::logging::warn(
                "System clock precedes Unix epoch; workflow timestamps use zero until the clock is corrected",
            );
            0
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct ObservedLifecycle {
    #[serde(default)]
    retrying: bool,
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
