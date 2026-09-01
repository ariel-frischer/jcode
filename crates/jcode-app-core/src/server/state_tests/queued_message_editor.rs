use super::*;
use crate::protocol::{
    QueuedMessageEditorDirection, QueuedMessageEditorOperation, QueuedMessageEditorOutcome,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub(super) const OWNER_CLIENT_ID: &str = "queued-editor-owner";
pub(super) const OTHER_CLIENT_ID: &str = "queued-editor-other-client";
pub(super) const NAVIGATION_SESSION_ID: &str = "queued-editor-navigation-session";
pub(super) const START_OPERATION_ID: &str = "queued-editor-operation-start";
pub(super) const MOVE_OLDER_OPERATION_ID: &str = "queued-editor-operation-move-older";
pub(super) const MOVE_NEWER_OPERATION_ID: &str = "queued-editor-operation-move-newer";
pub(super) const FINISH_OPERATION_ID: &str = "queued-editor-operation-finish";
pub(super) const RELEASE_OPERATION_ID: &str = "queued-editor-operation-release";

#[derive(Debug, Clone)]
pub(super) struct QueuedMessageFixture {
    pub(super) message: SoftInterruptMessage,
    pub(super) injected: bool,
}

pub(super) fn ordered_images(label: &str) -> Vec<(String, String)> {
    vec![
        ("image/png".to_string(), format!("{label}-png-bytes-base64")),
        (
            "image/jpeg".to_string(),
            format!("{label}-jpeg-bytes-base64"),
        ),
        (
            "image/webp".to_string(),
            format!("{label}-webp-bytes-base64"),
        ),
    ]
}

fn fixture(
    label: &str,
    source: SoftInterruptSource,
    owner_client_instance_id: Option<&str>,
    enqueue_sequence: Option<u64>,
    injected: bool,
) -> QueuedMessageFixture {
    QueuedMessageFixture {
        message: SoftInterruptMessage {
            content: format!("queued editor fixture {label}"),
            images: ordered_images(label),
            urgent: false,
            source,
            message_id: Some(format!("queued-editor-message-{label}")),
            owner_client_instance_id: owner_client_instance_id.map(str::to_string),
            enqueue_sequence,
        },
        injected,
    }
}

pub(super) fn owned_user(label: &str, enqueue_sequence: u64) -> QueuedMessageFixture {
    fixture(
        label,
        SoftInterruptSource::User,
        Some(OWNER_CLIENT_ID),
        Some(enqueue_sequence),
        false,
    )
}

pub(super) fn other_client_user(label: &str, enqueue_sequence: u64) -> QueuedMessageFixture {
    fixture(
        label,
        SoftInterruptSource::User,
        Some(OTHER_CLIENT_ID),
        Some(enqueue_sequence),
        false,
    )
}

pub(super) fn unowned_legacy_user(label: &str) -> QueuedMessageFixture {
    let mut fixture = fixture(label, SoftInterruptSource::User, None, None, false);
    fixture.message.message_id = None;
    fixture
}

pub(super) fn system_message(label: &str, enqueue_sequence: u64) -> QueuedMessageFixture {
    fixture(
        label,
        SoftInterruptSource::System,
        Some(OWNER_CLIENT_ID),
        Some(enqueue_sequence),
        false,
    )
}

pub(super) fn background_message(label: &str, enqueue_sequence: u64) -> QueuedMessageFixture {
    fixture(
        label,
        SoftInterruptSource::BackgroundTask,
        Some(OWNER_CLIENT_ID),
        Some(enqueue_sequence),
        false,
    )
}

pub(super) fn injected_user(label: &str, enqueue_sequence: u64) -> QueuedMessageFixture {
    fixture(
        label,
        SoftInterruptSource::User,
        Some(OWNER_CLIENT_ID),
        Some(enqueue_sequence),
        true,
    )
}

pub(super) fn post_snapshot_arrival(label: &str, snapshot_sequence: u64) -> QueuedMessageFixture {
    owned_user(label, snapshot_sequence + 1)
}

#[test]
fn mixed_queue_fixtures_are_stable_distinct_and_image_ordered() {
    let owned = owned_user("owned", 10);
    let other = other_client_user("other", 11);
    let legacy = unowned_legacy_user("legacy");
    let system = system_message("system", 12);
    let background = background_message("background", 13);
    let injected = injected_user("injected", 14);
    let arrival = post_snapshot_arrival("arrival", 14);

    assert_eq!(
        owned.message.message_id.as_deref(),
        Some("queued-editor-message-owned")
    );
    assert_eq!(
        owned.message.owner_client_instance_id.as_deref(),
        Some(OWNER_CLIENT_ID)
    );
    assert_eq!(owned.message.enqueue_sequence, Some(10));
    assert_eq!(owned.message.images, ordered_images("owned"));
    assert_ne!(owned.message.images, other.message.images);

    assert_eq!(
        other.message.owner_client_instance_id.as_deref(),
        Some(OTHER_CLIENT_ID)
    );
    assert_eq!(legacy.message.message_id, None);
    assert_eq!(legacy.message.owner_client_instance_id, None);
    assert_eq!(legacy.message.enqueue_sequence, None);
    assert!(matches!(system.message.source, SoftInterruptSource::System));
    assert!(matches!(
        background.message.source,
        SoftInterruptSource::BackgroundTask
    ));
    assert!(injected.injected);
    assert_eq!(arrival.message.enqueue_sequence, Some(15));

    assert_eq!(NAVIGATION_SESSION_ID, "queued-editor-navigation-session");
    assert_eq!(START_OPERATION_ID, "queued-editor-operation-start");
    assert_eq!(
        MOVE_OLDER_OPERATION_ID,
        "queued-editor-operation-move-older"
    );
    assert_eq!(
        MOVE_NEWER_OPERATION_ID,
        "queued-editor-operation-move-newer"
    );
    assert_eq!(FINISH_OPERATION_ID, "queued-editor-operation-finish");
    assert_eq!(RELEASE_OPERATION_ID, "queued-editor-operation-release");
}

fn control_with(fixtures: Vec<QueuedMessageFixture>) -> (SessionControlHandle, SoftInterruptQueue) {
    static NEXT_TEST_SESSION: AtomicU64 = AtomicU64::new(1);
    let queue = Arc::new(Mutex::new(
        fixtures
            .into_iter()
            .filter(|fixture| !fixture.injected)
            .map(|fixture| fixture.message)
            .collect(),
    ));
    let control = SessionControlHandle::new(
        format!(
            "queued-editor-authority-test-{}",
            NEXT_TEST_SESSION.fetch_add(1, Ordering::Relaxed)
        ),
        Arc::clone(&queue),
        InterruptSignal::new(),
        InterruptSignal::new(),
    );
    (control, queue)
}

#[test]
fn start_reserves_only_verified_owned_user_records_and_selects_newest() {
    let excluded_other = other_client_user("other", 2);
    let excluded_legacy = unowned_legacy_user("legacy");
    let excluded_system = system_message("system", 4);
    let excluded_background = background_message("background", 5);
    let (control, queue) = control_with(vec![
        owned_user("oldest", 1),
        excluded_other.clone(),
        excluded_legacy.clone(),
        excluded_system.clone(),
        excluded_background.clone(),
        owned_user("newest", 6),
        injected_user("already-injected", 7),
    ]);

    let result = control
        .queued_message_editor(
            OWNER_CLIENT_ID,
            NAVIGATION_SESSION_ID,
            START_OPERATION_ID,
            QueuedMessageEditorOperation::Start,
        )
        .expect("start must succeed");

    assert_eq!(result.outcome, QueuedMessageEditorOutcome::Started);
    let selection = result.selection.expect("newest selection");
    assert_eq!(selection.message_id, "queued-editor-message-newest");
    assert!(selection.older_available);
    assert!(!selection.newer_available);
    assert_eq!(selection.images, ordered_images("newest"));

    let remaining = queue.lock().expect("queue lock");
    let remaining_ids: Vec<_> = remaining
        .iter()
        .map(|message| message.message_id.as_deref())
        .collect();
    assert_eq!(
        remaining_ids,
        vec![
            excluded_other.message.message_id.as_deref(),
            excluded_legacy.message.message_id.as_deref(),
            excluded_system.message.message_id.as_deref(),
            excluded_background.message.message_id.as_deref(),
        ]
    );
}

#[test]
fn matching_replay_returns_equivalent_outcome_without_second_reservation() {
    let (control, queue) = control_with(vec![owned_user("only", 1)]);
    let first = control
        .queued_message_editor(
            OWNER_CLIENT_ID,
            NAVIGATION_SESSION_ID,
            START_OPERATION_ID,
            QueuedMessageEditorOperation::Start,
        )
        .expect("first start");
    let replay = control
        .queued_message_editor(
            OWNER_CLIENT_ID,
            NAVIGATION_SESSION_ID,
            START_OPERATION_ID,
            QueuedMessageEditorOperation::Start,
        )
        .expect("replayed start");

    assert_eq!(first.selection, replay.selection);
    assert_eq!(replay.outcome, QueuedMessageEditorOutcome::Replay);
    assert!(queue.lock().expect("queue lock").is_empty());
}

#[test]
fn cross_client_and_conflicting_operation_reuse_fail_without_disclosure_or_mutation() {
    let (control, queue) = control_with(vec![owned_user("oldest", 1), owned_user("newest", 2)]);
    let started = control
        .queued_message_editor(
            OWNER_CLIENT_ID,
            NAVIGATION_SESSION_ID,
            START_OPERATION_ID,
            QueuedMessageEditorOperation::Start,
        )
        .expect("start");
    let selected = started.selection.expect("selection");

    let cross_client = control.queued_message_editor(
        OTHER_CLIENT_ID,
        NAVIGATION_SESSION_ID,
        MOVE_OLDER_OPERATION_ID,
        QueuedMessageEditorOperation::Move {
            direction: QueuedMessageEditorDirection::Older,
            selected_message_id: selected.message_id.clone(),
            draft: RecallableSoftInterrupt {
                content: "must not be disclosed or saved".to_string(),
                images: ordered_images("unauthorized"),
            },
        },
    );
    assert!(cross_client.is_err());
    assert!(queue.lock().expect("queue lock").is_empty());

    let conflicting_reuse = control.queued_message_editor(
        OWNER_CLIENT_ID,
        NAVIGATION_SESSION_ID,
        START_OPERATION_ID,
        QueuedMessageEditorOperation::Release,
    );
    assert!(conflicting_reuse.is_err());

    let moved = control
        .queued_message_editor(
            OWNER_CLIENT_ID,
            NAVIGATION_SESSION_ID,
            MOVE_OLDER_OPERATION_ID,
            QueuedMessageEditorOperation::Move {
                direction: QueuedMessageEditorDirection::Older,
                selected_message_id: selected.message_id,
                draft: RecallableSoftInterrupt {
                    content: "saved newest draft".to_string(),
                    images: ordered_images("saved-newest"),
                },
            },
        )
        .expect("owner move");
    assert_eq!(moved.outcome, QueuedMessageEditorOutcome::Moved);
    assert_eq!(
        moved.selection.expect("older selection").message_id,
        "queued-editor-message-oldest"
    );
}

#[test]
fn finish_commits_only_selected_and_restores_held_order_with_stale_anchor_feedback() {
    let predecessor = other_client_user("predecessor", 1);
    let successor = other_client_user("successor", 4);
    let (control, queue) = control_with(vec![
        predecessor.clone(),
        owned_user("older", 2),
        owned_user("selected", 3),
        successor.clone(),
    ]);
    let started = control
        .queued_message_editor(
            OWNER_CLIENT_ID,
            "finish-navigation",
            "finish-start",
            QueuedMessageEditorOperation::Start,
        )
        .expect("start");
    let selected = started.selection.expect("selection");

    {
        let mut pending = queue.lock().expect("queue lock");
        pending.remove(0);
        pending.push(post_snapshot_arrival("arrival", 4).message);
    }
    let result = control
        .queued_message_editor(
            OWNER_CLIENT_ID,
            "finish-navigation",
            FINISH_OPERATION_ID,
            QueuedMessageEditorOperation::Finish {
                selected_message_id: selected.message_id,
                draft: RecallableSoftInterrupt {
                    content: "committed edit".to_string(),
                    images: ordered_images("committed"),
                },
            },
        )
        .expect("finish");
    assert_eq!(result.outcome, QueuedMessageEditorOutcome::StalePlacement);

    let pending = queue.lock().expect("queue lock");
    let ids: Vec<_> = pending
        .iter()
        .map(|message| message.message_id.as_deref().unwrap_or("legacy"))
        .collect();
    assert_eq!(
        ids,
        vec![
            "queued-editor-message-older",
            "queued-editor-message-selected",
            "queued-editor-message-successor",
            "queued-editor-message-arrival",
        ]
    );
    assert_eq!(pending[1].content, "committed edit");
    assert_eq!(pending[1].images, ordered_images("committed"));
    assert_eq!(pending[0].content, "queued editor fixture older");
}

#[test]
fn finish_restores_between_two_surviving_anchors_exactly() {
    let (control, queue) = control_with(vec![
        other_client_user("predecessor", 1),
        owned_user("older", 2),
        owned_user("selected", 3),
        other_client_user("successor", 4),
    ]);
    let selected = control
        .queued_message_editor(
            OWNER_CLIENT_ID,
            "two-anchor-navigation",
            "two-anchor-start",
            QueuedMessageEditorOperation::Start,
        )
        .expect("start")
        .selection
        .expect("selection");

    let result = control
        .queued_message_editor(
            OWNER_CLIENT_ID,
            "two-anchor-navigation",
            "two-anchor-finish",
            QueuedMessageEditorOperation::Finish {
                selected_message_id: selected.message_id,
                draft: RecallableSoftInterrupt {
                    content: "exact committed edit".to_string(),
                    images: ordered_images("exact-committed"),
                },
            },
        )
        .expect("finish");

    assert_eq!(result.outcome, QueuedMessageEditorOutcome::Committed);
    assert_eq!(result.placement, QueuedMessageEditorPlacement::Exact);
    let pending = queue.lock().expect("queue lock");
    let ids: Vec<_> = pending
        .iter()
        .map(|message| message.message_id.as_deref().expect("stable id"))
        .collect();
    assert_eq!(
        ids,
        [
            "queued-editor-message-predecessor",
            "queued-editor-message-older",
            "queued-editor-message-selected",
            "queued-editor-message-successor",
        ]
    );
    assert_eq!(pending[2].content, "exact committed edit");
    assert_eq!(pending[2].images, ordered_images("exact-committed"));
}

#[test]
fn finish_with_only_predecessor_anchor_restores_after_it_and_reports_stale() {
    let (control, queue) = control_with(vec![
        other_client_user("predecessor", 1),
        owned_user("older", 2),
        owned_user("selected", 3),
        other_client_user("successor", 4),
    ]);
    let selected = control
        .queued_message_editor(
            OWNER_CLIENT_ID,
            "predecessor-anchor-navigation",
            "predecessor-anchor-start",
            QueuedMessageEditorOperation::Start,
        )
        .expect("start")
        .selection
        .expect("selection");
    queue.lock().expect("queue lock").remove(1);

    let result = control
        .queued_message_editor(
            OWNER_CLIENT_ID,
            "predecessor-anchor-navigation",
            "predecessor-anchor-finish",
            QueuedMessageEditorOperation::Finish {
                selected_message_id: selected.message_id,
                draft: RecallableSoftInterrupt {
                    content: "predecessor-side edit".to_string(),
                    images: ordered_images("predecessor-side"),
                },
            },
        )
        .expect("finish");

    assert_eq!(result.outcome, QueuedMessageEditorOutcome::StalePlacement);
    assert_eq!(
        result.placement,
        QueuedMessageEditorPlacement::StaleBestEffort
    );
    let pending = queue.lock().expect("queue lock");
    let ids: Vec<_> = pending
        .iter()
        .map(|message| message.message_id.as_deref().expect("stable id"))
        .collect();
    assert_eq!(
        ids,
        [
            "queued-editor-message-predecessor",
            "queued-editor-message-older",
            "queued-editor-message-selected",
        ]
    );
}

#[test]
fn finish_without_anchors_restores_between_survivors_and_arrivals() {
    let (control, queue) = control_with(vec![
        other_client_user("predecessor", 1),
        owned_user("older", 2),
        other_client_user("pre-snapshot-survivor", 3),
        owned_user("selected", 4),
        other_client_user("successor", 5),
    ]);
    let selected = control
        .queued_message_editor(
            OWNER_CLIENT_ID,
            "no-anchor-navigation",
            "no-anchor-start",
            QueuedMessageEditorOperation::Start,
        )
        .expect("start")
        .selection
        .expect("selection");
    {
        let mut pending = queue.lock().expect("queue lock");
        pending.retain(|message| {
            !matches!(
                message.message_id.as_deref(),
                Some("queued-editor-message-predecessor" | "queued-editor-message-successor")
            )
        });
        pending.push(post_snapshot_arrival("arrival-one", 5).message);
        pending.push(post_snapshot_arrival("arrival-two", 6).message);
    }

    let result = control
        .queued_message_editor(
            OWNER_CLIENT_ID,
            "no-anchor-navigation",
            "no-anchor-finish",
            QueuedMessageEditorOperation::Finish {
                selected_message_id: selected.message_id,
                draft: RecallableSoftInterrupt {
                    content: "no-anchor edit".to_string(),
                    images: ordered_images("no-anchor"),
                },
            },
        )
        .expect("finish");

    assert_eq!(result.outcome, QueuedMessageEditorOutcome::StalePlacement);
    let pending = queue.lock().expect("queue lock");
    let ids: Vec<_> = pending
        .iter()
        .map(|message| message.message_id.as_deref().expect("stable id"))
        .collect();
    assert_eq!(
        ids,
        [
            "queued-editor-message-pre-snapshot-survivor",
            "queued-editor-message-older",
            "queued-editor-message-selected",
            "queued-editor-message-arrival-one",
            "queued-editor-message-arrival-two",
        ]
    );
}

#[test]
fn unsafe_finish_selection_returns_recoverable_conflict_without_queue_mutation() {
    let (control, queue) = control_with(vec![owned_user("older", 1), owned_user("selected", 2)]);
    let selected = control
        .queued_message_editor(
            OWNER_CLIENT_ID,
            "conflict-navigation",
            "conflict-start",
            QueuedMessageEditorOperation::Start,
        )
        .expect("start")
        .selection
        .expect("selection");
    assert!(queue.lock().expect("queue lock").is_empty());

    let conflict = control
        .queued_message_editor(
            OWNER_CLIENT_ID,
            "conflict-navigation",
            "conflict-finish",
            QueuedMessageEditorOperation::Finish {
                selected_message_id: "queued-editor-message-not-selected".to_string(),
                draft: RecallableSoftInterrupt {
                    content: "must remain recoverable in the composer".to_string(),
                    images: ordered_images("conflict-draft"),
                },
            },
        )
        .expect("conflict result");

    assert_eq!(conflict.outcome, QueuedMessageEditorOutcome::Conflict);
    assert_eq!(conflict.placement, QueuedMessageEditorPlacement::NotApplied);
    assert_eq!(
        conflict
            .selection
            .expect("recoverable selection")
            .message_id,
        selected.message_id
    );
    assert!(queue.lock().expect("queue lock").is_empty());
}

#[test]
fn empty_finish_deletes_only_selected_and_images_only_finish_commits() {
    let (control, queue) = control_with(vec![owned_user("older", 1), owned_user("selected", 2)]);
    let selected = control
        .queued_message_editor(
            OWNER_CLIENT_ID,
            "delete-navigation",
            "delete-start",
            QueuedMessageEditorOperation::Start,
        )
        .expect("start")
        .selection
        .expect("selection");
    let deleted = control
        .queued_message_editor(
            OWNER_CLIENT_ID,
            "delete-navigation",
            "delete-finish",
            QueuedMessageEditorOperation::Finish {
                selected_message_id: selected.message_id,
                draft: RecallableSoftInterrupt {
                    content: String::new(),
                    images: Vec::new(),
                },
            },
        )
        .expect("delete");
    assert_eq!(deleted.outcome, QueuedMessageEditorOutcome::Deleted);
    let pending = queue.lock().expect("queue lock");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].content, "queued editor fixture older");
    drop(pending);

    let (control, queue) = control_with(vec![owned_user("images-only", 1)]);
    let selected = control
        .queued_message_editor(
            OWNER_CLIENT_ID,
            "images-only-navigation",
            "images-only-start",
            QueuedMessageEditorOperation::Start,
        )
        .expect("images-only start")
        .selection
        .expect("images-only selection");
    let images = ordered_images("images-only-edit");
    let committed = control
        .queued_message_editor(
            OWNER_CLIENT_ID,
            "images-only-navigation",
            "images-only-finish",
            QueuedMessageEditorOperation::Finish {
                selected_message_id: selected.message_id,
                draft: RecallableSoftInterrupt {
                    content: String::new(),
                    images: images.clone(),
                },
            },
        )
        .expect("images-only commit");
    assert_eq!(committed.outcome, QueuedMessageEditorOutcome::Committed);
    let pending = queue.lock().expect("queue lock");
    assert_eq!(pending.len(), 1);
    assert!(pending[0].content.is_empty());
    assert_eq!(pending[0].images, images);
}

