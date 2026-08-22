use jcode_session_types::lifecycle::{
    LifecycleObservabilityStatus, SessionLifecycleStream,
};

#[test]
fn lifecycle_request_and_response_round_trip_preserve_correlation_and_stream() {
    let request = Request::GetLifecycleEvents {
        id: 42,
        session_id: "synthetic-session-001".to_string(),
    };
    let json = serde_json::to_string(&request).expect("serialize lifecycle request");
    assert_eq!(
        json,
        r#"{"type":"get_lifecycle_events","id":42,"session_id":"synthetic-session-001"}"#
    );
    match serde_json::from_str::<Request>(&json).expect("deserialize lifecycle request") {
        Request::GetLifecycleEvents { id, session_id } => {
            assert_eq!(id, 42);
            assert_eq!(session_id, "synthetic-session-001");
        }
        other => panic!("unexpected lifecycle request: {other:?}"),
    }

    let event = ServerEvent::LifecycleEvents {
        id: 42,
        stream: SessionLifecycleStream {
            session_id: "synthetic-session-001".to_string(),
            status: LifecycleObservabilityStatus {
                enabled: true,
                persist_session_events: true,
                emit_structured_logs: false,
            },
            events: Vec::new(),
            warnings: Vec::new(),
        },
    };
    let json = serde_json::to_string(&event).expect("serialize lifecycle response");
    match serde_json::from_str::<ServerEvent>(&json).expect("deserialize lifecycle response") {
        ServerEvent::LifecycleEvents { id, stream } => {
            assert_eq!(id, 42);
            assert_eq!(stream.session_id, "synthetic-session-001");
            assert!(stream.events.is_empty());
            assert!(stream.warnings.is_empty());
        }
        other => panic!("unexpected lifecycle response: {other:?}"),
    }
}

#[test]
fn existing_error_event_remains_the_correlated_invalid_session_response() {
    let json = r#"{"type":"error","id":42,"message":"invalid session","provider_code":"invalid_session"}"#;
    match serde_json::from_str::<ServerEvent>(json).expect("deserialize correlated error") {
        ServerEvent::Error {
            id,
            message,
            provider_code,
            ..
        } => {
            assert_eq!(id, 42);
            assert_eq!(message, "invalid session");
            assert_eq!(provider_code.as_deref(), Some("invalid_session"));
        }
        other => panic!("unexpected error response: {other:?}"),
    }
}
