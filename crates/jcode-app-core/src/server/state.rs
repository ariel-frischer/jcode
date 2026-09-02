use crate::bus::FileOp;
use crate::plan::VersionedPlan;
use crate::protocol::{
    QueuedMessageEditorDirection, QueuedMessageEditorOperation, QueuedMessageEditorOutcome,
    QueuedMessageEditorPlacement, QueuedMessageEditorSelection, RecallableSoftInterrupt,
    ServerEvent,
};
use jcode_agent_runtime::{
    InterruptSignal, SoftInterruptMessage, SoftInterruptQueue, SoftInterruptSource,
};
use jcode_swarm_core::{SwarmLifecycleStatus, SwarmMemberRecord, SwarmRole};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex as StdMutex};
use std::time::Instant;
use tokio::sync::{RwLock, mpsc};

/// Process-global registry mapping session id -> background-tool signal.
///
/// The background-tool ("move tool to background", Alt+B/Ctrl+B) signal lives on
/// the `Agent`, so a `SessionControlHandle` can normally only obtain it by
/// locking the agent mutex. When a turn is busy (e.g. running `await_members`),
/// `refresh_session_control_handle` falls back to a lock-free `cancel_only`
/// handle that historically dropped the background signal entirely, which made
/// Alt+B/Ctrl+B silently no-op (`BACKGROUND_TOOL_SIGNAL_FIRE result=no_signal_handle`).
///
/// This registry is populated every time a full `SessionControlHandle` is built
/// (which always has both the session id and the correct signal), so the
/// lock-free fallback can still fire the background signal without the agent
/// lock. Entries are keyed by session id; renames/removals reuse
/// [`rename_background_tool_signal`]/[`remove_background_tool_signal`] alongside
/// the existing shutdown-signal lifecycle.
static BACKGROUND_TOOL_SIGNALS: LazyLock<StdMutex<HashMap<String, InterruptSignal>>> =
    LazyLock::new(|| StdMutex::new(HashMap::new()));

const SOFT_INTERRUPT_REPLAY_CAPACITY: usize = 64;
const SOFT_INTERRUPT_REPLAY_SESSION_CAPACITY: usize = 256;
const QUEUED_MESSAGE_EDITOR_REPLAY_CAPACITY: usize = 128;
type SoftInterruptReplayKey = (String, String);
type SoftInterruptReplayEntry = (SoftInterruptReplayKey, Option<RecallableSoftInterrupt>);

#[derive(Default)]
struct SoftInterruptReplay {
    completed: VecDeque<SoftInterruptReplayEntry>,
}

impl SoftInterruptReplay {
    fn get(
        &self,
        client_instance_id: &str,
        operation_id: &str,
    ) -> Option<Option<RecallableSoftInterrupt>> {
        self.completed
            .iter()
            .find(|((client, operation), _)| {
                client == client_instance_id && operation == operation_id
            })
            .map(|(_, result)| result.clone())
    }

    fn insert(
        &mut self,
        client_instance_id: &str,
        operation_id: &str,
        result: Option<RecallableSoftInterrupt>,
    ) {
        if self.completed.len() == SOFT_INTERRUPT_REPLAY_CAPACITY {
            self.completed.pop_front();
        }
        self.completed.push_back((
            (client_instance_id.to_string(), operation_id.to_string()),
            result,
        ));
    }
}

type SharedSoftInterruptReplay = Arc<StdMutex<SoftInterruptReplay>>;

#[derive(Default)]
struct SoftInterruptReplayRegistry {
    replays: HashMap<String, SharedSoftInterruptReplay>,
    session_order: VecDeque<String>,
}

impl SoftInterruptReplayRegistry {
    fn get_or_insert(&mut self, session_id: &str) -> SharedSoftInterruptReplay {
        if let Some(replay) = self.replays.get(session_id).cloned() {
            self.session_order.retain(|id| id != session_id);
            self.session_order.push_back(session_id.to_string());
            return replay;
        }

        if self.replays.len() == SOFT_INTERRUPT_REPLAY_SESSION_CAPACITY
            && let Some(oldest_session_id) = self.session_order.pop_front()
        {
            self.replays.remove(&oldest_session_id);
        }

        let replay = Arc::new(StdMutex::new(SoftInterruptReplay::default()));
        self.replays.insert(session_id.to_string(), replay.clone());
        self.session_order.push_back(session_id.to_string());
        replay
    }

    fn rename(&mut self, old_session_id: &str, new_session_id: &str) {
        if old_session_id == new_session_id {
            return;
        }
        if let Some(replay) = self.replays.remove(old_session_id) {
            self.replays.insert(new_session_id.to_string(), replay);
            self.session_order.retain(|id| id != old_session_id);
            self.session_order.retain(|id| id != new_session_id);
            self.session_order.push_back(new_session_id.to_string());
        }
    }
}

static SOFT_INTERRUPT_REPLAYS: LazyLock<StdMutex<SoftInterruptReplayRegistry>> =
    LazyLock::new(|| StdMutex::new(SoftInterruptReplayRegistry::default()));

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QueuedMessageEditorResult {
    pub outcome: QueuedMessageEditorOutcome,
    pub selection: Option<QueuedMessageEditorSelection>,
    pub placement: QueuedMessageEditorPlacement,
    pub message: Option<String>,
}

#[derive(Clone)]
struct HeldQueuedMessage {
    original: SoftInterruptMessage,
    draft: RecallableSoftInterrupt,
}

#[derive(Clone)]
struct QueuedMessageReservation {
    owner_client_instance_id: String,
    snapshot_queue_sequence: u64,
    selected_index: usize,
    predecessor_message_id: Option<String>,
    successor_message_id: Option<String>,
    held: Vec<HeldQueuedMessage>,
}

#[derive(Clone)]
struct CompletedQueuedMessageEditorOperation {
    owner_client_instance_id: String,
    navigation_session_id: String,
    operation_id: String,
    request_fingerprint: String,
    result: QueuedMessageEditorResult,
}

#[derive(Clone, Default)]
struct QueuedMessageEditorCoordinator {
    reservations: HashMap<String, QueuedMessageReservation>,
    completed: VecDeque<CompletedQueuedMessageEditorOperation>,
    grace_tokens: HashMap<String, u64>,
    next_grace_token: u64,
    next_enqueue_sequence: u64,
}

type SharedQueuedMessageEditorCoordinator = Arc<StdMutex<QueuedMessageEditorCoordinator>>;

#[derive(Default)]
struct QueuedMessageEditorRegistry {
    coordinators: HashMap<String, SharedQueuedMessageEditorCoordinator>,
    session_order: VecDeque<String>,
}

impl QueuedMessageEditorRegistry {
    fn get_or_insert(&mut self, session_id: &str) -> SharedQueuedMessageEditorCoordinator {
        if let Some(coordinator) = self.coordinators.get(session_id).cloned() {
            self.session_order.retain(|id| id != session_id);
            self.session_order.push_back(session_id.to_string());
            return coordinator;
        }
        if self.coordinators.len() == SOFT_INTERRUPT_REPLAY_SESSION_CAPACITY
            && let Some(oldest) = self.session_order.pop_front()
        {
            self.coordinators.remove(&oldest);
        }
        let coordinator = Arc::new(StdMutex::new(QueuedMessageEditorCoordinator::default()));
        self.coordinators
            .insert(session_id.to_string(), coordinator.clone());
        self.session_order.push_back(session_id.to_string());
        coordinator
    }

