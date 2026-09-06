#![cfg_attr(test, allow(clippy::await_holding_lock))]

use super::{
    NotifySessionContext, clone_split_session, create_handoff_child_session, handle_handoff,
    handle_notify_session, handle_rename_session, handle_resume_all_sessions, handle_set_feature,
    handle_split, handoff_payload, process_manifest_id_for_lifecycle,
};
use crate::agent::Agent;
use crate::message::{ContentBlock, Message, Role, StreamEvent, ToolDefinition};
use crate::protocol::{FeatureToggle, ServerEvent};
use crate::provider::{EventStream, Provider};
use crate::server::{ClientConnectionInfo, SwarmMember};
use crate::tool::Registry;
use anyhow::Result;
use async_stream::stream;
use async_trait::async_trait;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Instant;
use tokio::sync::{Mutex, RwLock, mpsc};
use tokio::time::{Duration, timeout};

#[allow(clippy::type_complexity)]
fn empty_swarm_status_state() -> (
    Arc<RwLock<HashMap<String, std::collections::HashSet<String>>>>,
    Arc<RwLock<std::collections::VecDeque<crate::server::SwarmEvent>>>,
    Arc<std::sync::atomic::AtomicU64>,
    tokio::sync::broadcast::Sender<crate::server::SwarmEvent>,
) {
    let (swarm_event_tx, _) = tokio::sync::broadcast::channel(16);
    (
        Arc::new(RwLock::new(HashMap::new())),
        Arc::new(RwLock::new(std::collections::VecDeque::new())),
        Arc::new(std::sync::atomic::AtomicU64::new(0)),
        swarm_event_tx,
    )
}

struct MockProvider;

#[derive(Clone, Default)]
struct StreamingMockProvider {
    responses: Arc<StdMutex<VecDeque<Vec<StreamEvent>>>>,
}

impl StreamingMockProvider {
    fn queue_response(&self, events: Vec<StreamEvent>) {
        self.responses.lock().unwrap().push_back(events);
    }
}

#[async_trait]
impl Provider for MockProvider {
    async fn complete(
        &self,
        _messages: &[crate::message::Message],
        _tools: &[crate::message::ToolDefinition],
        _system: &str,
        _resume_session_id: Option<&str>,
    ) -> Result<EventStream> {
        Err(anyhow::anyhow!(
            "mock provider complete should not be called in client_actions tests"
        ))
    }

    async fn complete_simple(&self, _prompt: &str, _system: &str) -> Result<String> {
        Ok("Generated handoff summary. Next steps: continue the unfinished work.".to_string())
    }

    fn name(&self) -> &str {
        "mock"
    }

    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(MockProvider)
    }
}

#[async_trait]
impl Provider for StreamingMockProvider {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[ToolDefinition],
        _system: &str,
        _resume_session_id: Option<&str>,
    ) -> Result<EventStream> {
        let events = self
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_default();
        let stream = stream! {
            for event in events {
                yield Ok(event);
            }
        };
        Ok(Box::pin(stream))
    }

    fn name(&self) -> &str {
        "mock"
    }

    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(self.clone())
    }
}

#[test]
fn clone_split_session_uses_persisted_session_state() {
    let _guard = crate::storage::lock_test_env();
    let temp = tempfile::tempdir().expect("tempdir");
    let prev_home = std::env::var_os("JCODE_HOME");
    crate::env::set_var("JCODE_HOME", temp.path());

    let mut parent = crate::session::Session::create_with_id(
        "session_parent_split_test".to_string(),
        None,
        None,
    );
    parent.working_dir = Some("/tmp/jcode-split-test".to_string());
    parent.model = Some("gpt-test".to_string());
    parent.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "hello from parent".to_string(),
            cache_control: None,
        }],
    );
    parent.compaction = Some(crate::session::StoredCompactionState {
        summary_text: "summary".to_string(),
        openai_encrypted_content: None,
        covers_up_to_turn: 1,
        original_turn_count: 1,
        compacted_count: 1,
    });
    parent.save().expect("save parent");

    let mut unsaved_parent = parent.clone();
    unsaved_parent.model = Some("unsaved-model".into());
    unsaved_parent.add_message(
        Role::Assistant,
        vec![ContentBlock::Text {
            text: "unfinished turn".into(),
            cache_control: None,
        }],
    );
    let (child_id, _child_name) =
        clone_split_session(&parent.id, Some(&unsaved_parent)).expect("clone split");
    let child = crate::session::Session::load(&child_id).expect("load child");

    assert_eq!(child.parent_id.as_deref(), Some(parent.id.as_str()));
    assert_eq!(
        child.messages.len(),
        parent.messages.len() + 1,
        "fork should inherit the transcript plus one fork notice"
    );
    assert_eq!(
        child.messages[0].content_preview(),
        parent.messages[0].content_preview()
    );
    let fork_notice = child.messages.last().expect("fork notice message");
    assert_eq!(
        fork_notice.display_role,
        Some(crate::session::StoredDisplayRole::System),
        "fork notice must be hidden from the visible transcript"
    );
    let fork_notice_text = fork_notice.content_preview();
    assert!(
        fork_notice_text.contains("forked") && fork_notice_text.contains(parent.id.as_str()),
        "fork notice should mention the parent session: {fork_notice_text}"
    );
    assert_eq!(child.compaction, parent.compaction);
    assert_eq!(child.working_dir, parent.working_dir);
    assert_eq!(child.model, parent.model);
    assert_eq!(child.status, crate::session::SessionStatus::Closed);
    assert_ne!(child.id, parent.id);

    if let Some(prev_home) = prev_home {
        crate::env::set_var("JCODE_HOME", prev_home);
    } else {
        crate::env::remove_var("JCODE_HOME");
    }
}

