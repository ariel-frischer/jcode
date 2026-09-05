//! Fork-specific tasks.yaml adapter. No CLI execution, session discovery, or provider logic.
use super::ArtifactProgress;
use anyhow::{Result, bail};
use serde::Deserialize;
use std::collections::HashSet;

#[derive(Deserialize)]
struct TasksFile {
    phases: Vec<Phase>,
}

#[derive(Deserialize)]
struct Phase {
    number: u32,
    tasks: Vec<Task>,
}

#[derive(Deserialize)]
struct Task {
    id: String,
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
    let document: TasksFile = serde_yaml::from_slice(bytes)
        .map_err(|_| anyhow::anyhow!("Invalid Autospec tasks artifact"))?;
    if document.phases.len() > 256 {
        bail!("Autospec phase limit exceeded");
    }
    let mut progress = ArtifactProgress::default();
    let mut ids = HashSet::new();
    let mut active = None;
    let mut pending = None;
    for phase in &document.phases {
        for task in &phase.tasks {
            if task.id.trim().is_empty() || !ids.insert(task.id.as_str()) {
                bail!("Autospec tasks require unique nonempty IDs");
            }
            progress.total += 1;
            if progress.total > 4096 {
                bail!("Autospec task limit exceeded");
            }
            match task.status {
                Status::Completed => progress.completed += 1,
                Status::Pending => {
                    pending.get_or_insert((phase.number, task));
                }
                Status::InProgress | Status::Blocked => {
                    active.get_or_insert((phase.number, task));
                    progress.blocked |= task.status == Status::Blocked;
                }
            }
        }
    }
    if progress.total == 0 {
        bail!("Autospec task artifact contains no tasks");
    }
    if let Some((phase, task)) = active.or(pending) {
        progress.stage = Some(format!(
            "Implement · phase {phase}/{}",
            document.phases.len()
        ));
        progress.activity = Some(super::display_text(&format!("{}: {}", task.id, task.title)));
    } else {
        // Completed task checkboxes do not establish controller/validation success.
        progress.stage = Some("Implementation tasks complete".into());
    }
    Ok(progress)
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