    fn rename(&mut self, old_session_id: &str, new_session_id: &str) {
        if old_session_id == new_session_id {
            return;
        }
        if let Some(coordinator) = self.coordinators.remove(old_session_id) {
            self.coordinators
                .insert(new_session_id.to_string(), coordinator);
            self.session_order.retain(|id| id != old_session_id);
            self.session_order.retain(|id| id != new_session_id);
            self.session_order.push_back(new_session_id.to_string());
        }
    }
}

static QUEUED_MESSAGE_EDITORS: LazyLock<StdMutex<QueuedMessageEditorRegistry>> =
    LazyLock::new(|| StdMutex::new(QueuedMessageEditorRegistry::default()));

fn queued_message_editor_for_session(session_id: &str) -> SharedQueuedMessageEditorCoordinator {
    QUEUED_MESSAGE_EDITORS
        .lock()
        .map(|mut editors| editors.get_or_insert(session_id))
        .unwrap_or_else(|_| Arc::new(StdMutex::new(QueuedMessageEditorCoordinator::default())))
}

fn editor_selection(reservation: &QueuedMessageReservation) -> QueuedMessageEditorSelection {
    let selected = &reservation.held[reservation.selected_index];
    QueuedMessageEditorSelection {
        message_id: selected
            .original
            .message_id
            .clone()
            .expect("authoritative held messages always have stable identity"),
        content: selected.draft.content.clone(),
        images: selected.draft.images.clone(),
        older_available: reservation.selected_index > 0,
        newer_available: reservation.selected_index + 1 < reservation.held.len(),
    }
}

fn editor_fingerprint(operation: &QueuedMessageEditorOperation) -> Result<String, String> {
    serde_json::to_string(operation)
        .map_err(|error| format!("invalid queued message editor operation: {error}"))
}

fn restoration_position(
    queue: &[SoftInterruptMessage],
    reservation: &QueuedMessageReservation,
) -> (usize, bool) {
    let predecessor = reservation
        .predecessor_message_id
        .as_deref()
        .and_then(|id| {
            queue
                .iter()
                .position(|message| message.message_id.as_deref() == Some(id))
        });
    let successor = reservation.successor_message_id.as_deref().and_then(|id| {
        queue
            .iter()
            .position(|message| message.message_id.as_deref() == Some(id))
    });
    let predecessor_survived =
        reservation.predecessor_message_id.is_none() || predecessor.is_some();
    let successor_survived = reservation.successor_message_id.is_none() || successor.is_some();
    let exact = predecessor_survived
        && successor_survived
        && !matches!((predecessor, successor), (Some(before), Some(after)) if before >= after);
    let position = match (predecessor, successor) {
        (Some(before), Some(after)) if before < after => before + 1,
        (Some(before), _) => before + 1,
        (_, Some(after)) => after,
        (None, None) => queue
            .iter()
            .position(|message| {
                message
                    .enqueue_sequence
                    .is_some_and(|sequence| sequence > reservation.snapshot_queue_sequence)
            })
            .unwrap_or(queue.len()),
    };
    (position, exact)
}

fn restore_reservation(
    queue: &mut Vec<SoftInterruptMessage>,
    mut reservation: QueuedMessageReservation,
    selected_draft: Option<RecallableSoftInterrupt>,
) -> bool {
    let (position, exact) = restoration_position(queue, &reservation);
    if let Some(draft) = selected_draft {
        let selected = &mut reservation.held[reservation.selected_index];
        selected.original.content = draft.content;
        selected.original.images = draft.images;
    }
    queue.splice(
        position..position,
        reservation.held.into_iter().map(|held| held.original),
    );
    exact
}

#[cfg(not(test))]
fn persist_queued_message_editor_state(
    session_id: &str,
    queue: &[SoftInterruptMessage],
    coordinator: &QueuedMessageEditorCoordinator,
) -> Result<(), String> {
    use crate::soft_interrupt_store::{
        PersistedCompletedEditorOperation, PersistedHeldSoftInterrupt, PersistedReservationState,
        PersistedSoftInterruptDraft, PersistedSoftInterruptReservation, SoftInterruptStoreEnvelope,
    };
    let reservations = coordinator
        .reservations
        .iter()
        .map(
            |(navigation_session_id, reservation)| PersistedSoftInterruptReservation {
                navigation_session_id: navigation_session_id.clone(),
                owner_client_instance_id: reservation.owner_client_instance_id.clone(),
                snapshot_queue_sequence: reservation.snapshot_queue_sequence,
                state: PersistedReservationState::Active,
                selected_index: reservation.selected_index,
                predecessor_message_id: reservation.predecessor_message_id.clone(),
                successor_message_id: reservation.successor_message_id.clone(),
                held: reservation
                    .held
                    .iter()
                    .enumerate()
                    .map(|(index, held)| PersistedHeldSoftInterrupt {
                        message_id: held
                            .original
                            .message_id
                            .clone()
                            .expect("held message identity"),
                        original: held.original.clone().into(),
                        draft: PersistedSoftInterruptDraft {
                            content: held.draft.content.clone(),
                            images: held.draft.images.clone(),
                        },
                        original_relative_index: index,
                    })
                    .collect(),
                completed_operations: coordinator
                    .completed
                    .iter()
                    .filter(|completed| {
                        completed.navigation_session_id == *navigation_session_id
                            && completed.owner_client_instance_id
                                == reservation.owner_client_instance_id
                    })
                    .map(|completed| PersistedCompletedEditorOperation {
                        operation_id: completed.operation_id.clone(),
                        request_fingerprint: completed.request_fingerprint.clone(),
                        outcome: serde_json::to_value(&completed.result.outcome)
                            .unwrap_or(serde_json::Value::Null),
                    })
                    .collect(),
            },
        )
        .collect();
    crate::soft_interrupt_store::overwrite_envelope(
        session_id,
        &SoftInterruptStoreEnvelope {
            version: 1,
            dispatchable: queue.iter().cloned().map(Into::into).collect(),
            reservations,
        },
    )
    .map_err(|error| format!("failed to persist queued message editor state: {error}"))
}

pub(crate) fn persist_session_soft_interrupt_state(
    session_id: &str,
    queue: &SoftInterruptQueue,
) -> Result<(), String> {
    let coordinator = queued_message_editor_for_session(session_id);
    let coordinator = coordinator
        .lock()
        .map_err(|_| "queued message editor authority is unavailable".to_string())?;
    let queue = queue
        .lock()
        .map_err(|_| "soft interrupt queue is unavailable".to_string())?;
    persist_queued_message_editor_state(session_id, &queue, &coordinator)
}

#[cfg(test)]
fn persist_queued_message_editor_state(
    _session_id: &str,
    _queue: &[SoftInterruptMessage],
    _coordinator: &QueuedMessageEditorCoordinator,
) -> Result<(), String> {
    Ok(())
}