#[test]
fn handoff_child_is_clean_and_preserves_only_operational_session_state() {
    let _guard = crate::storage::lock_test_env();
    let temp = tempfile::tempdir().expect("tempdir");
    let prev_home = std::env::var_os("JCODE_HOME");
    crate::env::set_var("JCODE_HOME", temp.path());

    let mut parent = crate::session::Session::create_with_id(
        "session_parent_handoff_test".to_string(),
        None,
        None,
    );
    parent.working_dir = Some("/tmp/jcode-handoff-test".to_string());
    parent.provider_key = Some("openai".to_string());
    parent.model = Some("gpt-test".to_string());
    parent.reasoning_effort = Some("high".to_string());
    parent.profile_name = Some("focus".to_string());
    parent.provider_session_id = Some("must-not-copy".to_string());
    parent.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "old context".to_string(),
            cache_control: None,
        }],
    );
    parent.compaction = Some(crate::session::StoredCompactionState {
        summary_text: "must not copy".to_string(),
        openai_encrypted_content: None,
        covers_up_to_turn: 1,
        original_turn_count: 1,
        compacted_count: 1,
    });
    parent.save().expect("save parent");

    let (child_id, _) = create_handoff_child_session(&parent.id, None, false, 8, true)
        .expect("create clean handoff child");
    let child = crate::session::Session::load(&child_id).expect("load child");

    assert_eq!(child.parent_id.as_deref(), Some(parent.id.as_str()));
    assert!(child.messages.is_empty());
    assert!(child.compaction.is_none());
    assert!(child.provider_session_id.is_none());
    assert_eq!(child.working_dir, parent.working_dir);
    assert_eq!(child.provider_key, parent.provider_key);
    assert_eq!(child.model, parent.model);
    assert_eq!(child.reasoning_effort, parent.reasoning_effort);
    assert_eq!(child.profile_name, parent.profile_name);
    assert_eq!(child.status, crate::session::SessionStatus::Closed);
    let chain_error = create_handoff_child_session(&child.id, None, false, 1, true)
        .expect_err("a second transition must exceed a one-transition chain limit");
    assert!(chain_error.to_string().contains("chain limit reached"));

    if let Some(prev_home) = prev_home {
        crate::env::set_var("JCODE_HOME", prev_home);
    } else {
        crate::env::remove_var("JCODE_HOME");
    }
}

#[test]
fn agent_initiated_handoff_carries_open_todos_into_the_child() {
    let _guard = crate::storage::lock_test_env();
    let temp = tempfile::tempdir().expect("tempdir");
    let prev_home = std::env::var_os("JCODE_HOME");
    crate::env::set_var("JCODE_HOME", temp.path());

    let mut parent = crate::session::Session::create_with_id(
        "session_parent_handoff_todo_test".to_string(),
        None,
        None,
    );
    parent.save().expect("save parent");

    let todos = vec![
        crate::todo::TodoItem {
            content: "finished milestone".to_string(),
            status: "completed".to_string(),
            priority: "high".to_string(),
            id: "done-1".to_string(),
            group: None,
            confidence: None,
            completion_confidence: None,
            confidence_history: Vec::new(),
            blocked_by: Vec::new(),
            assigned_to: None,
        },
        crate::todo::TodoItem {
            content: "next milestone".to_string(),
            status: "pending".to_string(),
            priority: "high".to_string(),
            id: "next-1".to_string(),
            group: None,
            confidence: None,
            completion_confidence: None,
            confidence_history: Vec::new(),
            blocked_by: Vec::new(),
            assigned_to: None,
        },
    ];
    crate::todo::save_todos(&parent.id, &todos).expect("save parent todos");

    let (child_id, _) = create_handoff_child_session(&parent.id, None, false, 8, true)
        .expect("create handoff child");

    let carried = crate::todo::load_todos(&child_id).expect("load child todos");
    assert_eq!(
        carried.len(),
        2,
        "agent-initiated handoff must carry the todo list like the user-initiated path"
    );
    assert!(carried.iter().any(|todo| todo.id == "next-1"));

    let (opted_out_id, _) = create_handoff_child_session(&parent.id, None, false, 8, false)
        .expect("create handoff child without todos");
    assert!(
        crate::todo::load_todos(&opted_out_id)
            .expect("load opted-out child todos")
            .is_empty(),
        "copy_todos=false must leave the fresh session without carried todos"
    );

    if let Some(prev_home) = prev_home {
        crate::env::set_var("JCODE_HOME", prev_home);
    } else {
        crate::env::remove_var("JCODE_HOME");
    }
}

#[test]
fn handoff_child_persists_prompt_for_destination_startup() {
    let _guard = crate::storage::lock_test_env();
    let temp = tempfile::tempdir().expect("tempdir");
    let prev_home = std::env::var_os("JCODE_HOME");
    crate::env::set_var("JCODE_HOME", temp.path());

    let mut parent = crate::session::Session::create_with_id(
        "session_parent_handoff_context_test".to_string(),
        None,
        None,
    );
    parent.save().expect("save parent");

    let (submitted_child_id, _) = create_handoff_child_session(
        &parent.id,
        Some("continue with the handoff".to_string()),
        true,
        8,
        true,
    )
    .expect("create submitted handoff child");
    let submitted_path = temp
        .path()
        .join(format!("client-input-{submitted_child_id}"));
    let submitted: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&submitted_path).expect("read submitted startup context"),
    )
    .expect("parse submitted startup context");
    assert_eq!(submitted["input"], "");
    assert_eq!(submitted["submit_on_restore"], false);
    assert_eq!(
        submitted["queued_messages"][0],
        "[SYSTEM: continue with the handoff]"
    );

    let (draft_child_id, _) = create_handoff_child_session(
        &parent.id,
        Some("review the handoff first".to_string()),
        false,
        8,
        true,
    )
    .expect("create draft handoff child");
    let draft_path = temp.path().join(format!("client-input-{draft_child_id}"));
    let draft: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&draft_path).expect("read draft startup context"),
    )
    .expect("parse draft startup context");
    assert_eq!(draft["input"], "review the handoff first");
    assert_eq!(draft["submit_on_restore"], false);

    if let Some(prev_home) = prev_home {
        crate::env::set_var("JCODE_HOME", prev_home);
    } else {
        crate::env::remove_var("JCODE_HOME");
    }
}

