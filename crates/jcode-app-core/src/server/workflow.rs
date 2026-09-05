//! Passive workflow scheduling and per-connection coalescing. Never owns model/process work.
use crate::bus::{WorkflowHealth, WorkflowSnapshot};
use crate::protocol::ServerEvent;
use std::sync::{Arc, OnceLock};
use tokio::sync::watch;

const MAX_SNAPSHOTS: usize = 256;

#[derive(Clone, Default)]
struct SnapshotBatch {
    owned: Vec<(String, WorkflowSnapshot)>,
    warning: Option<&'static str>,
}

impl SnapshotBatch {
    fn new(owned: Vec<(String, WorkflowSnapshot)>, warning: Option<&'static str>) -> Self {
        // Store populations are already bounded. Cap only after filtering by
        // owner so one busy session cannot erase another owner's sole record.
        Self { owned, warning }
    }

    fn event(&self, session_id: &str) -> ServerEvent {
        let mut workflows: Vec<_> = self
            .owned
            .iter()
            .filter(|(owner, _)| owner == session_id)
            .take(MAX_SNAPSHOTS)
            .map(|(_, snapshot)| snapshot.clone())
            .collect();
        let owned_count = self
            .owned
            .iter()
            .filter(|(owner, _)| owner == session_id)
            .count();
        if owned_count > MAX_SNAPSHOTS {
            workflows.insert(
                0,
                WorkflowSnapshot {
                    id: "workflow-capacity".into(),
                    label: "Workflow observer".into(),
                    health: WorkflowHealth::ObserverError,
                    detail: Some(format!(
                        "{} additional workflows omitted from this session snapshot",
                        owned_count - MAX_SNAPSHOTS + 1
                    )),
                    ..Default::default()
                },
            );
            workflows.truncate(MAX_SNAPSHOTS);
        }
        if let Some(warning) = self.warning {
            workflows.insert(
                0,
                WorkflowSnapshot {
                    id: "workflow-observer".into(),
                    label: "Workflow observer".into(),
                    health: WorkflowHealth::ObserverError,
                    detail: Some(warning.into()),
                    ..Default::default()
                },
            );
            workflows.truncate(MAX_SNAPSHOTS);
        }
        ServerEvent::WorkflowStatus {
            session_id: session_id.into(),
            workflows,
        }
    }
}

fn feed() -> &'static watch::Sender<Arc<SnapshotBatch>> {
    static FEED: OnceLock<watch::Sender<Arc<SnapshotBatch>>> = OnceLock::new();
    FEED.get_or_init(|| watch::channel(Arc::new(SnapshotBatch::default())).0)
}

/// No snapshot enters the unbounded client event queue or attachment broadcast.
/// The current connection session is supplied at delivery time, never captured at attach.
#[derive(Default)]
pub(super) struct Subscription {
    receiver: Option<watch::Receiver<Arc<SnapshotBatch>>>,
}

impl Subscription {
    pub(super) fn set_enabled(&mut self, enabled: bool) {
        self.receiver = enabled.then(|| feed().subscribe());
        self.refresh();
    }

    pub(super) fn refresh(&mut self) {
        if let Some(receiver) = &mut self.receiver {
            receiver.mark_changed();
        }
    }

    pub(super) async fn changed(&mut self) {
        match &mut self.receiver {
            Some(receiver) => {
                // The process-owned sender outlives every connection.
                if receiver.changed().await.is_err() {
                    std::future::pending::<()>().await;
                }
            }
            None => std::future::pending::<()>().await,
        }
    }

    pub(super) fn event(&mut self, current_session_id: &str) -> Option<ServerEvent> {
        self.receiver
            .as_mut()
            .map(|receiver| receiver.borrow_and_update().event(current_session_id))
    }
}

