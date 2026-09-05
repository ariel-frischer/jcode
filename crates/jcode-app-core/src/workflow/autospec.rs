//! Fork-specific tasks.yaml adapter. No CLI execution, session discovery, or provider logic.
use super::ArtifactProgress;
use anyhow::{Result, bail};
use serde::{
    Deserialize, Deserializer,
    de::{self, SeqAccess, Visitor},
};
use std::collections::HashSet;

#[derive(Deserialize)]
struct TasksFile {
    #[serde(deserialize_with = "parse_phases")]
    phases: ArtifactProgress,
}

#[derive(Deserialize)]
struct Phase {
    number: u32,
    #[serde(deserialize_with = "bounded_tasks")]
    tasks: Vec<Task>,
}

#[derive(Deserialize)]
struct Task {
    #[serde(deserialize_with = "bounded_string")]
    id: String,
    #[serde(deserialize_with = "bounded_string")]
    title: String,
    status: Status,
}

#[derive(Deserialize, PartialEq, Eq)]
enum Status {
    Pending,
    InProgress,
    Completed,
    Blocked,
}

#[derive(Deserialize)]
struct ControllerStatus {
    state: ControllerState,
    error_code: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum ControllerState {
    Running,
    Waiting,
    Retrying,
    Blocked,
    Failed,
    Completed,
    Stopped,
}

pub(super) fn parse_controller(bytes: &[u8]) -> Result<super::ObservedLifecycle> {
    use crate::bus::WorkflowHealth;
    let status: ControllerStatus = serde_json::from_slice(bytes)
        .map_err(|_| anyhow::anyhow!("Invalid workflow controller artifact"))?;
    let (health, detail) = match status.state {
        ControllerState::Running => (WorkflowHealth::Running, None),
        ControllerState::Waiting => (WorkflowHealth::Waiting, Some("Waiting")),
        ControllerState::Retrying => (WorkflowHealth::Waiting, Some("Retrying")),
        ControllerState::Blocked => (WorkflowHealth::Blocked, Some("Blocked")),
        ControllerState::Failed => (
            WorkflowHealth::Failed,
            Some(match status.error_code.as_deref() {
                Some("insufficient_quota" | "credit_balance_too_low" | "credits_exhausted") => {
                    "Credits exhausted"
                }
                Some("rate_limit_exceeded") => "Rate limit failure",
                Some("authentication_error" | "invalid_api_key") => "Authentication failure",
                _ => "Workflow failed",
            }),
        ),
        ControllerState::Completed => (WorkflowHealth::Completed, None),
        ControllerState::Stopped => (WorkflowHealth::Stopped, Some("Stopped")),
    };
    Ok(super::ObservedLifecycle {
        health,
        detail: detail.map(str::to_owned),
    })
}

pub(super) fn parse_tasks(bytes: &[u8]) -> Result<ArtifactProgress> {
    if bytes.len() as u64 > super::registry::MAX_ARTIFACT_BYTES {
        bail!("Autospec artifact byte limit exceeded");
    }
    let document: TasksFile = serde_yaml::from_slice(bytes)
        .map_err(|_| anyhow::anyhow!("Invalid Autospec tasks artifact"))?;
    Ok(document.phases)
}

fn bounded_string<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<String, D::Error> {
    struct Text;
    impl<'de> Visitor<'de> for Text {
        type Value = String;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a string of at most 1024 bytes")
        }
        fn visit_str<E: de::Error>(self, value: &str) -> std::result::Result<String, E> {
            if value.len() > 1024 {
                return Err(E::custom("Autospec string limit exceeded"));
            }
            Ok(value.to_owned())
        }
    }
    deserializer.deserialize_str(Text)
}

fn bounded_tasks<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Vec<Task>, D::Error> {
    struct Tasks;
    impl<'de> Visitor<'de> for Tasks {
        type Value = Vec<Task>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("at most 4096 tasks")
        }
        fn visit_seq<A: SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut tasks = Vec::new(); // Never trust a producer-controlled size_hint.
            while tasks.len() < 4096 {
                let Some(task) = seq.next_element()? else {
                    return Ok(tasks);
                };
                tasks.push(task);
            }
            if seq.next_element::<de::IgnoredAny>()?.is_some() {
                return Err(de::Error::custom("Autospec task limit exceeded"));
            }
            Ok(tasks)
        }
    }
    deserializer.deserialize_seq(Tasks)
}