#[test]
fn handoff_lifecycle_metadata_records_shape_without_prompt_or_todo_content() {
    let payload = handoff_payload(
        "session-parent-safe",
        Some("session-child-safe".to_string()),
        3,
        "super-secret handoff prompt".len(),
        4,
        Some(true),
        crate::session::lifecycle_types::HandoffStartupOutcome::Started,
    );
    let event = crate::session::lifecycle_types::LifecycleEvent::Handoff {
        decision_type: crate::session::lifecycle_types::LifecycleDecisionType::Completed,
        semantic_reason: crate::session::lifecycle_types::LifecycleSemanticReason::ChildStartup,
        suppression_reason: None,
        payload: payload.clone(),
        process_manifest_id: None,
    };
    let json = serde_json::to_string(&event).expect("serialize lifecycle event");

    assert_eq!(payload.chain_depth, 3);
    assert_eq!(
        payload.generated_prompt_bytes,
        "super-secret handoff prompt".len()
    );
    assert_eq!(payload.todo_carryover_count, 4);
    assert_eq!(payload.startup_acknowledged, Some(true));
    assert_eq!(
        payload.startup_outcome,
        crate::session::lifecycle_types::HandoffStartupOutcome::Started
    );
    assert!(!json.contains("super-secret handoff prompt"));
    assert!(!json.contains("sensitive-todo-text"));
}

#[test]
fn process_manifest_lifecycle_reference_is_opaque_and_session_bound() {
    let status = crate::background::TaskStatusFile {
        task_id: "task-manifest-opaque".to_string(),
        tool_name: "bash".to_string(),
        display_name: Some("secret command label".to_string()),
        session_id: "session-parent-safe".to_string(),
        status: crate::bus::BackgroundTaskStatus::Running,
        exit_code: None,
        error: Some("secret raw error".to_string()),
        started_at: "2026-08-22T00:00:00Z".to_string(),
        completed_at: None,
        duration_secs: None,
        pid: Some(12345),
        owner_pid: Some(12345),
        owner_instance: Some("secret owner instance".to_string()),
        detached: true,
        notify: true,
        wake: false,
        progress: Some(crate::bus::BackgroundTaskProgress {
            kind: crate::bus::BackgroundTaskProgressKind::Determinate,
            percent: Some(50.0),
            message: Some("secret command output".to_string()),
            current: None,
            total: None,
            unit: None,
            updated_at: "2026-08-22T00:00:00Z".to_string(),
            eta_seconds: None,
            source: crate::bus::BackgroundTaskProgressSource::Reported,
        }),
        event_history: Vec::new(),
        stall_wake_seconds: None,
        managed_process: None,
        hard_deadline_at: None,
    };
    let process_manifest_id = process_manifest_id_for_lifecycle(&status, "session-parent-safe");
    assert_eq!(process_manifest_id.as_deref(), Some("task-manifest-opaque"));
    assert!(process_manifest_id_for_lifecycle(&status, "other-session").is_none());

    let unsafe_status = crate::background::TaskStatusFile {
        task_id: "secret task id with spaces".to_string(),
        ..status
    };
    assert!(process_manifest_id_for_lifecycle(&unsafe_status, "session-parent-safe").is_none());

    let event = crate::session::lifecycle_types::LifecycleEvent::Handoff {
        decision_type: crate::session::lifecycle_types::LifecycleDecisionType::Started,
        semantic_reason: crate::session::lifecycle_types::LifecycleSemanticReason::ChildStartup,
        suppression_reason: None,
        payload: handoff_payload(
            "session-parent-safe",
            Some("session-child-safe".to_string()),
            1,
            128,
            2,
            Some(true),
            crate::session::lifecycle_types::HandoffStartupOutcome::Started,
        ),
        process_manifest_id,
    };
    let json = serde_json::to_string(&event).expect("serialize process reference event");
    assert!(json.contains("task-manifest-opaque"));
    for sensitive in [
        "secret command label",
        "secret raw error",
        "secret owner instance",
        "secret command output",
        "12345",
    ] {
        assert!(
            !json.contains(sensitive),
            "process detail leaked: {sensitive}"
        );
    }
}

