//! One bounded passive observation. Scheduling and native session association belong to the server.
use super::{
    autospec,
    registry::{Registration, read_artifact},
};
use crate::bus::{WorkflowHealth, WorkflowSnapshot, observed_health};

pub(super) fn observe(run: &mut Registration, now: u64, quiet_seconds: u64) -> WorkflowSnapshot {
    // A first observation has no previous snapshot. This is not an error fallback.
    let mut snapshot = run
        .last_good
        .clone()
        .unwrap_or_else(WorkflowSnapshot::default);
    snapshot.id = run.id.clone();
    snapshot.label = super::display_text(&run.label);
    snapshot.source = "autospec".into();
    let mut warning = None;
    match read_artifact(&run.working_dir, &run.tasks_file)
        .and_then(|bytes| autospec::parse_tasks(&bytes))
    {
        Ok(progress) => {
            let changed = snapshot.completed != Some(progress.completed)
                || snapshot.total != Some(progress.total)
                || snapshot.stage != progress.stage
                || snapshot.activity != progress.activity;
            if changed {
                if snapshot.total.is_some() {
                    run.activity_at = Some(now);
                }
                run.checkpoint_at = Some(now);
            }
            snapshot.completed = Some(progress.completed);
            snapshot.total = Some(progress.total);
            snapshot.stage = progress.stage;
            snapshot.activity = progress.activity;
            snapshot.health = if progress.blocked {
                WorkflowHealth::Blocked
            } else {
                WorkflowHealth::Running
            };
        }
        Err(_) => {
            warning = Some("Task artifact missing, unsafe or invalid; showing last known progress")
        }
    }
    if let Some(path) = &run.status_file {
        match read_artifact(&run.working_dir, path)
            .and_then(|bytes| autospec::parse_controller(&bytes))
        {
            Ok(lifecycle) => {
                if run.lifecycle.as_ref() != Some(&lifecycle) {
                    if is_terminal(lifecycle.health) || lifecycle.retrying {
                        run.lifecycle_at = Some(now);
                    }
                    run.terminal_at = None;
                }
                if run
                    .lifecycle
                    .as_ref()
                    .is_some_and(|previous| previous != &lifecycle)
                {
                    run.activity_at = Some(now);
                }
                run.lifecycle = Some(lifecycle);
            }
            Err(_) => {
                warning =
                    Some("Controller artifact missing, unsafe or invalid; status may be stale")
            }
        }
    }
    snapshot.detail = None;
    if let Some(lifecycle) = &run.lifecycle {
        snapshot.health = lifecycle.health;
        snapshot.detail = lifecycle.detail.clone();
    }
    if is_terminal(snapshot.health) {
        run.terminal_at.get_or_insert(now);
    } else {
        run.terminal_at = None;
    }
    // Store source evidence only, not ticking ages or transient observer failures.
    snapshot.activity_age_secs = None;
    snapshot.checkpoint_age_secs = None;
    run.last_good = Some(snapshot.clone());
    snapshot.activity_age_secs = run.activity_at.map(|at| now.saturating_sub(at));
    snapshot.checkpoint_age_secs = run.checkpoint_at.map(|at| now.saturating_sub(at));
    snapshot.health = observed_health(
        snapshot.health,
        Some(now.saturating_sub(run.activity_at.unwrap_or(run.registered_at))),
        quiet_seconds,
    );
    if let Some(warning) = warning {
        if !is_terminal(snapshot.health) {
            snapshot.health = WorkflowHealth::ObserverError;
        }
        snapshot.detail = Some(super::display_text(&match snapshot.detail {
            Some(detail) => format!("{detail}; {warning}"),
            None => warning.into(),
        }));
    }
    snapshot
}

pub(super) fn is_terminal(health: WorkflowHealth) -> bool {
    matches!(
        health,
        WorkflowHealth::Completed | WorkflowHealth::Failed | WorkflowHealth::Stopped
    )
}
