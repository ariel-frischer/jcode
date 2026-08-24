#[tokio::test]
async fn lifecycle_query_flushes_recorder_before_reading_typed_stream() {
    let root = tempfile::tempdir().expect("create lifecycle query root");
    let recorder = crate::lifecycle_observability::LifecycleRecorder::new_with_clock(
        crate::config::LifecycleObservabilityConfig::default(),
        root.path().to_path_buf(),
        8,
        std::sync::Arc::new(chrono::Utc::now),
    );
    let session_id = "query-session-typed";
    assert_eq!(
        recorder.submit(
            session_id,
            crate::session::lifecycle_types::LifecycleEvent::Retry {
                decision_type: crate::session::lifecycle_types::LifecycleDecisionType::Started,
                semantic_reason:
                    crate::session::lifecycle_types::LifecycleSemanticReason::ContextLimit,
                suppression_reason: None,
                attempt: 1,
                max_attempts: 3,
                process_manifest_id: None,
            },
        ),
        crate::lifecycle_observability::LifecycleSubmitOutcome::Accepted
    );
    assert_eq!(
        recorder.submit(
            session_id,
            crate::session::lifecycle_types::LifecycleEvent::Block {
                decision_type: crate::session::lifecycle_types::LifecycleDecisionType::Suppressed,
                semantic_reason: crate::session::lifecycle_types::LifecycleSemanticReason::Policy,
                suppression_reason: Some(
                    crate::session::lifecycle_types::LifecycleSuppressionReason::PolicyDenied,
                ),
                process_manifest_id: None,
            },
        ),
        crate::lifecycle_observability::LifecycleSubmitOutcome::Accepted
    );

    let stream = super::read_lifecycle_query_stream(&recorder, session_id, root.path())
        .await
        .expect("read flushed lifecycle stream");
    assert_eq!(stream.session_id, session_id);
    assert_eq!(stream.events.len(), 2);
    assert_eq!(stream.events[0].sequence, 1);
    assert_eq!(stream.events[1].sequence, 2);
    assert!(matches!(
        stream.events[0].event,
        crate::session::lifecycle_types::LifecycleEvent::Retry { .. }
    ));
    assert!(matches!(
        stream.events[1].event,
        crate::session::lifecycle_types::LifecycleEvent::Block { .. }
    ));
    assert!(stream.warnings.is_empty());
}

#[tokio::test]
async fn lifecycle_query_reports_persistence_failure_without_hiding_the_stream() {
    let root = tempfile::tempdir().expect("create lifecycle query root");
    let invalid_base = root.path().join("not-a-directory");
    std::fs::write(&invalid_base, b"file").expect("create invalid persistence root");
    let recorder = crate::lifecycle_observability::LifecycleRecorder::new_with_clock(
        crate::config::LifecycleObservabilityConfig::default(),
        invalid_base,
        8,
        std::sync::Arc::new(chrono::Utc::now),
    );
    let session_id = "query-session-persistence-failure";
    assert_eq!(
        recorder.submit(
            session_id,
            crate::session::lifecycle_types::LifecycleEvent::Block {
                decision_type: crate::session::lifecycle_types::LifecycleDecisionType::Suppressed,
                semantic_reason: crate::session::lifecycle_types::LifecycleSemanticReason::Policy,
                suppression_reason: None,
                process_manifest_id: None,
            },
        ),
        crate::lifecycle_observability::LifecycleSubmitOutcome::Accepted
    );

    let stream = super::read_lifecycle_query_stream(&recorder, session_id, root.path())
        .await
        .expect("persistence diagnostics must not hide the readable stream");
    assert!(stream.events.is_empty());
    assert!(stream.warnings.contains(
        &crate::session::lifecycle_types::LifecycleCompatibilityWarning::PersistenceUnavailable
    ));
}

#[tokio::test]
async fn lifecycle_query_propagates_sidecar_read_failures() {
    let root = tempfile::tempdir().expect("create lifecycle query root");
    let recorder = crate::lifecycle_observability::LifecycleRecorder::new_with_clock(
        crate::config::LifecycleObservabilityConfig::default(),
        root.path().to_path_buf(),
        8,
        std::sync::Arc::new(chrono::Utc::now),
    );
    let session_id = "query-session-read-failure";
    let path = crate::session::lifecycle_path_in_dir(root.path(), session_id)
        .expect("valid lifecycle path");
    std::fs::create_dir_all(&path).expect("make lifecycle sidecar unreadable as a file");

    let error = super::read_lifecycle_query_stream(&recorder, session_id, root.path())
        .await
        .expect_err("sidecar read failure must remain explicit");
    assert!(!crate::util::format_error_chain(&error).trim().is_empty());
}