#[tokio::test]
async fn no_argument_handoff_persists_summary_for_destination_startup() {
    let _guard = crate::storage::lock_test_env();
    let temp = tempfile::tempdir().expect("tempdir");
    let prev_home = std::env::var_os("JCODE_HOME");
    crate::env::set_var("JCODE_HOME", temp.path());

    let mut parent = crate::session::Session::create_with_id(
        "session_parent_handoff_summary_test".to_string(),
        None,
        None,
    );
    parent.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "Preserve the exact handoff codeword salamander123".to_string(),
            cache_control: None,
        }],
    );
    parent.save().expect("save parent");
    crate::todo::save_todos(
        &parent.id,
        &[crate::todo::TodoItem {
            content: "Review the generated handoff next steps".to_string(),
            status: "in_progress".to_string(),
            priority: "high".to_string(),
            id: "handoff-next-step".to_string(),
            ..Default::default()
        }],
    )
    .expect("save parent todo");

    let provider: Arc<dyn Provider> = Arc::new(MockProvider);
    let registry = Registry::new(provider.clone()).await;
    let agent = Arc::new(Mutex::new(Agent::new(provider, registry)));
    let (client_event_tx, mut client_event_rx) = mpsc::unbounded_channel();
    handle_handoff(
        71,
        parent.id.clone(),
        agent,
        None,
        Some(true),
        client_event_tx,
    )
    .await;

    let event = client_event_rx
        .recv()
        .await
        .expect("handoff should emit an event");
    let (child_id, auto_start) = match event {
        ServerEvent::SessionHandoffReady {
            new_session_id,
            auto_start,
            ..
        } => (new_session_id, auto_start),
        other => panic!("expected handoff-ready event, got {other:?}"),
    };
    assert!(
        auto_start,
        "no-argument handoff should start its summary once"
    );

    let startup_path = temp.path().join(format!("client-input-{child_id}"));
    let startup: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&startup_path)
            .expect("no-argument handoff should persist destination startup context"),
    )
    .expect("parse persisted handoff startup context");
    assert_eq!(startup["input"], "");
    assert_eq!(startup["submit_on_restore"], false);
    let queued = startup["queued_messages"]
        .as_array()
        .expect("handoff startup context should contain a queued message");
    assert_eq!(queued.len(), 1);
    let queued_prompt = queued[0]
        .as_str()
        .expect("queued handoff prompt should be text");
    assert!(queued_prompt.contains("Generated handoff summary"));
    assert!(
        queued_prompt.contains("salamander123"),
        "no-argument handoff must not rely exclusively on a lossy model summary"
    );
    assert!(queued_prompt.contains("Next steps:"));
    assert!(queued_prompt.contains("Review the generated handoff next steps"));

    let child = crate::session::Session::load(&child_id).expect("load handoff child");
    assert!(child.messages.is_empty());
    assert!(child.compaction.is_some());
    let child_todos = crate::todo::load_todos(&child_id).expect("load handoff child todos");
    assert_eq!(child_todos.len(), 1);
    assert_eq!(
        child_todos[0].content,
        "Review the generated handoff next steps"
    );

    if let Some(prev_home) = prev_home {
        crate::env::set_var("JCODE_HOME", prev_home);
    } else {
        crate::env::remove_var("JCODE_HOME");
    }
}

struct SplitTestHome {
    _directory: tempfile::TempDir,
    previous_home: Option<std::ffi::OsString>,
}

impl SplitTestHome {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("split test home");
        let previous_home = std::env::var_os("JCODE_HOME");
        crate::env::set_var("JCODE_HOME", directory.path());
        Self {
            _directory: directory,
            previous_home,
        }
    }
}

impl Drop for SplitTestHome {
    fn drop(&mut self) {
        if let Some(home) = &self.previous_home {
            crate::env::set_var("JCODE_HOME", home);
        } else {
            crate::env::remove_var("JCODE_HOME");
        }
    }
}

async fn new_split_test_agent() -> Arc<Mutex<Agent>> {
    let provider: Arc<dyn Provider> = Arc::new(MockProvider);
    let registry = Registry::new(provider.clone()).await;
    Arc::new(Mutex::new(Agent::new_with_initial_working_dir(
        provider,
        registry,
        Some("/project/empty-split"),
    )))
}

fn split_response(
    rx: &mut mpsc::UnboundedReceiver<ServerEvent>,
    request_id: u64,
) -> crate::session::Session {
    let event = rx.try_recv().expect("split must respond");
    let ServerEvent::SplitResponse {
        id,
        new_session_id,
        new_session_name,
    } = event
    else {
        panic!("expected SplitResponse, got {event:?}");
    };
    assert_eq!(id, request_id);
    assert!(!new_session_name.is_empty());
    assert!(rx.try_recv().is_err(), "exactly one split response");
    crate::session::Session::load(&new_session_id).expect("fork must be persisted for attachment")
}

#[tokio::test]
async fn split_empty_live_session_without_persisted_parent() {
    let _guard = crate::storage::lock_test_env();
    let _home = SplitTestHome::new();
    let agent = new_split_test_agent().await;
    let parent = agent.lock().await.session_for_split().clone();
    assert_eq!(parent.visible_conversation_message_count(), 0);
    assert!(
        !crate::session::session_exists(&parent.id),
        "regression requires an unsaved parent"
    );
    let (tx, mut rx) = mpsc::unbounded_channel();

    handle_split(17, &parent.id, &agent, &tx).await;
    let child = split_response(&mut rx, 17);
    assert_ne!(child.id, parent.id);
    assert_eq!(child.parent_id.as_deref(), Some(parent.id.as_str()));
    assert_eq!(child.working_dir, parent.working_dir);
    assert_eq!(child.model, parent.model);
    assert_eq!(child.status, crate::session::SessionStatus::Closed);
    assert_eq!(child.messages.len(), parent.messages.len() + 1);
    let notice = child.messages.last().unwrap();
    assert_eq!(
        notice.display_role,
        Some(crate::session::StoredDisplayRole::System)
    );
    assert!(notice.content_preview().contains(&parent.id));
    assert_eq!(agent.lock().await.session_id(), parent.id);
    assert!(
        !crate::session::session_exists(&parent.id),
        "fork must not mutate/persist its parent"
    );
}

#[tokio::test]
async fn split_busy_session_uses_persisted_state_without_waiting_for_agent() {
    let _guard = crate::storage::lock_test_env();
    let _home = SplitTestHome::new();
    let agent = new_split_test_agent().await;
    let mut busy = agent.lock().await;
    let mut parent = busy.session_for_split().clone();
    parent.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "persisted request".into(),
            cache_control: None,
        }],
    );
    parent.save().expect("save pre-turn snapshot");
    busy.add_message(
        Role::Assistant,
        vec![ContentBlock::Text {
            text: "unsaved streaming output".into(),
            cache_control: None,
        }],
    );
    let (tx, mut rx) = mpsc::unbounded_channel();

    timeout(
        Duration::from_millis(100),
        handle_split(18, &parent.id, &agent, &tx),
    )
    .await
    .expect("split must not wait on the held streaming Agent lock");
    let child = split_response(&mut rx, 18);
    assert_eq!(child.messages.len(), parent.messages.len() + 1);
    assert_eq!(
        child.messages[0].content_preview(),
        parent.messages[0].content_preview()
    );
    assert!(
        !child
            .messages
            .iter()
            .any(|m| m.content_preview().contains("unsaved streaming output"))
    );
    assert!(
        child
            .messages
            .last()
            .unwrap()
            .content_preview()
            .contains("forked")
    );
    assert!(
        agent.try_lock().is_err(),
        "parent lock is still owned by the busy turn"
    );
    drop(busy);
}