fn parse_phases<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<ArtifactProgress, D::Error> {
    struct Phases;
    impl<'de> Visitor<'de> for Phases {
        type Value = ArtifactProgress;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("at most 256 phases with 4096 total tasks")
        }
        fn visit_seq<A: SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            // One bounded phase is materialized at a time, including when YAML aliases repeat it.
            let mut progress = ArtifactProgress::default();
            let mut ids = HashSet::new();
            let mut active = None;
            let mut pending = None;
            let mut phase_count = 0;
            while phase_count < 256 {
                let Some(phase) = seq.next_element::<Phase>()? else {
                    break;
                };
                phase_count += 1;
                for task in phase.tasks {
                    if task.id.trim().is_empty() || !ids.insert(task.id.clone()) {
                        return Err(de::Error::custom(
                            "Autospec tasks require unique nonempty IDs",
                        ));
                    }
                    progress.total += 1;
                    if progress.total > 4096 {
                        return Err(de::Error::custom("Autospec task limit exceeded"));
                    }
                    match task.status {
                        Status::Completed => progress.completed += 1,
                        Status::Pending => {
                            pending.get_or_insert((phase.number, task));
                        }
                        Status::InProgress | Status::Blocked => {
                            progress.blocked |= task.status == Status::Blocked;
                            active.get_or_insert((phase.number, task));
                        }
                    }
                }
            }
            if phase_count == 256 && seq.next_element::<de::IgnoredAny>()?.is_some() {
                return Err(de::Error::custom("Autospec phase limit exceeded"));
            }
            if progress.total == 0 {
                return Err(de::Error::custom(
                    "Autospec task artifact contains no tasks",
                ));
            }
            if let Some((phase, task)) = active.or(pending) {
                progress.stage = Some(format!("Implement · phase {phase}/{}", phase_count));
                progress.activity =
                    Some(super::display_text(&format!("{}: {}", task.id, task.title)));
            } else {
                // Completed task checkboxes do not establish controller/validation success.
                progress.stage = Some("Implementation tasks complete".into());
            }
            Ok(progress)
        }
    }
    deserializer.deserialize_seq(Phases)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TASKS: &str = "phases:\n- number: 1\n  tasks:\n  - id: T001\n    title: contract\n    status: Completed\n- number: 2\n  tasks:\n  - id: T002\n    title: adapter validation\n    status: InProgress\n  - id: T003\n    title: polish\n    status: Pending\n";

    #[test]
    fn autospec_reports_actual_counts_phase_and_active_task() {
        let progress = parse_tasks(TASKS.as_bytes()).unwrap();
        assert_eq!((progress.completed, progress.total), (1, 3));
        assert_eq!(progress.stage.as_deref(), Some("Implement · phase 2/2"));
        assert_eq!(
            progress.activity.as_deref(),
            Some("T002: adapter validation")
        );
    }

    #[test]
    fn autospec_bounds_strings_and_alias_expansion_before_collection() {
        assert!(parse_tasks(TASKS.replace("contract", &"x".repeat(1025)).as_bytes()).is_err());
        let aliases = format!(
            "phases:\n- number: 1\n  tasks:\n  - &task {{id: T1, title: ok, status: Pending}}\n{}",
            "  - *task\n".repeat(4096)
        );
        assert!(parse_tasks(aliases.as_bytes()).is_err());
        let phases = format!(
            "phases:\n- &phase {{number: 1, tasks: []}}\n{}",
            "- *phase\n".repeat(256)
        );
        assert!(parse_tasks(phases.as_bytes()).is_err());
        assert!(parse_tasks(&vec![b' '; 512 * 1024 + 1]).is_err());
    }

    #[test]
    fn autospec_rejects_partial_and_unknown_status_instead_of_claiming_success() {
        assert!(parse_tasks(b"phases: [").is_err());
        assert!(parse_tasks(b"unrelated: document").is_err());
        assert!(parse_tasks(TASKS.replace("InProgress", "mystery").as_bytes()).is_err());
    }

    #[test]
    fn autospec_task_completion_is_not_controller_completion() {
        let progress = parse_tasks(
            TASKS
                .replace("InProgress", "Completed")
                .replace("Pending", "Completed")
                .as_bytes(),
        )
        .unwrap();
        assert_eq!((progress.completed, progress.total), (3, 3));
        assert_eq!(
            progress.stage.as_deref(),
            Some("Implementation tasks complete")
        );
    }
}
