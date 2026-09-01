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
    let control = control(queue.clone());

    let first = control
        .recall_soft_interrupt("client-a", "stable-operation")
        .expect("first recall should return newest message");
    assert_eq!(first.content, "owned-new");
    assert_eq!(queued_contents(&queue), vec!["owned-old"]);

    let replay = control
        .recall_soft_interrupt("client-a", "stable-operation")
        .expect("replay should return the completed operation result");
    assert_eq!(replay, first);
    assert_eq!(
        queued_contents(&queue),
        vec!["owned-old"],
        "replaying an operation must not remove another message"
    );
}