#[tokio::test]
async fn split_busy_unsaved_session_returns_error_without_waiting() {
    let _guard = crate::storage::lock_test_env();
    let _home = SplitTestHome::new();
    let agent = new_split_test_agent().await;
    let busy = agent.lock().await;
    let parent_id = busy.session_id().to_owned();
    assert!(!crate::session::session_exists(&parent_id));
    let (tx, mut rx) = mpsc::unbounded_channel();
    timeout(
        Duration::from_millis(100),
        handle_split(19, &parent_id, &agent, &tx),
    )
    .await
    .expect("missing snapshot must not block a busy session");
    assert!(matches!(
        rx.try_recv(),
        Ok(ServerEvent::Error { id: 19, .. })
    ));
    assert!(rx.try_recv().is_err());
    drop(busy);
}

#[test]
fn split_missing_parent_never_uses_another_live_session() {
    let _guard = crate::storage::lock_test_env();
    let _home = SplitTestHome::new();
    let other = crate::session::Session::create(None, None);
    assert!(clone_split_session("session_missing_parent", Some(&other)).is_err());
}

#[test]
fn split_corrupt_persisted_parent_is_not_hidden_by_live_fallback() {
    let _guard = crate::storage::lock_test_env();
    let _home = SplitTestHome::new();
    let mut parent = crate::session::Session::create(None, Some("persisted parent".into()));
    parent.save().expect("create snapshot");
    let path = crate::session::session_path(&parent.id).unwrap();
    std::fs::write(&path, b"invalid session JSON").unwrap();
    assert!(clone_split_session(&parent.id, Some(&parent)).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"invalid session JSON");
}

#[tokio::test]
async fn enabling_swarm_does_not_auto_elect_coordinator() {
    let provider: Arc<dyn Provider> = Arc::new(MockProvider);
    let registry = Registry::new(provider.clone()).await;
    let agent = Arc::new(Mutex::new(Agent::new(provider, registry)));
    let (member_event_tx, _member_event_rx) = mpsc::unbounded_channel();
    let now = Instant::now();
    let session_id = "session_test_swarm_toggle";
    let swarm_members = Arc::new(RwLock::new(HashMap::from([(
        session_id.to_string(),
        crate::server::SwarmMember {
            session_id: session_id.to_string(),
            event_tx: member_event_tx,
            event_txs: HashMap::new(),
            working_dir: Some(PathBuf::from("/tmp/jcode-passive-swarm")),
            swarm_id: None,
            swarm_enabled: false,
            status: "ready".to_string(),
            detail: None,
            task_label: None,
            friendly_name: Some("duck".to_string()),
            report_back_to_session_id: None,
            latest_completion_report: None,
            role: "agent".to_string(),
            joined_at: now,
            last_status_change: now,
            is_headless: false,
            output_tail: None,
            todo_progress: None,
            todo_items: Vec::new(),
            runtime: crate::protocol::SwarmMemberRuntime::default(),
        },
    )])));
    let swarms_by_id = Arc::new(RwLock::new(HashMap::<String, HashSet<String>>::new()));
    let swarm_coordinators = Arc::new(RwLock::new(HashMap::<String, String>::new()));
    let channel_subscriptions = Arc::new(RwLock::new(HashMap::<
        String,
        HashMap<String, HashSet<String>>,
    >::new()));
    let channel_subscriptions_by_session = Arc::new(RwLock::new(HashMap::<
        String,
        HashMap<String, HashSet<String>>,
    >::new()));
    let swarm_plans = Arc::new(RwLock::new(HashMap::new()));
    let (client_event_tx, mut client_event_rx) = mpsc::unbounded_channel();
    let mut swarm_enabled = false;

    handle_set_feature(
        42,
        FeatureToggle::Swarm,
        true,
        &agent,
        session_id,
        &Some("duck".to_string()),
        &mut swarm_enabled,
        &swarm_members,
        &swarms_by_id,
        &swarm_coordinators,
        &channel_subscriptions,
        &channel_subscriptions_by_session,
        &swarm_plans,
        &client_event_tx,
    )
    .await;

    assert!(swarm_enabled);
    assert!(swarm_coordinators.read().await.is_empty());
    assert_eq!(
        swarm_members
            .read()
            .await
            .get(session_id)
            .and_then(|member| member.swarm_id.clone())
            .as_deref(),
        Some("/tmp/jcode-passive-swarm")
    );
    assert_eq!(
        swarm_members
            .read()
            .await
            .get(session_id)
            .map(|member| member.role.as_str()),
        Some("agent")
    );

    let events: Vec<_> = std::iter::from_fn(|| client_event_rx.try_recv().ok()).collect();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ServerEvent::Done { id: 42 }))
    );
    assert!(events.iter().all(|event| {
        !matches!(
            event,
            ServerEvent::Notification { message, .. }
                if message == "You are the coordinator for this swarm."
        )
    }));
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn rename_session_event_uses_agent_session_id_even_when_client_id_is_stale() {
    let _guard = crate::storage::lock_test_env();
    let temp = tempfile::tempdir().expect("tempdir");
    let prev_home = std::env::var_os("JCODE_HOME");
    crate::env::set_var("JCODE_HOME", temp.path());

    let provider: Arc<dyn Provider> = Arc::new(MockProvider);
    let registry = Registry::new(provider.clone()).await;
    let agent = Arc::new(Mutex::new(Agent::new(provider, registry)));
    let agent_session_id = agent.lock().await.session_id().to_string();
    let stale_client_session_id = "session_stale_client_id";
    let (member_event_tx, mut member_event_rx) = mpsc::unbounded_channel();
    let now = Instant::now();
    let swarm_members = Arc::new(RwLock::new(HashMap::from([(
        stale_client_session_id.to_string(),
        SwarmMember {
            session_id: stale_client_session_id.to_string(),
            event_tx: member_event_tx,
            event_txs: HashMap::new(),
            working_dir: None,
            swarm_id: None,
            swarm_enabled: false,
            status: "ready".to_string(),
            detail: None,
            task_label: None,
            friendly_name: Some("stale".to_string()),
            report_back_to_session_id: None,
            latest_completion_report: None,
            role: "agent".to_string(),
            joined_at: now,
            last_status_change: now,
            is_headless: false,
            output_tail: None,
            todo_progress: None,
            todo_items: Vec::new(),
            runtime: crate::protocol::SwarmMemberRuntime::default(),
        },
    )])));
    let (client_event_tx, mut client_event_rx) = mpsc::unbounded_channel();

    handle_rename_session(
        99,
        Some("Release planning".to_string()),
        &agent,
        stale_client_session_id,
        &swarm_members,
        &client_event_tx,
    )
    .await;

    let rename_event = timeout(Duration::from_secs(2), member_event_rx.recv())
        .await
        .expect("rename event should arrive")
        .expect("member event channel should stay open");
    match rename_event {
        ServerEvent::SessionRenamed {
            session_id,
            title,
            display_title,
        } => {
            assert_eq!(session_id, agent_session_id);
            assert_eq!(title.as_deref(), Some("Release planning"));
            assert_eq!(display_title, "Release planning");
        }
        other => panic!("expected SessionRenamed, got {other:?}"),
    }

    let client_events: Vec<_> = std::iter::from_fn(|| client_event_rx.try_recv().ok()).collect();
    assert!(
        client_events
            .iter()
            .any(|event| matches!(event, ServerEvent::Done { id } if *id == 99))
    );
    let loaded = crate::session::Session::load(&agent_session_id).expect("renamed session saved");
    assert_eq!(loaded.custom_title.as_deref(), Some("Release planning"));

    if let Some(prev_home) = prev_home {
        crate::env::set_var("JCODE_HOME", prev_home);
    } else {
        crate::env::remove_var("JCODE_HOME");
    }
}

