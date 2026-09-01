#[test]
fn recall_soft_interrupt_request_roundtrip_preserves_operation_identity() -> Result<()> {
    let request = Request::RecallSoftInterrupt {
        id: 41,
        operation_id: "recall-operation-1".to_string(),
    };

    let json = serde_json::to_string(&request)?;
    assert_eq!(
        json,
        r#"{"type":"recall_soft_interrupt","id":41,"operation_id":"recall-operation-1"}"#
    );

    let decoded = parse_request_json(&json)?;
    let Request::RecallSoftInterrupt { id, operation_id } = decoded else {
        return Err(anyhow!("wrong request type"));
    };
    assert_eq!(id, 41);
    assert_eq!(operation_id, "recall-operation-1");
    Ok(())
}

#[test]
fn soft_interrupt_recalled_roundtrip_preserves_exact_optional_message() -> Result<()> {
    let message = RecallableSoftInterrupt {
        content: "edit this queued message".to_string(),
        images: vec![
            ("image/png".to_string(), "cG5n".to_string()),
            ("image/jpeg".to_string(), "anBlZw==".to_string()),
        ],
    };
    let event = ServerEvent::SoftInterruptRecalled {
        id: 42,
        operation_id: "recall-operation-2".to_string(),
        message: Some(message.clone()),
    };

    let json = serde_json::to_string(&event)?;
    let decoded = parse_event_json(&json)?;
    let ServerEvent::SoftInterruptRecalled {
        id,
        operation_id,
        message: decoded_message,
    } = decoded
    else {
        return Err(anyhow!("wrong event type"));
    };
    assert_eq!(id, 42);
    assert_eq!(operation_id, "recall-operation-2");
    assert_eq!(decoded_message, Some(message));

    let no_message = ServerEvent::SoftInterruptRecalled {
        id: 43,
        operation_id: "recall-operation-3".to_string(),
        message: None,
    };
    let json = serde_json::to_string(&no_message)?;
    assert!(json.contains(r#""message":null"#));
    let decoded = parse_event_json(&json)?;
    let ServerEvent::SoftInterruptRecalled { message, .. } = decoded else {
        return Err(anyhow!("wrong empty event type"));
    };
    assert_eq!(message, None);
    Ok(())
}