fn soft_interrupt_replay_for_session(session_id: &str) -> SharedSoftInterruptReplay {
    SOFT_INTERRUPT_REPLAYS
        .lock()
        .map(|mut replays| replays.get_or_insert(session_id))
        .unwrap_or_else(|_| Arc::new(StdMutex::new(SoftInterruptReplay::default())))
}

/// Register (or replace) the background-tool signal for a session.
pub(super) fn register_background_tool_signal(session_id: &str, signal: InterruptSignal) {
    if let Ok(mut map) = BACKGROUND_TOOL_SIGNALS.lock() {
        map.insert(session_id.to_string(), signal);
    }
}

/// Look up the registered background-tool signal for a session, if any.
pub(super) fn background_tool_signal_for_session(session_id: &str) -> Option<InterruptSignal> {
    BACKGROUND_TOOL_SIGNALS
        .lock()
        .ok()
        .and_then(|map| map.get(session_id).cloned())
}

/// Move a session's background-tool signal registration to a new session id.
pub(super) fn rename_background_tool_signal(old_session_id: &str, new_session_id: &str) {
    if old_session_id == new_session_id {
        return;
    }
    if let Ok(mut map) = BACKGROUND_TOOL_SIGNALS.lock()
        && let Some(signal) = map.remove(old_session_id)
    {
        map.insert(new_session_id.to_string(), signal);
    }
}

/// Drop a session's background-tool signal registration.
pub(super) fn remove_background_tool_signal(session_id: &str) {
    if let Ok(mut map) = BACKGROUND_TOOL_SIGNALS.lock() {
        map.remove(session_id);
    }
}

/// Record of a file access by an agent
#[derive(Clone, Debug)]
pub struct FileAccess {
    pub session_id: String,
    pub op: FileOp,
    pub timestamp: Instant,
    pub absolute_time: std::time::SystemTime,
    pub intent: Option<String>,
    pub summary: Option<String>,
    pub detail: Option<String>,
}

pub(super) fn latest_peer_touches(
    accesses: &[FileAccess],
    current_session_id: &str,
    swarm_session_ids: &HashSet<String>,
) -> Vec<FileAccess> {
    let mut latest_by_session: HashMap<&str, &FileAccess> = HashMap::new();

    for access in accesses.iter().filter(|access| {
        access.session_id != current_session_id
            && swarm_session_ids.contains(&access.session_id)
            && access.op.is_modification()
    }) {
        latest_by_session
            .entry(&access.session_id)
            .and_modify(|existing| {
                if access.timestamp > existing.timestamp {
                    *existing = access;
                }
            })
            .or_insert(access);
    }

    let mut latest: Vec<FileAccess> = latest_by_session.into_values().cloned().collect();
    latest.sort_by(|left, right| left.session_id.cmp(&right.session_id));
    latest
}

/// Shared ownership of the core persisted swarm coordination state.
#[derive(Clone)]
pub struct SwarmState {
    pub members: Arc<RwLock<HashMap<String, SwarmMember>>>,
    pub swarms_by_id: Arc<RwLock<HashMap<String, HashSet<String>>>>,
    pub plans: Arc<RwLock<HashMap<String, VersionedPlan>>>,
    pub coordinators: Arc<RwLock<HashMap<String, String>>>,
}

/// First-class snapshot of a single swarm's logical runtime state.
#[derive(Clone, Debug)]
pub struct SwarmRuntime {
    pub swarm_id: String,
    pub coordinator_session_id: Option<String>,
    pub member_session_ids: HashSet<String>,
    pub members: Vec<SwarmMember>,
    pub plan: Option<VersionedPlan>,
}

impl SwarmRuntime {
    pub fn has_any_state(&self) -> bool {
        self.plan.is_some() || self.coordinator_session_id.is_some() || !self.members.is_empty()
    }
}

/// Live transport attachment for a connected session.
#[derive(Clone, Debug)]
pub struct LiveSessionAttachment {
    pub connection_id: String,
    pub event_tx: mpsc::UnboundedSender<ServerEvent>,
}

impl SwarmState {
    pub fn new(
        members: HashMap<String, SwarmMember>,
        swarms_by_id: HashMap<String, HashSet<String>>,
        plans: HashMap<String, VersionedPlan>,
        coordinators: HashMap<String, String>,
    ) -> Self {
        Self {
            members: Arc::new(RwLock::new(members)),
            swarms_by_id: Arc::new(RwLock::new(swarms_by_id)),
            plans: Arc::new(RwLock::new(plans)),
            coordinators: Arc::new(RwLock::new(coordinators)),
        }
    }

    pub async fn load_runtime(&self, swarm_id: &str) -> SwarmRuntime {
        let plan = {
            let plans = self.plans.read().await;
            plans.get(swarm_id).cloned()
        };
        let coordinator_session_id = {
            let coordinators = self.coordinators.read().await;
            coordinators.get(swarm_id).cloned()
        };
        let member_session_ids = {
            let swarms = self.swarms_by_id.read().await;
            swarms.get(swarm_id).cloned().unwrap_or_default()
        };
        let mut members = {
            let members = self.members.read().await;
            members
                .values()
                .filter(|member| member.swarm_id.as_deref() == Some(swarm_id))
                .cloned()
                .collect::<Vec<_>>()
        };
        members.sort_by(|left, right| left.session_id.cmp(&right.session_id));

        SwarmRuntime {
            swarm_id: swarm_id.to_string(),
            coordinator_session_id,
            member_session_ids,
            members,
            plan,
        }
    }
}

/// Information about a session in a swarm
#[derive(Clone, Debug)]
pub struct SwarmMember {
    pub session_id: String,
    /// Primary channel to send events to this session.
    ///
    /// This remains for backward-compatible single-sender call sites and for
    /// headless sessions that do not maintain a live attachment map.
    pub event_tx: mpsc::UnboundedSender<ServerEvent>,
    /// Live client attachments for this session keyed by connection id.
    pub event_txs: HashMap<String, mpsc::UnboundedSender<ServerEvent>>,
    /// Working directory (used to derive swarm id)
    pub working_dir: Option<PathBuf>,
    /// Swarm identifier (shared across worktrees)
    pub swarm_id: Option<String>,
    /// Whether swarm coordination is enabled for this member
    pub swarm_enabled: bool,
    /// Lifecycle status (ready, running, completed, failed, stopped, etc.)
    pub status: String,
    /// Optional detail (current task, error, etc.)
    pub detail: Option<String>,
    /// Stable, human-readable label of the task/role this member was spawned
    /// or assigned for (compacted from the spawn prompt or plan item). Unlike
    /// `detail`, this is not overwritten by transient status updates.
    pub task_label: Option<String>,
    /// Friendly name like "fox"
    pub friendly_name: Option<String>,
    /// Session that should receive direct completion report-back for this member, if any.
    pub report_back_to_session_id: Option<String>,
    /// Latest explicit completion report submitted by this member.
    pub latest_completion_report: Option<String>,
    /// Role: "agent" or "coordinator"
    pub role: String,
    /// When this member joined the swarm
    pub joined_at: Instant,
    /// When status was last changed
    pub last_status_change: Instant,
    /// Whether this is a headless (spawned) session vs a TUI-connected session.
    /// Headless sessions should not be automatically elected as coordinator.
    pub is_headless: bool,
    /// Recent streamed output tail (last few lines of in-progress assistant
    /// text), captured for inline swarm gallery rendering. Updated by the bus
    /// monitor from worker streaming taps; not persisted.
    pub output_tail: Option<String>,
    /// Aggregate todo progress (completed, total) for this member's session,
    /// updated from `TodoUpdated` bus events. Surfaced on the inline swarm
    /// strip; not persisted.
    pub todo_progress: Option<(u32, u32)>,
    /// Compact snapshot of this member's todo list (content + status), capped
    /// at a few entries by the bus monitor. Rendered in the focused inline
    /// swarm panel; not persisted.
    pub todo_items: Vec<crate::protocol::SwarmTodoItem>,
    /// Ephemeral model/timing metadata for the inline swarm card.
    pub runtime: crate::protocol::SwarmMemberRuntime,
}