#[tokio::test]
async fn notify_session_runs_scheduled_task_immediately_for_idle_live_session() {
    let provider = Arc::new(StreamingMockProvider::default());
    provider.queue_response(vec![
        StreamEvent::TextDelta("Working on scheduled task.".to_string()),
        StreamEvent::MessageEnd { stop_reason: None },
    ]);
    let provider_dyn: Arc<dyn Provider> = provider.clone();
    let registry = Registry::new(provider_dyn.clone()).await;
    let agent = Arc::new(Mutex::new(Agent::new(provider_dyn, registry)));
    let session_id = agent.lock().await.session_id().to_string();
    let sessions = Arc::new(RwLock::new(HashMap::<String, Arc<Mutex<Agent>>>::from([(
        session_id.clone(),
        agent.clone(),
    )])));
    let soft_interrupt_queues = Arc::new(RwLock::new(HashMap::new()));
    let client_connections = Arc::new(RwLock::new(HashMap::from([(
        "client-1".to_string(),
        ClientConnectionInfo {
            client_id: "client-1".to_string(),
            session_id: session_id.clone(),
            client_instance_id: None,
            debug_client_id: Some("debug-1".to_string()),
            connected_at: Instant::now(),
            last_seen: Instant::now(),
            is_processing: false,
            current_tool_name: None,
            terminal_env: Vec::new(),
            disconnect_tx: mpsc::unbounded_channel().0,
        },
    )])));
    let (member_event_tx, mut member_event_rx) = mpsc::unbounded_channel();
    let swarm_members = Arc::new(RwLock::new(HashMap::from([(
        session_id.clone(),
        SwarmMember {
            session_id: session_id.clone(),
            event_tx: member_event_tx,
            event_txs: HashMap::new(),
            working_dir: None,
            swarm_id: None,
            swarm_enabled: false,
            status: "ready".to_string(),
            detail: None,
            task_label: None,
            friendly_name: Some("otter".to_string()),
            report_back_to_session_id: None,
            latest_completion_report: None,
            role: "agent".to_string(),
            joined_at: Instant::now(),
            last_status_change: Instant::now(),
            is_headless: false,
            output_tail: None,
            todo_progress: None,
            todo_items: Vec::new(),
            runtime: crate::protocol::SwarmMemberRuntime::default(),
        },
    )])));
    let (client_event_tx, mut client_event_rx) = mpsc::unbounded_channel();

    let (swarms_by_id, event_history, event_counter, swarm_event_tx) = empty_swarm_status_state();
    handle_notify_session(
        77,
        session_id.clone(),
        "[Scheduled task]\nTask: Follow up".to_string(),
        NotifySessionContext {
            sessions: &sessions,
            soft_interrupt_queues: &soft_interrupt_queues,
            client_connections: &client_connections,
            swarm_members: &swarm_members,
            swarms_by_id: &swarms_by_id,
            event_history: &event_history,
            event_counter: &event_counter,
            swarm_event_tx: &swarm_event_tx,
            client_event_tx: &client_event_tx,
        },
    )
    .await;

    let streamed_event = timeout(Duration::from_secs(2), async {
        loop {
            match member_event_rx.recv().await {
                Some(ServerEvent::TextDelta { text })
                    if text.contains("Working on scheduled task.") =>
                {
                    return text;
                }
                Some(_) => continue,
                None => panic!("live member stream closed before scheduled task ran"),
            }
        }
    })
    .await
    .expect("scheduled task should start streaming promptly");
    assert!(streamed_event.contains("Working on scheduled task."));

    let client_events: Vec<_> = std::iter::from_fn(|| client_event_rx.try_recv().ok()).collect();
    assert!(
        client_events
            .iter()
            .any(|event| matches!(event, ServerEvent::Done { id } if *id == 77))
    );

    let guard = agent.lock().await;
    assert!(guard.messages().iter().any(|message| {
        message.role == Role::User
            && message.display_role == Some(crate::session::StoredDisplayRole::System)
            && message
                .content_preview()
                .contains("[Scheduled task] Task: Follow up")
    }));
    assert!(guard.messages().iter().any(|message| {
        message.role == Role::Assistant
            && message
                .content_preview()
                .contains("Working on scheduled task.")
    }));
}