#[tokio::test]
async fn disconnect_grace_can_resume_or_release_immutable_originals_exactly_once() {
    let (control, queue) = control_with(vec![owned_user("held", 1)]);
    let selected = control
        .queued_message_editor(
            OWNER_CLIENT_ID,
            "disconnect-navigation",
            "disconnect-start",
            QueuedMessageEditorOperation::Start,
        )
        .expect("start")
        .selection
        .expect("selection");
    control
        .queued_message_editor(
            OWNER_CLIENT_ID,
            "disconnect-navigation",
            "disconnect-edit",
            QueuedMessageEditorOperation::Move {
                direction: QueuedMessageEditorDirection::Older,
                selected_message_id: selected.message_id,
                draft: RecallableSoftInterrupt {
                    content: "uncommitted edit".to_string(),
                    images: ordered_images("uncommitted"),
                },
            },
        )
        .expect("boundary save");

    control.begin_queued_message_editor_disconnect_grace(
        OWNER_CLIENT_ID.to_string(),
        std::time::Duration::from_millis(20),
    );
    control.resume_queued_message_editor_owner(OWNER_CLIENT_ID);
    tokio::time::sleep(std::time::Duration::from_millis(40)).await;
    assert!(queue.lock().expect("queue lock").is_empty());

    control.begin_queued_message_editor_disconnect_grace(
        OWNER_CLIENT_ID.to_string(),
        std::time::Duration::from_millis(20),
    );
    tokio::time::sleep(std::time::Duration::from_millis(40)).await;
    let pending = queue.lock().expect("queue lock");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].content, "queued editor fixture held");
    assert_eq!(pending[0].images, ordered_images("held"));
}