impl SwarmMember {
    pub fn durable_record(&self) -> SwarmMemberRecord {
        SwarmMemberRecord {
            session_id: self.session_id.clone(),
            working_dir: self.working_dir.clone(),
            swarm_id: self.swarm_id.clone(),
            swarm_enabled: self.swarm_enabled,
            status: SwarmLifecycleStatus::from(self.status.clone()),
            detail: self.detail.clone(),
            task_label: self.task_label.clone(),
            friendly_name: self.friendly_name.clone(),
            report_back_to_session_id: self.report_back_to_session_id.clone(),
            latest_completion_report: self.latest_completion_report.clone(),
            role: SwarmRole::from(self.role.clone()),
            is_headless: self.is_headless,
        }
    }

    pub fn live_attachments(&self) -> Vec<LiveSessionAttachment> {
        self.event_txs
            .iter()
            .map(|(connection_id, event_tx)| LiveSessionAttachment {
                connection_id: connection_id.clone(),
                event_tx: event_tx.clone(),
            })
            .collect()
    }

    pub fn from_record(
        record: SwarmMemberRecord,
        event_tx: mpsc::UnboundedSender<ServerEvent>,
    ) -> Self {
        Self {
            session_id: record.session_id,
            event_tx,
            event_txs: HashMap::new(),
            working_dir: record.working_dir,
            swarm_id: record.swarm_id,
            swarm_enabled: record.swarm_enabled,
            status: record.status.as_str().into_owned(),
            detail: record.detail,
            task_label: record.task_label,
            friendly_name: record.friendly_name,
            report_back_to_session_id: record.report_back_to_session_id,
            latest_completion_report: record.latest_completion_report,
            role: record.role.as_str().into_owned(),
            joined_at: Instant::now(),
            last_status_change: Instant::now(),
            is_headless: record.is_headless,
            output_tail: None,
            todo_progress: None,
            todo_items: Vec::new(),
            runtime: crate::protocol::SwarmMemberRuntime::default(),
        }
    }
}

/// A shared context entry stored by the server
#[derive(Clone, Debug)]
pub struct SharedContext {
    pub key: String,
    pub value: String,
    pub from_session: String,
    pub from_name: Option<String>,
    /// When this context was created
    pub created_at: Instant,
    /// When this context was last updated
    pub updated_at: Instant,
}

/// Event types for real-time event subscription
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SwarmEventType {
    /// A file was touched (read/write/edit)
    FileTouch {
        path: String,
        op: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        intent: Option<String>,
        summary: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    /// A notification was broadcast
    Notification {
        notification_type: String,
        message: String,
    },
    /// A swarm plan was updated
    PlanUpdate { swarm_id: String, item_count: usize },
    /// A plan proposal was submitted
    PlanProposal {
        swarm_id: String,
        proposer_session: String,
        item_count: usize,
    },
    /// Shared context was updated
    ContextUpdate { swarm_id: String, key: String },
    /// Session status changed
    StatusChange {
        old_status: String,
        new_status: String,
    },
    /// Session joined/left swarm
    MemberChange {
        action: String, // "joined" or "left"
    },
}

/// A swarm event with metadata
#[derive(Clone, Debug)]
pub struct SwarmEvent {
    pub id: u64,
    pub session_id: String,
    pub session_name: Option<String>,
    pub swarm_id: Option<String>,
    pub event: SwarmEventType,
    pub timestamp: Instant,
    pub absolute_time: std::time::SystemTime,
}

/// Ring buffer for recent swarm events
pub(super) const MAX_EVENT_HISTORY: usize = 5000;

pub(super) type SessionInterruptQueues = Arc<RwLock<HashMap<String, SoftInterruptQueue>>>;

pub(super) async fn register_session_event_sender(
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
    session_id: &str,
    connection_id: &str,
    event_tx: mpsc::UnboundedSender<ServerEvent>,
) {
    let mut members = swarm_members.write().await;
    if let Some(member) = members.get_mut(session_id) {
        member.event_tx = event_tx.clone();
        member.event_txs.insert(connection_id.to_string(), event_tx);
    }
}

pub(super) async fn unregister_session_event_sender(
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
    session_id: &str,
    connection_id: &str,
) {
    let mut members = swarm_members.write().await;
    if let Some(member) = members.get_mut(session_id) {
        member.event_txs.remove(connection_id);
        if let Some((_, tx)) = member.event_txs.iter().next() {
            member.event_tx = tx.clone();
        }
    }
}

pub(super) async fn fanout_session_event(
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
    session_id: &str,
    event: ServerEvent,
) -> usize {
    let targets = {
        let mut members = swarm_members.write().await;
        let Some(member) = members.get_mut(session_id) else {
            return 0;
        };

        member.event_txs.retain(|_, tx| !tx.is_closed());

        if member.event_txs.is_empty() {
            vec![member.event_tx.clone()]
        } else {
            if let Some((_, tx)) = member.event_txs.iter().next() {
                member.event_tx = tx.clone();
            }
            member.event_txs.values().cloned().collect::<Vec<_>>()
        }
    };

    let mut delivered = 0;
    for tx in targets {
        if tx.send(event.clone()).is_ok() {
            delivered += 1;
        }
    }
    delivered
}

pub(super) async fn fanout_live_client_event(
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
    session_id: &str,
    event: ServerEvent,
) -> usize {
    let targets = {
        let mut members = swarm_members.write().await;
        let Some(member) = members.get_mut(session_id) else {
            return 0;
        };

        member.event_txs.retain(|_, tx| !tx.is_closed());
        member.event_txs.values().cloned().collect::<Vec<_>>()
    };

    let mut delivered = 0;
    for tx in targets {
        if tx.send(event.clone()).is_ok() {
            delivered += 1;
        }
    }
    delivered
}

pub(super) fn session_event_fanout_sender(
    session_id: String,
    swarm_members: Arc<RwLock<HashMap<String, SwarmMember>>>,
) -> mpsc::UnboundedSender<ServerEvent> {
    let (tx, mut rx) = mpsc::unbounded_channel::<ServerEvent>();
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            let _ = fanout_session_event(&swarm_members, &session_id, event).await;
        }
    });
    tx
}

