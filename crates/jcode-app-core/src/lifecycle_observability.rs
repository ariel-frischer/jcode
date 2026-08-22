use crate::config::LifecycleObservabilityConfig;
use chrono::{DateTime, Utc};
use jcode_session_types::lifecycle::{
    LIFECYCLE_SCHEMA_VERSION, LifecycleEvent, LifecycleEventEnvelope, LifecycleObservabilityStatus,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};

const DEFAULT_QUEUE_CAPACITY: usize = 256;
const MAX_DIAGNOSTICS: usize = 32;

type Clock = Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleSubmitOutcome {
    Accepted,
    Disabled,
    DuplicatePolicy,
    QueueFull,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleRecorderDiagnostic {
    QueueFull,
    PersistenceFailure,
    LoggingFailure,
    WorkerUnavailable,
}

#[derive(Default)]
struct SessionRecorderState {
    next_sequence: u64,
    policy_fingerprint: Option<String>,
}

enum RecorderCommand {
    Event(LifecycleEventEnvelope),
    Flush(oneshot::Sender<()>),
}

pub struct LifecycleRecorder {
    status: LifecycleObservabilityStatus,
    sender: mpsc::Sender<RecorderCommand>,
    sessions: Mutex<HashMap<String, SessionRecorderState>>,
    diagnostics: Arc<Mutex<Vec<LifecycleRecorderDiagnostic>>>,
    clock: Clock,
}

impl LifecycleRecorder {
    pub fn new(config: LifecycleObservabilityConfig, base_dir: PathBuf) -> Arc<Self> {
        Self::new_with_clock(config, base_dir, DEFAULT_QUEUE_CAPACITY, Arc::new(Utc::now))
    }

    pub fn new_with_clock(
        config: LifecycleObservabilityConfig,
        base_dir: PathBuf,
        queue_capacity: usize,
        clock: Clock,
    ) -> Arc<Self> {
        let status = config.effective_status();
        let (sender, receiver) = mpsc::channel(queue_capacity.max(1));
        let diagnostics = Arc::new(Mutex::new(Vec::new()));
        let worker_diagnostics = Arc::clone(&diagnostics);
        let worker = run_recorder_worker(receiver, base_dir, status, worker_diagnostics);
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(worker);
        } else {
            let spawn_result = std::thread::Builder::new()
                .name("jcode-lifecycle-recorder".to_string())
                .spawn(move || {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build();
                    if let Ok(runtime) = runtime {
                        runtime.block_on(worker);
                    }
                });
            if spawn_result.is_err() {
                record_diagnostic(&diagnostics, LifecycleRecorderDiagnostic::WorkerUnavailable);
            }
        }
        Arc::new(Self {
            status,
            sender,
            sessions: Mutex::new(HashMap::new()),
            diagnostics,
            clock,
        })
    }

    pub fn status(&self) -> LifecycleObservabilityStatus {
        self.status
    }

    pub fn submit(&self, session_id: &str, event: LifecycleEvent) -> LifecycleSubmitOutcome {
        if !self.status.enabled {
            return LifecycleSubmitOutcome::Disabled;
        }

        let event = sanitize_event(event);
        let policy_fingerprint = match &event {
            LifecycleEvent::PolicySnapshot { snapshot } => Some(snapshot.fingerprint.clone()),
            _ => None,
        };
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let state = sessions.entry(session_id.to_string()).or_default();
        if policy_fingerprint
            .as_ref()
            .is_some_and(|fingerprint| state.policy_fingerprint.as_ref() == Some(fingerprint))
        {
            return LifecycleSubmitOutcome::DuplicatePolicy;
        }

        let sequence = state.next_sequence.saturating_add(1);
        let envelope = LifecycleEventEnvelope {
            schema_version: LIFECYCLE_SCHEMA_VERSION,
            session_id: session_id.to_string(),
            sequence,
            recorded_at: (self.clock)(),
            event,
        };
        match self.sender.try_send(RecorderCommand::Event(envelope)) {
            Ok(()) => {
                state.next_sequence = sequence;
                if let Some(fingerprint) = policy_fingerprint {
                    state.policy_fingerprint = Some(fingerprint);
                }
                LifecycleSubmitOutcome::Accepted
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                record_diagnostic(&self.diagnostics, LifecycleRecorderDiagnostic::QueueFull);
                LifecycleSubmitOutcome::QueueFull
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                record_diagnostic(
                    &self.diagnostics,
                    LifecycleRecorderDiagnostic::WorkerUnavailable,
                );
                LifecycleSubmitOutcome::QueueFull
            }
        }
    }

    pub async fn flush(&self) -> Vec<LifecycleRecorderDiagnostic> {
        let (sender, receiver) = oneshot::channel();
        if self
            .sender
            .send(RecorderCommand::Flush(sender))
            .await
            .is_err()
        {
            record_diagnostic(
                &self.diagnostics,
                LifecycleRecorderDiagnostic::WorkerUnavailable,
            );
        } else {
            let _ = receiver.await;
        }
        self.diagnostics
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

async fn run_recorder_worker(
    mut receiver: mpsc::Receiver<RecorderCommand>,
    base_dir: PathBuf,
    status: LifecycleObservabilityStatus,
    diagnostics: Arc<Mutex<Vec<LifecycleRecorderDiagnostic>>>,
) {
    while let Some(command) = receiver.recv().await {
        match command {
            RecorderCommand::Event(envelope) => {
                if status.persist_session_events
                    && crate::session::append_lifecycle_event_in_dir(&base_dir, &envelope).is_err()
                {
                    record_diagnostic(
                        &diagnostics,
                        LifecycleRecorderDiagnostic::PersistenceFailure,
                    );
                    crate::logging::event_info(
                        "lifecycle_recorder_error",
                        vec![("kind", "persistence_failure".to_string())],
                    );
                }
                if status.emit_structured_logs {
                    match serde_json::to_string(&envelope) {
                        Ok(serialized) => crate::logging::event_info(
                            "lifecycle_event",
                            vec![("envelope", serialized)],
                        ),
                        Err(_) => record_diagnostic(
                            &diagnostics,
                            LifecycleRecorderDiagnostic::LoggingFailure,
                        ),
                    }
                }
            }
            RecorderCommand::Flush(sender) => {
                let _ = sender.send(());
            }
        }
    }
}

fn record_diagnostic(
    diagnostics: &Mutex<Vec<LifecycleRecorderDiagnostic>>,
    diagnostic: LifecycleRecorderDiagnostic,
) {
    let mut diagnostics = diagnostics
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !diagnostics.contains(&diagnostic) && diagnostics.len() < MAX_DIAGNOSTICS {
        diagnostics.push(diagnostic);
    }
}

fn sanitize_identifier(value: &str) -> Option<String> {
    (!value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')))
    .then(|| value.to_string())
}

fn sanitize_event(mut event: LifecycleEvent) -> LifecycleEvent {
    match &mut event {
        LifecycleEvent::PolicySnapshot { snapshot } => {
            snapshot.fingerprint = sanitize_identifier(&snapshot.fingerprint)
                .unwrap_or_else(|| "redacted".to_string());
        }
        LifecycleEvent::Compaction {
            process_manifest_id,
            ..
        }
        | LifecycleEvent::Retry {
            process_manifest_id,
            ..
        }
        | LifecycleEvent::Block {
            process_manifest_id,
            ..
        } => {
            *process_manifest_id = process_manifest_id.as_deref().and_then(sanitize_identifier);
        }
        LifecycleEvent::Handoff {
            payload,
            process_manifest_id,
            ..
        } => {
            payload.parent_session_id = sanitize_identifier(&payload.parent_session_id)
                .unwrap_or_else(|| "redacted".to_string());
            payload.child_session_id = payload
                .child_session_id
                .as_deref()
                .and_then(sanitize_identifier);
            *process_manifest_id = process_manifest_id.as_deref().and_then(sanitize_identifier);
        }
        LifecycleEvent::StrategySwitch { .. } => {}
    }
    event
}
