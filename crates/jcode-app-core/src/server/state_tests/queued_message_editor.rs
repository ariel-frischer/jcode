use super::*;

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