pub(super) fn session_event_fanout_sender_with_fallback(
    session_id: String,
    swarm_members: Arc<RwLock<HashMap<String, SwarmMember>>>,
    fallback_tx: mpsc::UnboundedSender<ServerEvent>,
) -> mpsc::UnboundedSender<ServerEvent> {
    let (tx, mut rx) = mpsc::unbounded_channel::<ServerEvent>();
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if fanout_session_event(&swarm_members, &session_id, event.clone()).await == 0 {
                let _ = fallback_tx.send(event);
            }
        }
    });
    tx
}

fn enqueue_soft_interrupt(
    queue: &SoftInterruptQueue,
    coordinator: &SharedQueuedMessageEditorCoordinator,
    content: String,
    images: Vec<(String, String)>,
    urgent: bool,
    source: SoftInterruptSource,
) -> bool {
    enqueue_soft_interrupt_owned(queue, coordinator, content, images, urgent, source, None)
}

fn enqueue_soft_interrupt_owned(
    queue: &SoftInterruptQueue,
    coordinator: &SharedQueuedMessageEditorCoordinator,
    content: String,
    images: Vec<(String, String)>,
    urgent: bool,
    source: SoftInterruptSource,
    owner_client_instance_id: Option<&str>,
) -> bool {
    let content_bytes = content.len();
    let content_chars = content.chars().count();
    let Ok(mut coordinator) = coordinator.lock() else {
        crate::logging::warn(&format!(
            "SOFT_INTERRUPT_QUEUE_PUSH_FAILED source={:?} urgent={} content_bytes={} content_chars={} reason=editor_lock_poisoned",
            source, urgent, content_bytes, content_chars
        ));
        return false;
    };
    if let Ok(mut pending) = queue.lock() {
        let pending_before = pending.len();
        let queued_max = pending
            .iter()
            .filter_map(|message| message.enqueue_sequence)
            .max()
            .unwrap_or(0);
        coordinator.next_enqueue_sequence = coordinator.next_enqueue_sequence.max(queued_max);
        coordinator.next_enqueue_sequence = coordinator.next_enqueue_sequence.saturating_add(1);
        let enqueue_sequence = coordinator.next_enqueue_sequence;
        pending.push(SoftInterruptMessage {
            content,
            images,
            urgent,
            source,
            message_id: Some(crate::id::new_id("soft_interrupt")),
            owner_client_instance_id: owner_client_instance_id.map(str::to_string),
            enqueue_sequence: Some(enqueue_sequence),
        });
        crate::logging::info(&format!(
            "SOFT_INTERRUPT_QUEUE_PUSH source={:?} urgent={} content_bytes={} content_chars={} pending_before={} pending_after={}",
            source,
            urgent,
            content_bytes,
            content_chars,
            pending_before,
            pending.len()
        ));
        true
    } else {
        crate::logging::warn(&format!(
            "SOFT_INTERRUPT_QUEUE_PUSH_FAILED source={:?} urgent={} content_bytes={} content_chars={} reason=queue_lock_poisoned",
            source, urgent, content_bytes, content_chars
        ));
        false
    }
}

/// Lock-free control-plane handles for a live session.
///
/// This intentionally exposes only out-of-band controls that are safe to use
/// while a turn owns the Agent mutex. Stateful operations such as history
/// mutation, model changes, or direct tool execution should continue to
/// coordinate through the Agent lock after the turn is idle/stopped.
#[derive(Clone)]
pub struct SessionControlHandle {
    pub session_id: String,
    soft_interrupt_queue: SoftInterruptQueue,
    soft_interrupt_replay: SharedSoftInterruptReplay,
    queued_message_editor: SharedQueuedMessageEditorCoordinator,
    background_tool_signal: Option<InterruptSignal>,
    stop_current_turn_signal: InterruptSignal,
}

impl SessionControlHandle {
    pub fn new(
        session_id: impl Into<String>,
        soft_interrupt_queue: SoftInterruptQueue,
        background_tool_signal: InterruptSignal,
        stop_current_turn_signal: InterruptSignal,
    ) -> Self {
        let session_id = session_id.into();
        // Mirror the signal into the process-global registry so the lock-free
        // `cancel_only` fallback (used while the agent mutex is busy, e.g. during
        // `await_members`) can still fire it. Without this, Alt+B/Ctrl+B silently
        // no-ops for busy turns.
        register_background_tool_signal(&session_id, background_tool_signal.clone());
        let soft_interrupt_replay = soft_interrupt_replay_for_session(&session_id);
        let queued_message_editor = queued_message_editor_for_session(&session_id);
        Self {
            session_id,
            soft_interrupt_queue,
            soft_interrupt_replay,
            queued_message_editor,
            background_tool_signal: Some(background_tool_signal),
            stop_current_turn_signal,
        }
    }

    pub fn cancel_only(
        session_id: impl Into<String>,
        soft_interrupt_queue: SoftInterruptQueue,
        stop_current_turn_signal: InterruptSignal,
    ) -> Self {
        let session_id = session_id.into();
        let soft_interrupt_replay = soft_interrupt_replay_for_session(&session_id);
        let queued_message_editor = queued_message_editor_for_session(&session_id);
        Self {
            session_id,
            soft_interrupt_queue,
            soft_interrupt_replay,
            queued_message_editor,
            background_tool_signal: None,
            stop_current_turn_signal,
        }
    }

    pub fn queue_soft_interrupt(
        &self,
        content: String,
        images: Vec<(String, String)>,
        urgent: bool,
        source: SoftInterruptSource,
    ) -> bool {
        enqueue_soft_interrupt(
            &self.soft_interrupt_queue,
            &self.queued_message_editor,
            content,
            images,
            urgent,
            source,
        )
    }

    pub fn queue_owned_soft_interrupt(
        &self,
        content: String,
        images: Vec<(String, String)>,
        urgent: bool,
        source: SoftInterruptSource,
        owner_client_instance_id: Option<&str>,
    ) -> bool {
        let queued = enqueue_soft_interrupt_owned(
            &self.soft_interrupt_queue,
            &self.queued_message_editor,
            content,
            images,
            urgent,
            source,
            owner_client_instance_id,
        );
        if queued
            && let Err(error) =
                persist_session_soft_interrupt_state(&self.session_id, &self.soft_interrupt_queue)
        {
            crate::logging::warn(&format!(
                "Failed to persist queued soft interrupt with editor state for {}: {}",
                self.session_id, error
            ));
        }
        queued
    }

    pub fn clear_soft_interrupts(&self) -> bool {
        if let Ok(mut queue) = self.soft_interrupt_queue.lock() {
            let cleared = queue.len();
            queue.clear();
            crate::logging::info(&format!(
                "SOFT_INTERRUPT_QUEUE_CLEAR session={} cleared={}",
                self.session_id, cleared
            ));
        } else {
            crate::logging::warn(&format!(
                "SOFT_INTERRUPT_QUEUE_CLEAR_FAILED session={} reason=queue_lock_poisoned",
                self.session_id
            ));
        }
        match persist_session_soft_interrupt_state(&self.session_id, &self.soft_interrupt_queue) {
            Ok(()) => true,
            Err(error) => {
                crate::logging::warn(&format!(
                    "Failed to persist cleared soft interrupts with editor state for {}: {}",
                    self.session_id, error
                ));
                false
            }
        }
    }

