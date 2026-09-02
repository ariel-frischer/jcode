use serde_json::{Value, json};

fn ordered_editor_images(label: &str) -> Value {
    json!([
        ["image/png", format!("{label}-png-bytes-base64")],
        ["image/jpeg", format!("{label}-jpeg-bytes-base64")],
        ["image/webp", format!("{label}-webp-bytes-base64")]
    ])
}

fn assert_editor_request_roundtrip(operation: Value) -> Result<()> {
    let expected = json!({
        "type": "queued_message_editor",
        "id": 501,
        "navigation_session_id": "navigation-session-1",
        "operation_id": "operation-1",
        "operation": operation,
    });
    let decoded: Request = serde_json::from_value(expected.clone())?;
    assert_eq!(serde_json::to_value(decoded)?, expected);
    Ok(())
}

#[test]
fn queued_message_editor_start_request_roundtrips_stable_identities() -> Result<()> {
    assert_editor_request_roundtrip(json!({ "kind": "start" }))
}

#[test]
fn queued_message_editor_move_older_request_roundtrips_ordered_images() -> Result<()> {
    assert_editor_request_roundtrip(json!({
        "kind": "move",
        "direction": "older",
        "selected_message_id": "message-newest",
        "draft": {
            "content": "edited newest draft",
            "images": ordered_editor_images("older"),
        }
    }))
}

#[test]
fn queued_message_editor_move_newer_request_roundtrips_ordered_images() -> Result<()> {
    assert_editor_request_roundtrip(json!({
        "kind": "move",
        "direction": "newer",
        "selected_message_id": "message-older",
        "draft": {
            "content": "edited older draft",
            "images": ordered_editor_images("newer"),
        }
    }))
}

#[test]
fn queued_message_editor_finish_request_roundtrips_ordered_images() -> Result<()> {
    assert_editor_request_roundtrip(json!({
        "kind": "finish",
        "selected_message_id": "message-selected",
        "draft": {
            "content": "committed draft",
            "images": ordered_editor_images("finish"),
        }
    }))
}

#[test]
fn queued_message_editor_release_request_roundtrips_stable_identities() -> Result<()> {
    assert_editor_request_roundtrip(json!({ "kind": "release" }))
}

#[test]
fn queued_message_editor_results_roundtrip_all_outcomes_and_placements() -> Result<()> {
    let cases = [
        ("started", "exact", true),
        ("moved", "exact", true),
        ("boundary", "exact", true),
        ("committed", "exact", false),
        ("deleted", "exact", false),
        ("released", "exact", false),
        ("stale_placement", "stale_best_effort", false),
        ("conflict", "not_applied", true),
        ("replay", "exact", true),
    ];

    for (outcome, placement, has_selection) in cases {
        let selection = has_selection.then(|| {
            json!({
                "message_id": "message-selected",
                "content": format!("{outcome} selected draft"),
                "images": ordered_editor_images(outcome),
                "older_available": true,
                "newer_available": false,
            })
        });
        let expected = json!({
            "type": "queued_message_editor_result",
            "id": 502,
            "navigation_session_id": "navigation-session-1",
            "operation_id": format!("operation-{outcome}"),
            "outcome": outcome,
            "selection": selection,
            "placement": placement,
            "message": format!("visible {outcome} feedback"),
        });
        let decoded: ServerEvent = serde_json::from_value(expected.clone())?;
        assert_eq!(serde_json::to_value(decoded)?, expected, "outcome {outcome}");
    }
    Ok(())
}

#[test]
fn legacy_recall_soft_interrupt_json_is_unchanged_by_editor_contract() -> Result<()> {
    let request = Request::RecallSoftInterrupt {
        id: 503,
        operation_id: "legacy-recall-operation".to_string(),
    };
    assert_eq!(
        serde_json::to_string(&request)?,
        r#"{"type":"recall_soft_interrupt","id":503,"operation_id":"legacy-recall-operation"}"#
    );
    Ok(())
}

#[test]
fn queued_message_editor_rejects_empty_or_malformed_operation_identity() {
    let invalid = [
        json!({
            "type": "queued_message_editor",
            "id": 504,
            "navigation_session_id": "",
            "operation_id": "operation-1",
            "operation": { "kind": "start" },
        }),
        json!({
            "type": "queued_message_editor",
            "id": 505,
            "navigation_session_id": "navigation-session-1",
            "operation_id": "",
            "operation": { "kind": "start" },
        }),
        json!({
            "type": "queued_message_editor",
            "id": 506,
            "navigation_session_id": "navigation-session-1",
            "operation_id": "operation-invalid-direction",
            "operation": {
                "kind": "move",
                "direction": "sideways",
                "selected_message_id": "message-selected",
                "draft": { "content": "draft", "images": [] },
            },
        }),
        json!({
            "type": "queued_message_editor",
            "id": 507,
            "navigation_session_id": "navigation-session-1",
            "operation_id": "operation-missing-draft",
            "operation": {
                "kind": "finish",
                "selected_message_id": "message-selected",
            },
        }),
    ];

    for value in invalid {
        assert!(
            serde_json::from_value::<Request>(value).is_err(),
            "malformed editor request must be rejected"
        );
    }
}
