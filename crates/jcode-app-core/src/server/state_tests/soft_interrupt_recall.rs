use super::*;
use std::sync::{Arc, Mutex};

fn message(
    content: &str,
    source: SoftInterruptSource,
    owner_client_instance_id: Option<&str>,
) -> SoftInterruptMessage {
    SoftInterruptMessage {
        content: content.to_string(),
        images: vec![("image/png".to_string(), format!("{content}-image"))],
        urgent: false,
        source,
        message_id: Some(format!("message-{content}")),
        owner_client_instance_id: owner_client_instance_id.map(str::to_string),
    }
}

fn control(queue: SoftInterruptQueue) -> SessionControlHandle {
    SessionControlHandle::new(
        "session-recall-tests",
        queue,
        InterruptSignal::new(),
        InterruptSignal::new(),
    )
}

fn queued_contents(queue: &SoftInterruptQueue) -> Vec<String> {
    queue
        .lock()
        .expect("soft interrupt queue lock")
        .iter()
        .map(|message| message.content.clone())
        .collect()
}

#[test]
fn recall_selects_newest_matching_owned_user_one_at_a_time() {
    let queue = Arc::new(Mutex::new(vec![
        message("owned-old", SoftInterruptSource::User, Some("client-a")),
        message("other-client", SoftInterruptSource::User, Some("client-b")),
        message("system", SoftInterruptSource::System, Some("client-a")),
        message("unowned", SoftInterruptSource::User, None),
        message(
            "background",
            SoftInterruptSource::BackgroundTask,
            Some("client-a"),
        ),
        message("owned-new", SoftInterruptSource::User, Some("client-a")),
    ]));
    let control = control(queue.clone());

    let newest = control
        .recall_soft_interrupt("client-a", "operation-newest")
        .expect("newest owned user message should be recalled");
    assert_eq!(newest.content, "owned-new");
    assert_eq!(
        newest.images,
        vec![("image/png".to_string(), "owned-new-image".to_string())]
    );
    assert_eq!(
        queued_contents(&queue),
        vec![
            "owned-old",
            "other-client",
            "system",
            "unowned",
            "background",
        ]
    );

    let next = control
        .recall_soft_interrupt("client-a", "operation-next")
        .expect("next owned user message should be recalled separately");
    assert_eq!(next.content, "owned-old");
    assert_eq!(
        queued_contents(&queue),
        vec!["other-client", "system", "unowned", "background"]
    );
}

#[test]
fn recall_replay_returns_same_result_without_a_second_removal() {
    let queue = Arc::new(Mutex::new(vec![
        message("owned-old", SoftInterruptSource::User, Some("client-a")),
        message("owned-new", SoftInterruptSource::User, Some("client-a")),
    ]));
    let first_control = control(queue.clone());

    let first = first_control
        .recall_soft_interrupt("client-a", "stable-operation")
        .expect("first recall should return newest message");
    assert_eq!(first.content, "owned-new");
    assert_eq!(queued_contents(&queue), vec!["owned-old"]);

    let reconnect_control = control(queue.clone());
    let replay = reconnect_control
        .recall_soft_interrupt("client-a", "stable-operation")
        .expect("reconnect replay should return the completed operation result");
    assert_eq!(replay, first);
    assert_eq!(
        queued_contents(&queue),
        vec!["owned-old"],
        "replaying an operation must not remove another message"
    );
}

#[test]
fn recall_replay_preserves_an_authoritative_empty_result() {
    let queue = Arc::new(Mutex::new(Vec::new()));
    let first_control = SessionControlHandle::new(
        "session-empty-replay-tests",
        queue.clone(),
        InterruptSignal::new(),
        InterruptSignal::new(),
    );
    assert!(
        first_control
            .recall_soft_interrupt("client-a", "empty-operation")
            .is_none()
    );

    queue
        .lock()
        .expect("soft interrupt queue lock")
        .push(message(
            "arrived-later",
            SoftInterruptSource::User,
            Some("client-a"),
        ));
    let reconnect_control = SessionControlHandle::new(
        "session-empty-replay-tests",
        queue.clone(),
        InterruptSignal::new(),
        InterruptSignal::new(),
    );

    assert!(
        reconnect_control
            .recall_soft_interrupt("client-a", "empty-operation")
            .is_none(),
        "the same operation must replay its original empty result"
    );
    assert_eq!(queued_contents(&queue), vec!["arrived-later"]);
}

#[test]
fn recall_replay_record_is_bounded_per_session() {
    let queue = Arc::new(Mutex::new(
        (0..=SOFT_INTERRUPT_REPLAY_CAPACITY)
            .map(|index| {
                message(
                    &format!("owned-{index}"),
                    SoftInterruptSource::User,
                    Some("bounded-client"),
                )
            })
            .collect(),
    ));
    let control = SessionControlHandle::new(
        "session-bounded-replay-tests",
        queue,
        InterruptSignal::new(),
        InterruptSignal::new(),
    );

    for index in 0..=SOFT_INTERRUPT_REPLAY_CAPACITY {
        assert!(
            control
                .recall_soft_interrupt("bounded-client", &format!("operation-{index}"))
                .is_some()
        );
    }

    let replay = control
        .soft_interrupt_replay
        .lock()
        .expect("soft interrupt replay lock");
    assert_eq!(replay.completed.len(), SOFT_INTERRUPT_REPLAY_CAPACITY);
    assert!(replay.get("bounded-client", "operation-0").is_none());
    assert!(
        replay
            .get(
                "bounded-client",
                &format!("operation-{}", SOFT_INTERRUPT_REPLAY_CAPACITY)
            )
            .is_some()
    );
}