    pub fn recall_soft_interrupt(
        &self,
        client_instance_id: &str,
        operation_id: &str,
    ) -> Option<RecallableSoftInterrupt> {
        // Keep the replay lock through selection and recording so concurrent
        // retries of one operation cannot both remove a queue entry.
        let mut replay = self.soft_interrupt_replay.lock().ok()?;
        if let Some(result) = replay.get(client_instance_id, operation_id) {
            return result;
        }

        let result = {
            let mut queue = self.soft_interrupt_queue.lock().ok()?;
            let index = queue.iter().rposition(|message| {
                message.source == SoftInterruptSource::User
                    && message.owner_client_instance_id.as_deref() == Some(client_instance_id)
            });
            index.map(|index| {
                let message = queue.remove(index);
                RecallableSoftInterrupt {
                    content: message.content,
                    images: message.images,
                }
            })
        };

        replay.insert(client_instance_id, operation_id, result.clone());
        result
    }

    pub(crate) fn queued_message_editor(
        &self,
        client_instance_id: &str,
        navigation_session_id: &str,
        operation_id: &str,
        operation: QueuedMessageEditorOperation,
    ) -> Result<QueuedMessageEditorResult, String> {
        if client_instance_id.trim().is_empty()
            || navigation_session_id.trim().is_empty()
            || operation_id.trim().is_empty()
        {
            return Err("queued message editor identities must not be empty".to_string());
        }
        let fingerprint = editor_fingerprint(&operation)?;
        let mut coordinator = self
            .queued_message_editor
            .lock()
            .map_err(|_| "queued message editor authority is unavailable".to_string())?;

        if let Some(completed) = coordinator.completed.iter().find(|completed| {
            completed.navigation_session_id == navigation_session_id
                && completed.operation_id == operation_id
        }) {
            if completed.owner_client_instance_id != client_instance_id {
                return Err("queued message editor session is not owned by this client".to_string());
            }
            if completed.request_fingerprint != fingerprint {
                return Err(
                    "queued message editor operation identity was reused with different input"
                        .to_string(),
                );
            }
            let mut replay = completed.result.clone();
            replay.outcome = QueuedMessageEditorOutcome::Replay;
            replay.message = Some("replayed completed queued message editor operation".to_string());
            return Ok(replay);
        }

        let mut queue = self
            .soft_interrupt_queue
            .lock()
            .map_err(|_| "soft interrupt queue is unavailable".to_string())?;
        let coordinator_before = coordinator.clone();
        let queue_before = queue.clone();

        let result = match operation {
            QueuedMessageEditorOperation::Start => {
                if let Some(existing) = coordinator.reservations.get(navigation_session_id) {
                    if existing.owner_client_instance_id != client_instance_id {
                        return Err(
                            "queued message editor session is not owned by this client".to_string()
                        );
                    }
                    return Err("queued message editor session already exists".to_string());
                }
                if coordinator
                    .reservations
                    .values()
                    .any(|reservation| reservation.owner_client_instance_id == client_instance_id)
                {
                    return Err(
                        "client already owns an active queued message editor session".to_string(),
                    );
                }

                let eligible_indices: Vec<usize> = queue
                    .iter()
                    .enumerate()
                    .filter_map(|(index, message)| {
                        (message.source == SoftInterruptSource::User
                            && message.owner_client_instance_id.as_deref()
                                == Some(client_instance_id)
                            && message.message_id.is_some()
                            && message.enqueue_sequence.is_some())
                        .then_some(index)
                    })
                    .collect();
                if eligible_indices.is_empty() {
                    QueuedMessageEditorResult {
                        outcome: QueuedMessageEditorOutcome::Boundary,
                        selection: None,
                        placement: QueuedMessageEditorPlacement::Exact,
                        message: Some("no eligible queued user messages".to_string()),
                    }
                } else {
                    let first = eligible_indices[0];
                    let last = *eligible_indices
                        .last()
                        .expect("non-empty eligible snapshot");
                    let predecessor_message_id = first
                        .checked_sub(1)
                        .and_then(|index| queue.get(index))
                        .and_then(|message| message.message_id.clone());
                    let successor_message_id = queue
                        .get(last + 1)
                        .and_then(|message| message.message_id.clone());
                    let snapshot_queue_sequence = queue
                        .iter()
                        .filter_map(|message| message.enqueue_sequence)
                        .max()
                        .unwrap_or(0);
                    coordinator.next_enqueue_sequence = coordinator
                        .next_enqueue_sequence
                        .max(snapshot_queue_sequence);
                    let eligible: HashSet<usize> = eligible_indices.into_iter().collect();
                    let mut held = Vec::new();
                    let mut remaining = Vec::with_capacity(queue.len() - eligible.len());
                    for (index, message) in std::mem::take(&mut *queue).into_iter().enumerate() {
                        if eligible.contains(&index) {
                            held.push(HeldQueuedMessage {
                                draft: RecallableSoftInterrupt {
                                    content: message.content.clone(),
                                    images: message.images.clone(),
                                },
                                original: message,
                            });
                        } else {
                            remaining.push(message);
                        }
                    }
                    *queue = remaining;
                    let reservation = QueuedMessageReservation {
                        owner_client_instance_id: client_instance_id.to_string(),
                        snapshot_queue_sequence,
                        selected_index: held.len() - 1,
                        predecessor_message_id,
                        successor_message_id,
                        held,
                    };
                    let selection = editor_selection(&reservation);
                    coordinator
                        .reservations
                        .insert(navigation_session_id.to_string(), reservation);
                    QueuedMessageEditorResult {
                        outcome: QueuedMessageEditorOutcome::Started,
                        selection: Some(selection),
                        placement: QueuedMessageEditorPlacement::Exact,
                        message: None,
                    }
                }
            }
            QueuedMessageEditorOperation::Move {
                direction,
                selected_message_id,
                draft,
            } => {
                let reservation = coordinator
                    .reservations
                    .get_mut(navigation_session_id)
                    .ok_or_else(|| "queued message editor session does not exist".to_string())?;
                if reservation.owner_client_instance_id != client_instance_id {
                    return Err(
                        "queued message editor session is not owned by this client".to_string()
                    );
                }
                let current_id = reservation.held[reservation.selected_index]
                    .original
                    .message_id
                    .as_deref();
                if current_id != Some(selected_message_id.as_str()) {
                    QueuedMessageEditorResult {
                        outcome: QueuedMessageEditorOutcome::Conflict,
                        selection: Some(editor_selection(reservation)),
                        placement: QueuedMessageEditorPlacement::NotApplied,
                        message: Some(
                            "queued message editor selection changed; draft was not applied"
                                .to_string(),
                        ),
                    }
                } else {
                    reservation.held[reservation.selected_index].draft = draft;
                    let next_index = match direction {
                        QueuedMessageEditorDirection::Older => {
                            reservation.selected_index.checked_sub(1)
                        }
                        QueuedMessageEditorDirection::Newer => (reservation.selected_index + 1
                            < reservation.held.len())
                        .then_some(reservation.selected_index + 1),
                    };
                    let outcome = if let Some(next_index) = next_index {
                        reservation.selected_index = next_index;
                        QueuedMessageEditorOutcome::Moved
                    } else {
                        QueuedMessageEditorOutcome::Boundary
                    };
                    QueuedMessageEditorResult {
                        outcome,
                        selection: Some(editor_selection(reservation)),
                        placement: QueuedMessageEditorPlacement::Exact,
                        message: (outcome == QueuedMessageEditorOutcome::Boundary)
                            .then(|| "queued message editor boundary reached".to_string()),
                    }
                }
            }
            QueuedMessageEditorOperation::Finish {
                selected_message_id,
                draft,
            } => {
                let mut reservation = coordinator
                    .reservations
                    .remove(navigation_session_id)
                    .ok_or_else(|| "queued message editor session does not exist".to_string())?;
                if reservation.owner_client_instance_id != client_instance_id {
                    coordinator
                        .reservations
                        .insert(navigation_session_id.to_string(), reservation);
                    return Err(
                        "queued message editor session is not owned by this client".to_string()
                    );
                }
                let current_id = reservation.held[reservation.selected_index]
                    .original
                    .message_id
                    .as_deref();
                if current_id != Some(selected_message_id.as_str()) {
                    let selection = editor_selection(&reservation);
                    coordinator
                        .reservations
                        .insert(navigation_session_id.to_string(), reservation);
                    QueuedMessageEditorResult {
                        outcome: QueuedMessageEditorOutcome::Conflict,
                        selection: Some(selection),
                        placement: QueuedMessageEditorPlacement::NotApplied,
                        message: Some(
                            "queued message editor selection changed; finish was not applied"
                                .to_string(),
                        ),
                    }
                } else if draft.content.is_empty() && draft.images.is_empty() {
                    reservation.held.remove(reservation.selected_index);
                    let exact = if reservation.held.is_empty() {
                        let (_, exact) = restoration_position(&queue, &reservation);
                        exact
                    } else {
                        if reservation.selected_index >= reservation.held.len() {
                            reservation.selected_index = reservation.held.len() - 1;
                        }
                        restore_reservation(&mut queue, reservation, None)
                    };
                    QueuedMessageEditorResult {
                        outcome: if exact {
                            QueuedMessageEditorOutcome::Deleted
                        } else {
                            QueuedMessageEditorOutcome::StalePlacement
                        },
                        selection: None,
                        placement: if exact {
                            QueuedMessageEditorPlacement::Exact
                        } else {
                            QueuedMessageEditorPlacement::StaleBestEffort
                        },
                        message: (!exact).then(|| {
                            "selected queued message deleted with stale best-effort placement"
                                .to_string()
                        }),
                    }
                } else {
                    let exact = restore_reservation(&mut queue, reservation, Some(draft));
                    QueuedMessageEditorResult {
                        outcome: if exact {
                            QueuedMessageEditorOutcome::Committed
                        } else {
                            QueuedMessageEditorOutcome::StalePlacement
                        },
                        selection: None,
                        placement: if exact {
                            QueuedMessageEditorPlacement::Exact
                        } else {
                            QueuedMessageEditorPlacement::StaleBestEffort
                        },
                        message: (!exact).then(|| {
                            "queued message committed with stale best-effort placement".to_string()
                        }),
                    }
                }
            }
            QueuedMessageEditorOperation::Release => {
                let reservation = coordinator
                    .reservations
                    .remove(navigation_session_id)
                    .ok_or_else(|| "queued message editor session does not exist".to_string())?;
                if reservation.owner_client_instance_id != client_instance_id {
                    coordinator
                        .reservations
                        .insert(navigation_session_id.to_string(), reservation);
                    return Err(
                        "queued message editor session is not owned by this client".to_string()
                    );
                }
                let exact = restore_reservation(&mut queue, reservation, None);
                QueuedMessageEditorResult {
                    outcome: QueuedMessageEditorOutcome::Released,
                    selection: None,
                    placement: if exact {
                        QueuedMessageEditorPlacement::Exact
                    } else {
                        QueuedMessageEditorPlacement::StaleBestEffort
                    },
                    message: (!exact).then(|| {
                        "queued messages released with stale best-effort placement".to_string()
                    }),
                }
            }
        };

        if coordinator.completed.len() == QUEUED_MESSAGE_EDITOR_REPLAY_CAPACITY {
            coordinator.completed.pop_front();
        }
        coordinator
            .completed
            .push_back(CompletedQueuedMessageEditorOperation {
                owner_client_instance_id: client_instance_id.to_string(),
                navigation_session_id: navigation_session_id.to_string(),
                operation_id: operation_id.to_string(),
                request_fingerprint: fingerprint,
                result: result.clone(),
            });
        if let Err(error) =
            persist_queued_message_editor_state(&self.session_id, &queue, &coordinator)
        {
            *coordinator = coordinator_before;
            *queue = queue_before;
            return Err(error);
        }
        Ok(result)
    }

