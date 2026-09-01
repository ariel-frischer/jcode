use crate::*;

const CAPABILITY: &str = "queued_message_navigation_v1";

fn selection() -> QueuedMessageEditorSelection {
    QueuedMessageEditorSelection {
        message_id: "message-2".into(),
        content: "updated".into(),
        images: vec![
            ("image/png".into(), "first".into()),
            ("image/jpeg".into(), "second".into()),
        ],
        older_available: true,
        newer_available: false,
    }
}

#[test]
fn capability_discovery_is_additive_and_legacy_peers_remain_supported() {
    let capable: ServerFrame = serde_json::from_str(&format!(
        r#"{{"v":1,"ev":"hello_ok","version":1,"server":"jcode/test","capabilities":["{CAPABILITY}"]}}"#
    ))
    .unwrap();
    let legacy: ServerFrame =
        serde_json::from_str(r#"{"v":1,"ev":"hello_ok","version":1,"server":"jcode/legacy"}"#)
            .unwrap();

    assert!(matches!(
        capable.event,
        ApiEvent::HelloOk { capabilities, .. } if capabilities == [CAPABILITY]
    ));
    assert!(matches!(
        legacy.event,
        ApiEvent::HelloOk { capabilities, .. } if capabilities.is_empty()
    ));
}

#[test]
fn editor_request_roundtrips_typed_operations_and_ordered_images() {
    let requests = [
        QueuedMessageEditorOperation::Start,
        QueuedMessageEditorOperation::Move {
            direction: QueuedMessageEditorDirection::Older,
            selected_message_id: "message-2".into(),
            draft: QueuedMessageEditorDraft {
                content: "updated".into(),
                images: selection().images,
            },
        },
        QueuedMessageEditorOperation::Release,
    ];

    for operation in requests {
        let frame = ClientFrame::new(
            7,
            ApiRequest::QueuedMessageEditor {
                session_id: "session-1".into(),
                navigation_session_id: "navigation-1".into(),
                operation_id: "operation-1".into(),
                operation,
            },
        );
        let encoded = serde_json::to_string(&frame).unwrap();
        let decoded: ClientFrame = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, frame);
    }
}

#[test]
fn editor_results_represent_every_public_outcome_and_placement() {
    let outcomes = [
        QueuedMessageEditorOutcome::Started,
        QueuedMessageEditorOutcome::Moved,
        QueuedMessageEditorOutcome::Boundary,
        QueuedMessageEditorOutcome::Committed,
        QueuedMessageEditorOutcome::Deleted,
        QueuedMessageEditorOutcome::Released,
        QueuedMessageEditorOutcome::StalePlacement,
        QueuedMessageEditorOutcome::Conflict,
        QueuedMessageEditorOutcome::Replay,
    ];
    let placements = [
        QueuedMessageEditorPlacement::Exact,
        QueuedMessageEditorPlacement::StaleBestEffort,
        QueuedMessageEditorPlacement::NotApplied,
    ];

    for (index, outcome) in outcomes.into_iter().enumerate() {
        let frame = ServerFrame::event(ApiEvent::QueuedMessageEditorResult {
            session_id: "session-1".into(),
            navigation_session_id: "navigation-1".into(),
            operation_id: format!("operation-{index}"),
            outcome,
            selection: Some(selection()),
            placement: placements[index % placements.len()],
            message: Some("visible result".into()),
        });
        let encoded = serde_json::to_string(&frame).unwrap();
        let decoded: ServerFrame = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, frame);
    }
}