#[tokio::test]
async fn notify_session_queues_soft_interrupt_when_live_session_is_busy() {
    let provider: Arc<dyn Provider> = Arc::new(MockProvider);
    let registry = Registry::new(provider.clone()).await;
    let agent = Arc::new(Mutex::new(Agent::new(provider, registry)));
    let session_id = agent.lock().await.session_id().to_string();
    let queue = agent.lock().await.soft_interrupt_queue();

    let sessions = Arc::new(RwLock::new(HashMap::<String, Arc<Mutex<Agent>>>::from([(
        session_id.clone(),
        agent.clone(),
    )])));
    let soft_interrupt_queues = Arc::new(RwLock::new(HashMap::from([(
        session_id.clone(),
        queue.clone(),
    )])));
    let client_connections = Arc::new(RwLock::new(HashMap::from([(
        "client-1".to_string(),
        ClientConnectionInfo {
            client_id: "client-1".to_string(),
            session_id: session_id.clone(),
            client_instance_id: None,
            debug_client_id: Some("debug-1".to_string()),
            connected_at: Instant::now(),
            last_seen: Instant::now(),
            is_processing: false,
            current_tool_name: None,
            terminal_env: Vec::new(),
            disconnect_tx: mpsc::unbounded_channel().0,
        },
    )])));
    let (member_event_tx, mut member_event_rx) = mpsc::unbounded_channel();
    let swarm_members = Arc::new(RwLock::new(HashMap::from([(
        session_id.clone(),
        SwarmMember {
            session_id: session_id.clone(),
            event_tx: member_event_tx,
            event_txs: HashMap::new(),
            working_dir: None,
            swarm_id: None,
            swarm_enabled: false,
            status: "running".to_string(),
            detail: None,
            task_label: None,
            friendly_name: Some("otter".to_string()),
            report_back_to_session_id: None,
            latest_completion_report: None,
            role: "agent".to_string(),
            joined_at: Instant::now(),
            last_status_change: Instant::now(),
            is_headless: false,
            output_tail: None,
            todo_progress: None,
            todo_items: Vec::new(),
            runtime: crate::protocol::SwarmMemberRuntime::default(),
        },
    )])));
    let (client_event_tx, mut client_event_rx) = mpsc::unbounded_channel();

    let _busy_guard = agent.lock().await;

    let (swarms_by_id, event_history, event_counter, swarm_event_tx) = empty_swarm_status_state();
    handle_notify_session(
        88,
        session_id.clone(),
        "[Scheduled task]\nTask: Follow up while busy".to_string(),
        NotifySessionContext {
            sessions: &sessions,
            soft_interrupt_queues: &soft_interrupt_queues,
            client_connections: &client_connections,
            swarm_members: &swarm_members,
            swarms_by_id: &swarms_by_id,
            event_history: &event_history,
            event_counter: &event_counter,
            swarm_event_tx: &swarm_event_tx,
            client_event_tx: &client_event_tx,
        },
    )
    .await;

    let member_event = timeout(Duration::from_secs(2), member_event_rx.recv())
        .await
        .expect("notification should arrive promptly")
        .expect("live member should receive notification");
    match member_event {
        ServerEvent::Notification {
            from_session,
            from_name,
            message,
            ..
        } => {
            assert_eq!(from_session, "schedule");
            assert_eq!(from_name.as_deref(), Some("scheduled task"));
            assert!(message.contains("Task: Follow up while busy"));
        }
        other => panic!("expected notification event, got {other:?}"),
    }

    let queued = queue.lock().unwrap();
    assert_eq!(
        queued.len(),
        1,
        "scheduled task should queue as soft interrupt"
    );
    assert!(queued[0].content.contains("Task: Follow up while busy"));
    drop(queued);

    let client_events: Vec<_> = std::iter::from_fn(|| client_event_rx.try_recv().ok()).collect();
    assert!(
        client_events
            .iter()
            .any(|event| matches!(event, ServerEvent::Done { id } if *id == 88))
    );
}

/// Build a live SwarmMember with a real client attachment so the resume-all
/// sweep treats it as live. Returns the member and the receiver for events
/// fanned out to that attachment.
fn live_member(session_id: &str) -> (SwarmMember, mpsc::UnboundedReceiver<ServerEvent>) {
    let (attach_tx, attach_rx) = mpsc::unbounded_channel();
    let member = SwarmMember {
        session_id: session_id.to_string(),
        event_tx: mpsc::unbounded_channel().0,
        event_txs: HashMap::from([("client-1".to_string(), attach_tx)]),
        working_dir: None,
        swarm_id: None,
        swarm_enabled: false,
        status: "ready".to_string(),
        detail: None,
        task_label: None,
        friendly_name: Some("otter".to_string()),
        report_back_to_session_id: None,
        latest_completion_report: None,
        role: "agent".to_string(),
        joined_at: Instant::now(),
        last_status_change: Instant::now(),
        is_headless: false,
        output_tail: None,
        todo_progress: None,
        todo_items: Vec::new(),
        runtime: crate::protocol::SwarmMemberRuntime::default(),
    };
    (member, attach_rx)
}