    pub(crate) fn resume_queued_message_editor_owner(&self, client_instance_id: &str) {
        if let Ok(mut coordinator) = self.queued_message_editor.lock() {
            coordinator.grace_tokens.remove(client_instance_id);
        }
    }

    pub(crate) fn begin_queued_message_editor_disconnect_grace(
        &self,
        client_instance_id: String,
        grace: std::time::Duration,
    ) {
        let token = {
            let Ok(mut coordinator) = self.queued_message_editor.lock() else {
                return;
            };
            if !coordinator
                .reservations
                .values()
                .any(|reservation| reservation.owner_client_instance_id == client_instance_id)
            {
                return;
            }
            coordinator.next_grace_token = coordinator.next_grace_token.wrapping_add(1);
            let token = coordinator.next_grace_token;
            coordinator
                .grace_tokens
                .insert(client_instance_id.clone(), token);
            token
        };
        let control = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(grace).await;
            let Ok(mut coordinator) = control.queued_message_editor.lock() else {
                return;
            };
            if coordinator.grace_tokens.get(&client_instance_id) != Some(&token) {
                return;
            }
            coordinator.grace_tokens.remove(&client_instance_id);
            let navigation_ids: Vec<String> = coordinator
                .reservations
                .iter()
                .filter_map(|(navigation_id, reservation)| {
                    (reservation.owner_client_instance_id == client_instance_id)
                        .then_some(navigation_id.clone())
                })
                .collect();
            let Ok(mut queue) = control.soft_interrupt_queue.lock() else {
                return;
            };
            for navigation_id in navigation_ids {
                if let Some(reservation) = coordinator.reservations.remove(&navigation_id) {
                    restore_reservation(&mut queue, reservation, None);
                }
            }
            if let Err(error) =
                persist_queued_message_editor_state(&control.session_id, &queue, &coordinator)
            {
                crate::logging::warn(&format!(
                    "Failed to persist queued editor disconnect release for {}: {}",
                    control.session_id, error
                ));
            }
        });
    }

    /// Fire the stop-current-turn signal. Returns the signal's fire epoch so
    /// callers that schedule a deferred [`reset_cancel_if_epoch`](Self::reset_cancel_if_epoch)
    /// can avoid erasing a newer cancel that fired in the meantime (issue #428).
    ///
    /// Also fires every cancel signal registered for currently running turns
    /// of this session. The handle's own signal can be a stale instance that
    /// the streaming turn never observes (reattach after reload/disconnect,
    /// server-initiated turns, headless recovery), which used to make Esc show
    /// "Interrupting..." while the model kept generating for minutes
    /// (issue #428).
    pub fn request_cancel(&self) -> u64 {
        crate::logging::info(&format!(
            "SESSION_CANCEL_SIGNAL_FIRE session={}",
            self.session_id
        ));
        self.stop_current_turn_signal.fire();
        let active_turn_signals =
            crate::turn_cancel_registry::active_turn_signals(&self.session_id);
        let mut fired_active = 0usize;
        for signal in &active_turn_signals {
            if signal.same_instance(&self.stop_current_turn_signal) {
                continue;
            }
            signal.fire();
            fired_active += 1;
        }
        if fired_active > 0 {
            crate::logging::info(&format!(
                "SESSION_CANCEL_ACTIVE_TURN_SIGNALS_FIRED session={} fired={} registered={}",
                self.session_id,
                fired_active,
                active_turn_signals.len()
            ));
        }
        self.stop_current_turn_signal.epoch()
    }

    pub fn reset_cancel(&self) {
        crate::logging::info(&format!(
            "SESSION_CANCEL_SIGNAL_RESET session={}",
            self.session_id
        ));
        self.stop_current_turn_signal.reset();
    }

    /// Reset the cancel signal only if no newer cancel fired since `epoch`
    /// was captured from [`request_cancel`](Self::request_cancel). Timed
    /// resets (used when the running turn is not owned by this connection)
    /// must use this instead of [`reset_cancel`](Self::reset_cancel):
    /// an unconditional deferred reset can erase a newer, not-yet-observed
    /// cancel, making repeated Esc presses appear to be ignored (issue #428).
    pub fn reset_cancel_if_epoch(&self, epoch: u64) -> bool {
        let reset = self.stop_current_turn_signal.reset_if_epoch(epoch);
        crate::logging::info(&format!(
            "SESSION_CANCEL_SIGNAL_RESET session={} epoch={} applied={}",
            self.session_id, epoch, reset
        ));
        reset
    }

    pub fn request_background_current_tool(&self) -> bool {
        // Prefer the directly-held signal; fall back to the process-global
        // registry for lock-free (`cancel_only`) handles built while the agent
        // mutex was busy. This is what makes Alt+B/Ctrl+B work during a busy
        // turn such as `await_members`.
        let signal = self
            .background_tool_signal
            .clone()
            .or_else(|| background_tool_signal_for_session(&self.session_id));
        if let Some(signal) = signal {
            signal.fire();
            crate::logging::info(&format!(
                "BACKGROUND_TOOL_SIGNAL_FIRE session={} result=sent",
                self.session_id
            ));
            true
        } else {
            crate::logging::warn(&format!(
                "BACKGROUND_TOOL_SIGNAL_FIRE session={} result=no_signal_handle",
                self.session_id
            ));
            false
        }
    }

    pub fn stop_current_turn_signal(&self) -> InterruptSignal {
        self.stop_current_turn_signal.clone()
    }
}

