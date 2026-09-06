//! Server-only mapping of authoritative native lifecycle and independent bus clocks.
use super::state::SwarmMember;
use crate::{
    bus::{BusEvent, WorkflowHealth},
    workflow::NativeSample,
};
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
};

pub(super) struct NativeClocks {
    clocks: HashMap<String, Clock>,
    started_at: std::time::Instant,
}

impl Default for NativeClocks {
    fn default() -> Self {
        Self {
            clocks: HashMap::new(),
            started_at: std::time::Instant::now(),
        }
    }
}

#[derive(Default)]
struct Clock {
    activity_at: Option<u64>,
    checkpoint_at: Option<u64>,
    activity: Option<&'static str>,
    todo_hash: Option<u64>,
    progress: Option<(u32, u32)>,
}

impl NativeClocks {
    pub fn record(&mut self, event: &BusEvent, members: &HashMap<String, SwarmMember>, now: u64) {
        let (id, activity) = match event {
            BusEvent::ToolUpdated(event) => (&event.session_id, "Tool activity"),
            BusEvent::TodoUpdated(event) => (&event.session_id, "Task checkpoint"),
            BusEvent::SubagentStatus(event) => (&event.session_id, "Model activity"),
            BusEvent::SwarmOutputTail(event) => (&event.session_id, "Streaming"),
            _ => return,
        };
        let Some(member) = members.get(id) else {
            return;
        };
        if !self.clocks.contains_key(id) && self.clocks.len() >= 256 {
            self.clocks.retain(|key, _| members.contains_key(key));
            if self.clocks.len() >= 256 {
                // Keep the same explicit-parent/session ordering as samples.
                let priority = (member.report_back_to_session_id.is_none(), id.as_str());
                let evict = self
                    .clocks
                    .keys()
                    .filter_map(|key| {
                        let existing = members.get(key)?;
                        let existing_priority =
                            (existing.report_back_to_session_id.is_none(), key.as_str());
                        (existing_priority > priority).then_some((existing_priority, key.clone()))
                    })
                    .max_by(|a, b| a.0.cmp(&b.0))
                    .map(|(_, key)| key);
                let Some(evict) = evict else { return };
                self.clocks.remove(&evict);
            }
        }
        let clock = self.clocks.entry(id.clone()).or_default();
        clock.activity_at = Some(now);
        clock.activity = Some(activity);
        if let BusEvent::TodoUpdated(event) = event {
            let mut hash = std::collections::hash_map::DefaultHasher::new();
            let mut completed = 0;
            let mut total = 0;
            for todo in event.todos.iter().take(4096) {
                todo.id.hash(&mut hash);
                todo.status.hash(&mut hash);
                if todo.status == "completed" {
                    completed += 1;
                }
                total += 1;
            }
            let hash = hash.finish();
            if clock.todo_hash != Some(hash) {
                clock.checkpoint_at = Some(now);
                clock.todo_hash = Some(hash);
            }
            clock.progress = Some((completed, total));
        }
    }

    pub fn samples(
        &mut self,
        members: &HashMap<String, SwarmMember>,
        now: u64,
    ) -> Vec<NativeSample> {
        self.clocks.retain(|id, _| members.contains_key(id));
        let mut members: Vec<_> = members.values().collect();
        members.sort_by_key(|member| {
            (
                member.report_back_to_session_id.is_none(),
                &member.session_id,
            )
        });
        members
            .into_iter()
            .take(256)
            .map(|member| {
                let clock = self.clocks.get(&member.session_id);
                let (health, detail) = lifecycle(&member.status, member.detail.as_deref());
                NativeSample {
                    session_id: member.session_id.clone(),
                    owner: member.report_back_to_session_id.clone(),
                    working_dir: member.working_dir.clone(),
                    started_at: now.saturating_sub(member.joined_at.elapsed().as_secs()),
                    allow_registration: member.joined_at >= self.started_at,
                    label: member
                        .task_label
                        .clone()
                        .or_else(|| member.friendly_name.clone())
                        .unwrap_or_else(|| "Native worker".into()),
                    health,
                    detail: detail.map(str::to_owned),
                    activity: clock.and_then(|clock| clock.activity).map(str::to_owned),
                    activity_at: clock.and_then(|clock| clock.activity_at),
                    checkpoint_at: clock.and_then(|clock| clock.checkpoint_at),
                    progress: clock
                        .and_then(|clock| clock.progress)
                        .or(member.todo_progress),
                }
            })
            .collect()
    }
}

