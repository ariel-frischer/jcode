use anyhow::{Context, Result, bail};
use jcode_agent_runtime::{SoftInterruptMessage, SoftInterruptSource};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;

const STORE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSoftInterrupt {
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<(String, String)>,
    pub urgent: bool,
    pub source: PersistedSoftInterruptSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_client_instance_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enqueue_sequence: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistedSoftInterruptSource {
    User,
    System,
    BackgroundTask,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedSoftInterruptDraft {
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<(String, String)>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PersistedReservationState {
    Active,
    Finishing,
    Released,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedHeldSoftInterrupt {
    pub message_id: String,
    pub original: PersistedSoftInterrupt,
    pub draft: PersistedSoftInterruptDraft,
    pub original_relative_index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedCompletedEditorOperation {
    pub operation_id: String,
    pub request_fingerprint: String,
    pub outcome: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSoftInterruptReservation {
    pub navigation_session_id: String,
    pub owner_client_instance_id: String,
    pub snapshot_queue_sequence: u64,
    pub state: PersistedReservationState,
    pub selected_index: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predecessor_message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub successor_message_id: Option<String>,
    pub held: Vec<PersistedHeldSoftInterrupt>,
    #[serde(default)]
    pub completed_operations: Vec<PersistedCompletedEditorOperation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoftInterruptStoreEnvelope {
    pub version: u32,
    pub dispatchable: Vec<PersistedSoftInterrupt>,
    #[serde(default)]
    pub reservations: Vec<PersistedSoftInterruptReservation>,
}

impl SoftInterruptStoreEnvelope {
    pub fn empty() -> Self {
        Self {
            version: STORE_VERSION,
            dispatchable: Vec::new(),
            reservations: Vec::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PersistedStoreFile {
    Legacy(Vec<PersistedSoftInterrupt>),
    Envelope(SoftInterruptStoreEnvelope),
}

impl From<SoftInterruptSource> for PersistedSoftInterruptSource {
    fn from(value: SoftInterruptSource) -> Self {
        match value {
            SoftInterruptSource::User => Self::User,
            SoftInterruptSource::System => Self::System,
            SoftInterruptSource::BackgroundTask => Self::BackgroundTask,
        }
    }
}

impl From<PersistedSoftInterruptSource> for SoftInterruptSource {
    fn from(value: PersistedSoftInterruptSource) -> Self {
        match value {
            PersistedSoftInterruptSource::User => Self::User,
            PersistedSoftInterruptSource::System => Self::System,
            PersistedSoftInterruptSource::BackgroundTask => Self::BackgroundTask,
        }
    }
}

impl From<SoftInterruptMessage> for PersistedSoftInterrupt {
    fn from(value: SoftInterruptMessage) -> Self {
        Self {
            content: value.content,
            images: value.images,
            urgent: value.urgent,
            source: value.source.into(),
            message_id: value.message_id,
            owner_client_instance_id: value.owner_client_instance_id,
            enqueue_sequence: value.enqueue_sequence,
        }
    }
}

impl From<PersistedSoftInterrupt> for SoftInterruptMessage {
    fn from(value: PersistedSoftInterrupt) -> Self {
        Self {
            content: value.content,
            images: value.images,
            urgent: value.urgent,
            source: value.source.into(),
            message_id: value.message_id,
            owner_client_instance_id: value.owner_client_instance_id,
            enqueue_sequence: value.enqueue_sequence,
        }
    }
}

fn dir_path() -> Result<PathBuf> {
    Ok(crate::storage::jcode_dir()?.join("pending-soft-interrupts"))
}

fn path_for_session(session_id: &str) -> Result<PathBuf> {
    Ok(dir_path()?.join(format!("{}.json", session_id)))
}

fn validate_envelope(envelope: &SoftInterruptStoreEnvelope) -> Result<()> {
    if envelope.version != STORE_VERSION {
        bail!(
            "unsupported pending soft interrupt store version {} (expected {})",
            envelope.version,
            STORE_VERSION
        );
    }

    let mut navigation_ids = HashSet::new();
    let mut held_message_ids = HashSet::new();
    for reservation in &envelope.reservations {
        if reservation.navigation_session_id.trim().is_empty()
            || reservation.owner_client_instance_id.trim().is_empty()
        {
            bail!("soft interrupt reservation has an empty owner or navigation identity");
        }
        if !navigation_ids.insert(reservation.navigation_session_id.as_str()) {
            bail!("duplicate soft interrupt navigation session identity");
        }
        if reservation.held.is_empty() || reservation.selected_index >= reservation.held.len() {
            bail!("soft interrupt reservation has an invalid held selection");
        }
        for (expected_index, held) in reservation.held.iter().enumerate() {
            if held.message_id.trim().is_empty()
                || held.original.message_id.as_deref() != Some(held.message_id.as_str())
                || held.original_relative_index != expected_index
                || !held_message_ids.insert(held.message_id.as_str())
            {
                bail!("soft interrupt reservation contains unsafe held-message state");
            }
        }
    }
    Ok(())
}

fn restoration_index(
    dispatchable: &[PersistedSoftInterrupt],
    reservation: &PersistedSoftInterruptReservation,
) -> usize {
    let predecessor = reservation
        .predecessor_message_id
        .as_deref()
        .and_then(|id| {
            dispatchable
                .iter()
                .position(|message| message.message_id.as_deref() == Some(id))
        });
    let successor = reservation.successor_message_id.as_deref().and_then(|id| {
        dispatchable
            .iter()
            .position(|message| message.message_id.as_deref() == Some(id))
    });

    match (predecessor, successor) {
        (Some(before), Some(after)) if before < after => before + 1,
        (Some(before), _) => before + 1,
        (_, Some(after)) => after,
        (None, None) => dispatchable
            .iter()
            .position(|message| {
                message
                    .enqueue_sequence
                    .is_some_and(|sequence| sequence > reservation.snapshot_queue_sequence)
            })
            .unwrap_or(dispatchable.len()),
    }
}

fn recover_abandoned_reservations(envelope: &mut SoftInterruptStoreEnvelope) {
    let reservations = std::mem::take(&mut envelope.reservations);
    for mut reservation in reservations {
        reservation
            .held
            .sort_by_key(|held| held.original_relative_index);
        let insert_at = restoration_index(&envelope.dispatchable, &reservation);
        envelope.dispatchable.splice(
            insert_at..insert_at,
            reservation.held.into_iter().map(|held| held.original),
        );
    }
}

fn read_envelope(session_id: &str) -> Result<Option<SoftInterruptStoreEnvelope>> {
    let path = path_for_session(session_id)?;
    if !path.exists() {
        return Ok(None);
    }

    let persisted: PersistedStoreFile = crate::storage::read_json(&path)
        .with_context(|| format!("read pending soft interrupt store {}", path.display()))?;
    let envelope = match persisted {
        PersistedStoreFile::Legacy(dispatchable) => SoftInterruptStoreEnvelope {
            version: STORE_VERSION,
            dispatchable,
            reservations: Vec::new(),
        },
        PersistedStoreFile::Envelope(envelope) => envelope,
    };
    validate_envelope(&envelope)?;
    Ok(Some(envelope))
}

pub fn load_envelope(session_id: &str) -> Result<SoftInterruptStoreEnvelope> {
    Ok(read_envelope(session_id)?.unwrap_or_else(SoftInterruptStoreEnvelope::empty))
}

pub fn overwrite_envelope(session_id: &str, envelope: &SoftInterruptStoreEnvelope) -> Result<()> {
    validate_envelope(envelope)?;
    let path = path_for_session(session_id)?;
    if envelope.dispatchable.is_empty() && envelope.reservations.is_empty() {
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::storage::write_json_fast(&path, envelope)
}

pub fn load(session_id: &str) -> Result<Vec<SoftInterruptMessage>> {
    let Some(mut envelope) = read_envelope(session_id)? else {
        return Ok(Vec::new());
    };
    if !envelope.reservations.is_empty() {
        recover_abandoned_reservations(&mut envelope);
        overwrite_envelope(session_id, &envelope)?;
    }
    Ok(envelope
        .dispatchable
        .into_iter()
        .map(SoftInterruptMessage::from)
        .collect())
}

pub fn take(session_id: &str) -> Result<Vec<SoftInterruptMessage>> {
    let path = path_for_session(session_id)?;
    let loaded = load(session_id)?;
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
    Ok(loaded)
}

pub fn overwrite(session_id: &str, interrupts: &[SoftInterruptMessage]) -> Result<()> {
    let envelope = SoftInterruptStoreEnvelope {
        version: STORE_VERSION,
        dispatchable: interrupts.iter().cloned().map(Into::into).collect(),
        reservations: Vec::new(),
    };
    overwrite_envelope(session_id, &envelope)
}

pub fn append(session_id: &str, interrupt: SoftInterruptMessage) -> Result<()> {
    let mut current = load(session_id)?;
    current.push(interrupt);
    overwrite(session_id, &current)
}

pub fn clear(session_id: &str) -> Result<()> {
    overwrite(session_id, &[])
}

#[cfg(test)]
#[path = "soft_interrupt_store_tests.rs"]
mod soft_interrupt_store_tests;
