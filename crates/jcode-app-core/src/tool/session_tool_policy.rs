use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, RwLock};

#[derive(Clone, Debug, Default)]
pub(super) struct SessionToolPolicy {
    pub(super) allowed_tools: Option<HashSet<String>>,
    pub(super) disabled_tools: HashSet<String>,
}

static SESSION_TOOL_POLICIES: LazyLock<RwLock<HashMap<String, SessionToolPolicy>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

pub(crate) fn set_session_tool_policy(
    session_id: &str,
    allowed_tools: Option<HashSet<String>>,
    disabled_tools: HashSet<String>,
) {
    let mut policies = SESSION_TOOL_POLICIES
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    policies.insert(
        session_id.to_string(),
        SessionToolPolicy {
            allowed_tools,
            disabled_tools,
        },
    );
}

pub(crate) fn clear_session_tool_policy(session_id: &str) {
    let mut policies = SESSION_TOOL_POLICIES
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    policies.remove(session_id);
}

pub(super) fn session_tool_policy(session_id: &str) -> Option<SessionToolPolicy> {
    SESSION_TOOL_POLICIES
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(session_id)
        .cloned()
}
