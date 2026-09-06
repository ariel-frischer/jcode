//! Display-only workflow contracts. Producers own observation, consumers never poll.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowHealth {
    #[default]
    Running,
    Quiet,
    Waiting,
    Blocked,
    Failed,
    Completed,
    Stopped,
    ObserverError,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkflowSnapshot {
    pub id: String,
    pub label: String,
    pub source: String,
    pub stage: Option<String>,
    pub completed: Option<u32>,
    pub total: Option<u32>,
    pub activity: Option<String>,
    pub activity_age_secs: Option<u64>,
    pub checkpoint_age_secs: Option<u64>,
    pub health: WorkflowHealth,
    /// Bounded observer-authored explanation, never a raw provider error/log.
    pub detail: Option<String>,
}

/// Silence is only a suspicion. Explicit lifecycle outcomes take precedence.
pub fn observed_health(
    lifecycle: WorkflowHealth,
    activity_age_secs: Option<u64>,
    quiet_seconds: u64,
) -> WorkflowHealth {
    if lifecycle == WorkflowHealth::Running
        && activity_age_secs.is_some_and(|age| age >= quiet_seconds)
    {
        WorkflowHealth::Quiet
    } else {
        lifecycle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_is_quiet_not_failure_and_explicit_outcomes_win() {
        assert_eq!(
            observed_health(WorkflowHealth::Running, Some(300), 300),
            WorkflowHealth::Quiet
        );
        assert_eq!(
            observed_health(WorkflowHealth::Running, Some(299), 300),
            WorkflowHealth::Running
        );
        for state in [
            WorkflowHealth::Failed,
            WorkflowHealth::Blocked,
            WorkflowHealth::Completed,
            WorkflowHealth::Waiting,
            WorkflowHealth::ObserverError,
        ] {
            assert_eq!(observed_health(state, Some(900), 300), state);
        }
    }

    #[test]
    fn unknown_activity_is_not_claimed_as_recent() {
        assert_eq!(
            observed_health(WorkflowHealth::Running, None, 300),
            WorkflowHealth::Running
        );
        assert_eq!(WorkflowSnapshot::default().activity_age_secs, None);
    }
}