pub(super) fn spawn(
    members: Arc<tokio::sync::RwLock<std::collections::HashMap<String, super::state::SwarmMember>>>,
) {
    let config = crate::config::config().workflow.clone();
    if !config.enabled {
        return;
    }
    tokio::spawn(async move {
        let mut bus = crate::bus::Bus::global().subscribe();
        let mut native = super::workflow_native::NativeClocks::default();
        let mut lagged = false;
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(config.poll_seconds));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = tick.tick() => {},
                event = bus.recv() => {
                    match event {
                        Ok(event) => native.record(&event, &*members.read().await, crate::workflow::now_seconds()),
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => lagged = true,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                    }
                    continue;
                }
            }
            let (samples, native_limited) = {
                let members = members.read().await;
                (
                    native.samples(&members, crate::workflow::now_seconds()),
                    members.len() > 256,
                )
            };
            // Filesystem reads and private persistence do not block the Tokio worker.
            // Await each observation before scheduling another: no overlapping scans.
            let observation = tokio::task::spawn_blocking(move || {
                crate::workflow::global()?
                    .snapshots_with_native(crate::workflow::now_seconds(), samples)
            })
            .await;
            let batch = match observation {
                Ok(Ok(owned)) => SnapshotBatch::new(
                    owned,
                    if lagged {
                        Some(
                            "Activity events were missed; activity and checkpoint ages may be stale",
                        )
                    } else if native_limited {
                        Some("Native sampling capacity reached; some worker evidence may be stale")
                    } else {
                        None
                    },
                ),
                _ => SnapshotBatch {
                    owned: feed().borrow().owned.clone(),
                    warning: Some(
                        "Observation unavailable; last snapshot may be stale. Check the private workflow registry and restart after repair.",
                    ),
                },
            };
            feed().send_replace(Arc::new(batch));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn batch(value: &str) -> Arc<SnapshotBatch> {
        Arc::new(SnapshotBatch {
            owned: vec![
                (
                    "a".into(),
                    WorkflowSnapshot {
                        id: value.into(),
                        ..Default::default()
                    },
                ),
                (
                    "b".into(),
                    WorkflowSnapshot {
                        id: "private-b".into(),
                        ..Default::default()
                    },
                ),
            ],
            warning: None,
        })
    }

    #[tokio::test]
    async fn workflow_connection_coalesces_and_follows_current_session() {
        let (sender, receiver) = watch::channel(batch("first"));
        let mut subscription = Subscription {
            receiver: Some(receiver),
        };
        for n in 0..10_000 {
            sender.send_replace(batch(&n.to_string()));
        }
        subscription.changed().await;
        let Some(ServerEvent::WorkflowStatus {
            session_id,
            workflows,
        }) = subscription.event("a")
        else {
            panic!("typed snapshot required")
        };
        assert_eq!(session_id, "a");
        assert_eq!(workflows.len(), 1);
        assert_eq!(workflows[0].id, "9999");
        subscription.refresh();
        subscription.changed().await;
        let Some(ServerEvent::WorkflowStatus {
            session_id,
            workflows,
        }) = subscription.event("b")
        else {
            panic!("typed snapshot required")
        };
        assert_eq!(session_id, "b");
        assert_eq!(workflows.len(), 1);
        assert_eq!(workflows[0].id, "private-b");
        subscription.set_enabled(false);
        assert!(subscription.event("a").is_none());
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(1), subscription.changed())
                .await
                .is_err()
        );
    }

    #[test]
    fn workflow_snapshot_is_bounded_and_unrelated_owners_are_absent() {
        let batch = SnapshotBatch {
            owned: (0..1000)
                .map(|n| {
                    (
                        "a".into(),
                        WorkflowSnapshot {
                            id: n.to_string(),
                            ..Default::default()
                        },
                    )
                })
                .collect(),
            warning: None,
        };
        let ServerEvent::WorkflowStatus { workflows, .. } = batch.event("a") else {
            panic!()
        };
        assert_eq!(workflows.len(), MAX_SNAPSHOTS);
        let ServerEvent::WorkflowStatus { workflows, .. } = batch.event("b") else {
            panic!()
        };
        assert!(workflows.is_empty());
    }
    #[test]
    fn workflow_review_batch_does_not_discard_other_owners_before_filtering() {
        let mut owned = vec![("a".into(), WorkflowSnapshot::default()); 256];
        owned.push((
            "b".into(),
            WorkflowSnapshot {
                id: "sole-b".into(),
                ..Default::default()
            },
        ));
        let batch = SnapshotBatch::new(owned, None);
        let ServerEvent::WorkflowStatus { workflows, .. } = batch.event("b") else {
            panic!("snapshot")
        };
        assert_eq!(workflows.len(), 1);
        assert_eq!(workflows[0].id, "sole-b");
    }
    #[test]
    fn workflow_review_per_owner_limit_has_visible_overflow() {
        let batch = SnapshotBatch::new(vec![("a".into(), WorkflowSnapshot::default()); 320], None);
        let ServerEvent::WorkflowStatus { workflows, .. } = batch.event("a") else {
            panic!("snapshot")
        };
        assert_eq!(workflows.len(), 256);
        assert_eq!(workflows[0].id, "workflow-capacity");
        assert!(
            workflows[0]
                .detail
                .as_deref()
                .unwrap()
                .contains("65 additional")
        );
    }
}