#[tokio::test]
async fn resume_all_continues_interrupted_idle_live_session() {
    let _guard = crate::storage::lock_test_env();
    let temp = tempfile::tempdir().expect("tempdir");
    let prev_home = std::env::var_os("JCODE_HOME");
    crate::env::set_var("JCODE_HOME", temp.path());

    let provider = Arc::new(StreamingMockProvider::default());
    provider.queue_response(vec![
        StreamEvent::TextDelta("Continuing where I left off.".to_string()),
        StreamEvent::MessageEnd { stop_reason: None },
    ]);
    let provider_dyn: Arc<dyn Provider> = provider.clone();
    let registry = Registry::new(provider_dyn.clone()).await;
    let agent = Arc::new(Mutex::new(Agent::new(provider_dyn, registry)));
    let session_id = {
        let mut guard = agent.lock().await;
        // Leave the session with a pending user turn the assistant never answered
        // (simulating a turn that errored / was interrupted mid-generation).
        guard.add_message(
            Role::User,
            vec![ContentBlock::Text {
                text: "please keep going on the refactor".to_string(),
                cache_control: None,
            }],
        );
        guard.session_id().to_string()
    };

    let sessions = Arc::new(RwLock::new(HashMap::<String, Arc<Mutex<Agent>>>::from([(
        session_id.clone(),
        agent.clone(),
    )])));
    let (member, mut attach_rx) = live_member(&session_id);
    let swarm_members = Arc::new(RwLock::new(HashMap::from([(session_id.clone(), member)])));
    let (client_event_tx, mut client_event_rx) = mpsc::unbounded_channel();

    let (swarms_by_id, event_history, event_counter, swarm_event_tx) = empty_swarm_status_state();
    handle_resume_all_sessions(
        91,
        &sessions,
        &swarm_members,
        &swarms_by_id,
        &event_history,
        &event_counter,
        &swarm_event_tx,
        &client_event_tx,
    )
    .await;

    // The session should resume and stream the continuation.
    let streamed = timeout(Duration::from_secs(2), async {
        loop {
            match attach_rx.recv().await {
                Some(ServerEvent::TextDelta { text })
                    if text.contains("Continuing where I left off.") =>
                {
                    return text;
                }
                Some(_) => continue,
                None => panic!("live attachment closed before continuation streamed"),
            }
        }
    })
    .await
    .expect("interrupted session should resume promptly");
    assert!(streamed.contains("Continuing where I left off."));

    // The requesting client receives a summary describing one resumed session.
    let result = timeout(Duration::from_secs(2), async {
        loop {
            match client_event_rx.recv().await {
                Some(event @ ServerEvent::ResumeAllResult { .. }) => return event,
                Some(_) => continue,
                None => panic!("client channel closed before resume-all result"),
            }
        }
    })
    .await
    .expect("resume-all result should be emitted");
    match result {
        ServerEvent::ResumeAllResult {
            id,
            resumed,
            skipped,
            ..
        } => {
            assert_eq!(id, 91);
            assert_eq!(resumed, 1);
            assert_eq!(skipped, 0);
        }
        other => panic!("expected ResumeAllResult, got {other:?}"),
    }

    if let Some(home) = prev_home {
        crate::env::set_var("JCODE_HOME", home);
    } else {
        crate::env::remove_var("JCODE_HOME");
    }
}

#[tokio::test]
async fn resume_all_skips_session_with_completed_turn() {
    let _guard = crate::storage::lock_test_env();
    let temp = tempfile::tempdir().expect("tempdir");
    let prev_home = std::env::var_os("JCODE_HOME");
    crate::env::set_var("JCODE_HOME", temp.path());

    let provider: Arc<dyn Provider> = Arc::new(MockProvider);
    let registry = Registry::new(provider.clone()).await;
    let agent = Arc::new(Mutex::new(Agent::new(provider, registry)));
    let session_id = {
        let mut guard = agent.lock().await;
        // A completed turn: last visible message is from the assistant.
        guard.add_message(
            Role::User,
            vec![ContentBlock::Text {
                text: "do the thing".to_string(),
                cache_control: None,
            }],
        );
        guard.add_message(
            Role::Assistant,
            vec![ContentBlock::Text {
                text: "done".to_string(),
                cache_control: None,
            }],
        );
        guard.session_id().to_string()
    };

    let sessions = Arc::new(RwLock::new(HashMap::<String, Arc<Mutex<Agent>>>::from([(
        session_id.clone(),
        agent.clone(),
    )])));
    let (member, _attach_rx) = live_member(&session_id);
    let swarm_members = Arc::new(RwLock::new(HashMap::from([(session_id.clone(), member)])));
    let (client_event_tx, mut client_event_rx) = mpsc::unbounded_channel();

    let (swarms_by_id, event_history, event_counter, swarm_event_tx) = empty_swarm_status_state();
    handle_resume_all_sessions(
        92,
        &sessions,
        &swarm_members,
        &swarms_by_id,
        &event_history,
        &event_counter,
        &swarm_event_tx,
        &client_event_tx,
    )
    .await;

    let result = timeout(Duration::from_secs(2), async {
        loop {
            match client_event_rx.recv().await {
                Some(event @ ServerEvent::ResumeAllResult { .. }) => return event,
                Some(_) => continue,
                None => panic!("client channel closed before resume-all result"),
            }
        }
    })
    .await
    .expect("resume-all result should be emitted");
    match result {
        ServerEvent::ResumeAllResult {
            id,
            resumed,
            skipped,
            ..
        } => {
            assert_eq!(id, 92);
            assert_eq!(resumed, 0);
            assert_eq!(skipped, 1);
        }
        other => panic!("expected ResumeAllResult, got {other:?}"),
    }

    if let Some(home) = prev_home {
        crate::env::set_var("JCODE_HOME", home);
    } else {
        crate::env::remove_var("JCODE_HOME");
    }
}