pub(super) async fn register_session_interrupt_queue(
    queues: &SessionInterruptQueues,
    session_id: &str,
    queue: SoftInterruptQueue,
) {
    let mut guard = queues.write().await;
    guard.insert(session_id.to_string(), queue);
}

pub(super) async fn rename_session_interrupt_queue(
    queues: &SessionInterruptQueues,
    old_session_id: &str,
    new_session_id: &str,
) {
    let mut guard = queues.write().await;
    if let Some(queue) = guard.remove(old_session_id) {
        guard.insert(new_session_id.to_string(), queue);
    }
    if let Ok(mut replays) = SOFT_INTERRUPT_REPLAYS.lock() {
        replays.rename(old_session_id, new_session_id);
    }
    if let Ok(mut editors) = QUEUED_MESSAGE_EDITORS.lock() {
        editors.rename(old_session_id, new_session_id);
    }
}

pub(super) async fn remove_session_interrupt_queue(
    queues: &SessionInterruptQueues,
    session_id: &str,
) {
    let mut guard = queues.write().await;
    guard.remove(session_id);
}

pub(super) async fn queue_soft_interrupt_for_session(
    session_id: &str,
    content: String,
    urgent: bool,
    source: SoftInterruptSource,
    queues: &SessionInterruptQueues,
    sessions: &super::SessionAgents,
) -> bool {
    let coordinator = queued_message_editor_for_session(session_id);
    if let Some(queue) = queues.read().await.get(session_id).cloned() {
        return enqueue_soft_interrupt(&queue, &coordinator, content, Vec::new(), urgent, source);
    }

    let queue = {
        let guard = sessions.read().await;
        guard.get(session_id).and_then(|agent| {
            agent
                .try_lock()
                .ok()
                .map(|agent_guard| agent_guard.soft_interrupt_queue())
        })
    };

    if let Some(queue) = queue {
        register_session_interrupt_queue(queues, session_id, queue.clone()).await;
        enqueue_soft_interrupt(&queue, &coordinator, content, Vec::new(), urgent, source)
    } else {
        let session_exists = {
            let guard = sessions.read().await;
            guard.contains_key(session_id)
        } || crate::session::session_exists(session_id);

        if !session_exists {
            return false;
        }

        let persist = || -> anyhow::Result<()> {
            let mut coordinator = coordinator
                .lock()
                .map_err(|_| anyhow::anyhow!("queued message editor authority is unavailable"))?;
            let mut envelope = crate::soft_interrupt_store::load_envelope(session_id)?;
            let persisted_max = envelope
                .dispatchable
                .iter()
                .filter_map(|message| message.enqueue_sequence)
                .chain(
                    envelope
                        .reservations
                        .iter()
                        .map(|reservation| reservation.snapshot_queue_sequence),
                )
                .max()
                .unwrap_or(0);
            coordinator.next_enqueue_sequence =
                coordinator.next_enqueue_sequence.max(persisted_max);
            coordinator.next_enqueue_sequence = coordinator.next_enqueue_sequence.saturating_add(1);
            envelope.dispatchable.push(
                SoftInterruptMessage {
                    content,
                    images: Vec::new(),
                    urgent,
                    source,
                    message_id: Some(crate::id::new_id("soft_interrupt")),
                    owner_client_instance_id: None,
                    enqueue_sequence: Some(coordinator.next_enqueue_sequence),
                }
                .into(),
            );
            crate::soft_interrupt_store::overwrite_envelope(session_id, &envelope)
        };
        persist().map(|_| true).unwrap_or_else(|err| {
            crate::logging::warn(&format!(
                "Failed to persist deferred soft interrupt for session {}: {}",
                session_id, err
            ));
            false
        })
    }
}

#[cfg(test)]
#[path = "state_tests/soft_interrupt_recall.rs"]
mod soft_interrupt_recall_tests;

#[cfg(test)]
#[path = "state_tests/queued_message_editor.rs"]
mod queued_message_editor_fixtures;
