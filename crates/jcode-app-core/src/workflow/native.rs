//! Bounded native evidence, independent of server member and adapter schemas.
use super::{observer::is_terminal, registry::Registry};
use crate::bus::{WorkflowHealth, WorkflowSnapshot, observed_health};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub(super) const MAX_NATIVE: usize = 256;

#[derive(Clone)]
pub(crate) struct NativeSample {
    pub session_id: String,
    pub owner: Option<String>,
    pub working_dir: Option<PathBuf>,
    pub started_at: u64,
    pub allow_registration: bool,
    pub label: String,
    pub health: WorkflowHealth,
    pub detail: Option<String>,
    pub activity: Option<String>,
    pub activity_at: Option<u64>,
    pub checkpoint_at: Option<u64>,
    pub progress: Option<(u32, u32)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct NativeRecord {
    pub session_id: String,
    pub owner: String,
    pub registration_id: Option<String>,
    pub started_at: u64,
    pub activity_at: Option<u64>,
    pub checkpoint_at: Option<u64>,
    pub terminal_at: Option<u64>,
    pub snapshot: WorkflowSnapshot,
}

impl NativeRecord {
    pub fn snapshot(&self, now: u64, quiet_seconds: u64) -> WorkflowSnapshot {
        let mut snapshot = self.snapshot.clone();
        snapshot.activity_age_secs = self.activity_at.map(|at| now.saturating_sub(at));
        snapshot.checkpoint_age_secs = self.checkpoint_at.map(|at| now.saturating_sub(at));
        snapshot.health = observed_health(
            snapshot.health,
            Some(now.saturating_sub(self.activity_at.unwrap_or(self.started_at))),
            quiet_seconds,
        );
        snapshot
    }

    pub fn expired(&self, now: u64, retention: u64) -> bool {
        self.terminal_at
            .is_some_and(|at| now.saturating_sub(at) > retention)
    }
}

pub(super) fn update(
    registry: &mut Registry,
    samples: Vec<NativeSample>,
    autospec: bool,
    now: u64,
    retention: u64,
) -> anyhow::Result<()> {
    let present: std::collections::HashSet<_> = samples
        .iter()
        .map(|sample| sample.session_id.as_str())
        .collect();
    let latest: std::collections::HashSet<_> = registry
        .registrations
        .iter()
        .filter_map(|registration| {
            registry
                .native
                .iter()
                .filter(|run| run.registration_id.as_ref() == Some(&registration.id))
                .max_by_key(|run| (run.started_at, &run.session_id))
                .map(|run| run.session_id.clone())
        })
        .collect();
    registry.native.retain(|run| {
        !run.expired(now, retention)
            || present.contains(run.session_id.as_str())
            || latest.contains(&run.session_id)
    });
    for sample in samples.into_iter().take(MAX_NATIVE) {
        let existing = registry
            .native
            .iter()
            .position(|run| run.session_id == sample.session_id);
        let index = if let Some(index) = existing {
            index
        } else {
            // Only new live members in an explicitly registered, exclusive worktree qualify.
            // Explicit parent ownership always wins over worktree association.
            let registration = (autospec && sample.allow_registration)
                .then(|| {
                    registry.registrations.iter().find(|run| {
                        sample.working_dir.as_ref() == Some(&run.working_dir)
                            && sample.started_at >= run.registered_at
                            && sample.session_id != run.owner
                            && sample
                                .owner
                                .as_ref()
                                .is_none_or(|owner| owner == &run.owner)
                    })
                })
                .flatten();
            let owner = sample
                .owner
                .clone()
                .or_else(|| registration.map(|run| run.owner.clone()));
            let Some(owner) = owner.filter(|owner| owner != &sample.session_id) else {
                continue;
            };
            if registry.native.len() >= MAX_NATIVE {
                anyhow::bail!("native workflow retention limit reached");
            }
            registry.native.push(NativeRecord {
                session_id: sample.session_id.clone(),
                owner,
                registration_id: registration.map(|run| run.id.clone()),
                started_at: sample.started_at,
                activity_at: None,
                checkpoint_at: None,
                terminal_at: None,
                snapshot: WorkflowSnapshot {
                    id: format!("native-{}", sample.session_id),
                    source: "native".into(),
                    ..Default::default()
                },
            });
            registry.native.len() - 1
        };
        let run = &mut registry.native[index];
        // A later server observation must not transfer an existing workflow to another owner.
        if sample
            .owner
            .as_ref()
            .is_some_and(|owner| owner != &run.owner)
        {
            continue;
        }
        run.activity_at = run.activity_at.max(sample.activity_at);
        run.checkpoint_at = run.checkpoint_at.max(sample.checkpoint_at);
        run.snapshot.label = super::display_text(&sample.label);
        let keep_terminal = is_terminal(run.snapshot.health)
            && matches!(
                sample.health,
                WorkflowHealth::Waiting | WorkflowHealth::ObserverError
            );
        if !keep_terminal {
            run.snapshot.health = sample.health;
            run.snapshot.detail = sample.detail.map(|text| super::display_text(&text));
        }
        if let Some(activity) = sample.activity {
            run.snapshot.activity = Some(super::display_text(&activity));
        }
        if let Some((done, total)) = sample.progress {
            run.snapshot.completed = Some(done.min(total));
            run.snapshot.total = Some(total);
        }
        if is_terminal(run.snapshot.health) {
            run.terminal_at.get_or_insert(now);
        } else {
            run.terminal_at = None;
        }
    }
    Ok(())
}

pub(super) fn merge_registered(
    snapshot: &mut WorkflowSnapshot,
    native: &NativeRecord,
    now: u64,
    quiet_seconds: u64,
) {
    let evidence = native.snapshot(now, quiet_seconds);
    snapshot.activity_age_secs = match (snapshot.activity_age_secs, evidence.activity_age_secs) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    if snapshot.health == WorkflowHealth::Quiet
        && snapshot
            .activity_age_secs
            .is_some_and(|age| age < quiet_seconds)
    {
        snapshot.health = WorkflowHealth::Running;
    }
    // Adapter task counts/checkpoint and stable workflow identity never reset between native phases.
    // A successful child phase does not establish completion of its whole controller.
    if matches!(
        evidence.health,
        WorkflowHealth::Failed | WorkflowHealth::Blocked | WorkflowHealth::Stopped
    ) {
        snapshot.health = evidence.health;
        snapshot.detail = evidence.detail;
    }
}
