use super::*;

#[tokio::test]
async fn max_turns_bounds_all_one_shot_output_modes() {
    let _guard = crate::storage::lock_test_env();
    let _saved = SavedEnv::capture(&["JCODE_HOME", "JCODE_RUN_AUTO_POKE"]);
    let temp = tempfile::tempdir().expect("tempdir");
    crate::env::set_var("JCODE_HOME", temp.path());
    crate::env::set_var("JCODE_RUN_AUTO_POKE", "1");

    for (mode, emit_json, emit_ndjson) in [
        ("plain", false, false),
        ("json", true, false),
        ("ndjson", false, true),
    ] {
        let provider: Arc<dyn Provider> = Arc::new(TestProvider);
        let registry = Registry::new(provider.clone()).await;
        let mut agent = crate::agent::Agent::new(provider.clone(), registry);
        let session_id = agent.session_id().to_string();
        let mut turn_limit = run_safety::RunTurnLimit::parse(Some("1")).expect("bounded limit");

        run_safety::run_single_message_with_agent(
            &mut agent,
            provider,
            "Return the test response.",
            emit_json,
            emit_ndjson,
            &mut turn_limit,
        )
        .await
        .unwrap_or_else(|error| panic!("{mode} bounded run failed: {error:#}"));

        assert_eq!(
            turn_limit.stop_reason(),
            Some(run_safety::RunStopReason::MaxTurnsReached),
            "{mode} did not report the configured safety bound"
        );
        let persisted = crate::session::Session::load(&session_id)
            .unwrap_or_else(|error| panic!("load bounded {mode} session: {error:#}"));
        assert!(
            matches!(persisted.status, crate::session::SessionStatus::Closed),
            "{mode} bounded run did not close its session"
        );
    }
}

#[tokio::test]
async fn one_shot_cleanup_preserves_the_original_command_error() {
    let _guard = crate::storage::lock_test_env();
    let _saved = SavedEnv::capture(&["JCODE_HOME", "JCODE_RUN_AUTO_POKE"]);
    let temp = tempfile::tempdir().expect("tempdir");
    crate::env::set_var("JCODE_HOME", temp.path());
    crate::env::set_var("JCODE_RUN_AUTO_POKE", "0");

    for (mode, emit_json, emit_ndjson) in [
        ("plain", false, false),
        ("json", true, false),
        ("ndjson", false, true),
    ] {
        let provider: Arc<dyn Provider> = Arc::new(FailingTestProvider);
        let registry = Registry::new(provider.clone()).await;
        let mut agent = crate::agent::Agent::new(provider.clone(), registry);
        let session_id = agent.session_id().to_string();
        let marker = crate::storage::active_pids_dir()
            .expect("active PID directory")
            .join(&session_id);
        let mut turn_limit = run_safety::RunTurnLimit::parse(None).expect("unbounded limit");

        let error = match run_safety::run_single_message_with_agent(
            &mut agent,
            provider,
            "Fail this run.",
            emit_json,
            emit_ndjson,
            &mut turn_limit,
        )
        .await
        {
            Ok(()) => panic!("{mode} provider failure should remain the command result"),
            Err(error) => error,
        };

        assert!(
            error.to_string().contains("one-shot sentinel failure"),
            "{mode} changed the original error: {error:#}"
        );
        assert!(
            !marker.exists(),
            "failed {mode} run left an active PID marker"
        );
        let persisted = crate::session::Session::load(&session_id)
            .unwrap_or_else(|error| panic!("load failed {mode} session: {error:#}"));
        assert!(matches!(
            persisted.status,
            crate::session::SessionStatus::Closed
        ));
    }
}
