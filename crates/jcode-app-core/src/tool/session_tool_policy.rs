use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, RwLock as StdRwLock};

#[derive(Clone, Debug, Default)]
pub(super) struct SessionToolPolicy {
    pub(super) allowed_tools: Option<HashSet<String>>,
    pub(super) disabled_tools: HashSet<String>,
    owner: Option<u64>,
}

static SESSION_TOOL_POLICIES: LazyLock<StdRwLock<HashMap<String, SessionToolPolicy>>> =
    LazyLock::new(|| StdRwLock::new(HashMap::new()));
static NEXT_SESSION_TOOL_POLICY_OWNER: AtomicU64 = AtomicU64::new(1);

/// Removes an Agent-owned policy when that Agent actually leaves memory.
///
/// The owner token prevents a stale Agent from removing the policy installed by
/// a successor connection for the same persisted session ID.
pub(crate) struct SessionToolPolicyRegistration {
    session_id: String,
    owner: u64,
}

impl Drop for SessionToolPolicyRegistration {
    fn drop(&mut self) {
        let mut policies = SESSION_TOOL_POLICIES
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if policies
            .get(&self.session_id)
            .is_some_and(|policy| policy.owner == Some(self.owner))
        {
            policies.remove(&self.session_id);
        }
    }
}

pub(crate) fn register_session_tool_policy(
    session_id: &str,
    allowed_tools: Option<HashSet<String>>,
    disabled_tools: HashSet<String>,
) -> SessionToolPolicyRegistration {
    let owner = NEXT_SESSION_TOOL_POLICY_OWNER.fetch_add(1, Ordering::Relaxed);
    let mut policies = SESSION_TOOL_POLICIES
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    policies.insert(
        session_id.to_string(),
        SessionToolPolicy {
            allowed_tools,
            disabled_tools,
            owner: Some(owner),
        },
    );
    SessionToolPolicyRegistration {
        session_id: session_id.to_string(),
        owner,
    }
}

#[cfg(test)]
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
            owner: None,
        },
    );
}

#[cfg(test)]
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
