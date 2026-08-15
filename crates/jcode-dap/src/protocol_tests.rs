use super::protocol::{DapEventMessage, DapRequestMessage, DapResponseMessage};

#[test]
fn dap_messages_round_trip_unknown_adapter_fields() {
    let request = DapRequestMessage::new(7, "launch", serde_json::json!({
        "program": "/tmp/app",
        "adapterSpecific": {"stopOnEntry": true}
    }));
    let value = serde_json::to_value(&request).expect("serialize request");
    let decoded: DapRequestMessage = serde_json::from_value(value).expect("deserialize request");
    assert_eq!(decoded.seq, 7);
    assert_eq!(decoded.command, "launch");
    assert_eq!(decoded.arguments["adapterSpecific"]["stopOnEntry"], true);

    let response = DapResponseMessage::success(8, 7, "launch", serde_json::json!({"ok": true}));
    assert!(response.success);
    let event = DapEventMessage::new(9, "stopped", serde_json::json!({"threadId": 1}));
    assert_eq!(event.event, "stopped");
}

#[test]
fn deterministic_python_and_native_recordings_cover_common_adapter_messages() {
    for recording in [
        include_str!("../tests/recordings/python-basic.jsonl"),
        include_str!("../tests/recordings/native-basic.jsonl"),
    ] {
        let messages = recording
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("recording message"))
            .collect::<Vec<_>>();
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0]["command"], "initialize");
        assert_eq!(messages[1]["event"], "output");
        assert_eq!(messages[2]["event"], "stopped");
        assert_eq!(messages[3]["command"], "threads");
    }
}