fn lifecycle(status: &str, detail: Option<&str>) -> (WorkflowHealth, Option<&'static str>) {
    match status {
        "failed" => {
            let detail = detail
                .unwrap_or("")
                .chars()
                .take(4096)
                .collect::<String>()
                .to_ascii_lowercase();
            let safe = if [
                "insufficient_quota",
                "credit_balance_too_low",
                "credits_exhausted",
                "credit balance",
                "insufficient credits",
            ]
            .iter()
            .any(|code| detail.contains(code))
            {
                "Credits exhausted"
            } else {
                "Worker failed"
            };
            (WorkflowHealth::Failed, Some(safe))
        }
        "crashed" => (WorkflowHealth::Failed, Some("Worker crashed")),
        "completed" | "done" => (WorkflowHealth::Completed, None),
        "running_stale" => (WorkflowHealth::Quiet, Some("Worker activity is stale")),
        "stopped" => (WorkflowHealth::Stopped, Some("Stopped")),
        "blocked" => (WorkflowHealth::Blocked, Some("Blocked")),
        "ready" | "idle" | "waiting" | "retrying" | "spawned" | "queued" | "pending" | "todo" => {
            (WorkflowHealth::Waiting, Some("Waiting"))
        }
        "running" | "working" => (WorkflowHealth::Running, None),
        _ => (
            WorkflowHealth::ObserverError,
            Some("Unknown worker lifecycle"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_activity_and_checkpoint_clocks_are_independent_and_unknown_members_ignored() {
        let (tx, _) = tokio::sync::mpsc::unbounded_channel();
        let record = serde_json::from_value(serde_json::json!({
            "session_id": "child", "swarm_enabled": true, "status": "running",
            "role": "agent", "is_headless": true, "report_back_to_session_id": "owner"
        }))
        .unwrap();
        let members = HashMap::from([("child".into(), SwarmMember::from_record(record, tx))]);
        let mut clocks = NativeClocks::default();
        let todo = BusEvent::TodoUpdated(crate::bus::TodoEvent {
            session_id: "child".into(),
            todos: vec![crate::todo::TodoItem {
                id: "t1".into(),
                status: "in_progress".into(),
                ..Default::default()
            }],
        });
        clocks.record(&todo, &members, 100);
        clocks.record(&todo, &members, 110);
        clocks.record(
            &BusEvent::SwarmOutputTail(crate::bus::SwarmOutputTail {
                session_id: "child".into(),
                tail: "sensitive output".into(),
            }),
            &members,
            120,
        );
        let sample = clocks.samples(&members, 125).remove(0);
        assert_eq!(sample.activity_at, Some(120));
        assert_eq!(sample.checkpoint_at, Some(100));
        assert_eq!(sample.activity.as_deref(), Some("Streaming"));
        assert_eq!(sample.health, WorkflowHealth::Running);
        clocks.record(
            &BusEvent::SwarmOutputTail(crate::bus::SwarmOutputTail {
                session_id: "unknown".into(),
                tail: "private".into(),
            }),
            &members,
            130,
        );
        assert_eq!(clocks.clocks.len(), 1);
    }

    #[test]
    fn native_failures_are_safe_and_transient_status_does_not_establish_failure() {
        assert_eq!(
            lifecycle("failed", Some("insufficient_quota secret=do-not-display")),
            (WorkflowHealth::Failed, Some("Credits exhausted"))
        );
        assert_eq!(
            lifecycle("running", Some("failed tool secret")),
            (WorkflowHealth::Running, None)
        );
        assert_eq!(
            lifecycle("failed", Some("secret")),
            (WorkflowHealth::Failed, Some("Worker failed"))
        );
    }
    #[test]
    fn workflow_review_maps_authoritative_lifecycle_vocabulary() {
        for (status, health) in [
            ("crashed", WorkflowHealth::Failed),
            ("done", WorkflowHealth::Completed),
            ("running_stale", WorkflowHealth::Quiet),
            ("spawned", WorkflowHealth::Waiting),
            ("queued", WorkflowHealth::Waiting),
            ("pending", WorkflowHealth::Waiting),
            ("todo", WorkflowHealth::Waiting),
        ] {
            assert_eq!(lifecycle(status, None).0, health, "{status}");
        }
    }

    #[test]
    fn workflow_review_owned_activity_displaces_unrelated_clock_capacity() {
        let mut members = HashMap::new();
        let mut clocks = NativeClocks::default();
        for n in 0..257 {
            let id = format!("member-{n}");
            let (tx, _) = tokio::sync::mpsc::unbounded_channel();
            let record = serde_json::from_value(serde_json::json!({
                "session_id": id, "swarm_enabled": true, "status": "running", "role": "agent",
                "is_headless": true, "report_back_to_session_id": if n == 256 { Some("owner") } else { None }
            })).unwrap();
            members.insert(id.clone(), SwarmMember::from_record(record, tx));
            clocks.record(
                &BusEvent::TodoUpdated(crate::bus::TodoEvent {
                    session_id: id,
                    todos: vec![],
                }),
                &members,
                100,
            );
        }
        let owned = clocks
            .samples(&members, 120)
            .into_iter()
            .find(|sample| sample.owner.is_some())
            .unwrap();
        assert_eq!(owned.activity_at, Some(100));
        assert_eq!(owned.checkpoint_at, Some(100));
        assert!(clocks.clocks.len() <= 256);
    }
}
