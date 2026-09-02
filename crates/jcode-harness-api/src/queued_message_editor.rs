//! Public queued-message editor data transfer types.
//!
//! These shapes intentionally mirror the canonical `jcode-protocol` wire
//! contract. Queue ownership, eligibility, anchoring, and mutation remain
//! daemon concerns rather than being reimplemented in this API crate.

use serde::{Deserialize, Serialize};

/// Stable capability advertised by peers that support the complete editor contract.
pub const QUEUED_MESSAGE_NAVIGATION_CAPABILITY: &str = "queued_message_navigation_v1";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QueuedMessageEditorDirection {
    Older,
    Newer,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QueuedMessageEditorDraft {
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QueuedMessageEditorOperation {
    Start,
    Move {
        direction: QueuedMessageEditorDirection,
        selected_message_id: String,
        draft: QueuedMessageEditorDraft,
    },
    Finish {
        selected_message_id: String,
        draft: QueuedMessageEditorDraft,
    },
    Release,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QueuedMessageEditorOutcome {
    Started,
    Moved,
    Boundary,
    Committed,
    Deleted,
    Released,
    StalePlacement,
    Conflict,
    Replay,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QueuedMessageEditorPlacement {
    Exact,
    StaleBestEffort,
    NotApplied,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QueuedMessageEditorSelection {
    pub message_id: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<(String, String)>,
    pub older_available: bool,
    pub newer_available: bool,
}
