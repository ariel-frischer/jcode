use super::*;

const PROCESS_CHILD_ENV: &str = "JCODE_RUN_SAFETY_PROCESS_CHILD";

#[tokio::test]
async fn run_safety_process_child() {
    let Ok(mode) = std::env::var(PROCESS_CHILD_ENV) else {
        return;
    };

    let _guard = crate::storage::lock_test_env();
    let _saved = SavedEnv::capture(&["JCODE_HOME", "JCODE_RUN_AUTO_POKE"]);
    let temp = tempfile::tempdir().expect("tempdir");
    crate::env::set_var("JCODE_HOME", temp.path());
    crate::env::set_var("JCODE_RUN_AUTO_POKE", "0");
    let provider: Arc<dyn Provider> = Arc::new(TestProvider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = crate::agent::Agent::new(provider.clone(), registry);
    let mut turn_limit = run_safety::RunTurnLimit::parse(Some("1")).expect("bounded limit");

    let (emit_json, emit_ndjson) = match mode.as_str() {
        "plain" => (false, false),
        "json" => (true, false),
        "ndjson" => (false, true),
        other => panic!("unknown process child mode: {other}"),
    };
    run_safety::run_single_message_with_agent(
        &mut agent,
        provider,
        "Return the test response.",
        emit_json,
        emit_ndjson,
        &mut turn_limit,
    )
    .await
    .expect("bounded child run");
}

fn process_output(mode: &str) -> std::process::Output {
    std::process::Command::new(std::env::current_exe().expect("current test executable"))
        .args(["run_safety_process_child", "--nocapture"])
        .env(PROCESS_CHILD_ENV, mode)
        .output()
        .unwrap_or_else(|error| panic!("spawn {mode} process child: {error}"))
}

fn first_json_object(output: &str) -> serde_json::Value {
    let start = output.find('{').expect("JSON object start");
    let end = output[start..]
        .find("\n}")
        .map(|offset| start + offset + 2)
        .expect("JSON object end");
    serde_json::from_str(&output[start..end]).expect("valid JSON object")
}

#[test]
fn bounded_run_process_streams_and_exit_status_match_the_contract() {
    let plain = process_output("plain");
    assert!(
        plain.status.success(),
        "plain exit status: {:?}",
        plain.status
    );
    let plain_stdout = String::from_utf8(plain.stdout).expect("plain stdout");
    let plain_stderr = String::from_utf8(plain.stderr).expect("plain stderr");
    let stop_message = "Run stopped: maximum turns reached (max_turns_reached)";
    assert!(
        plain_stderr.contains(stop_message),
        "plain stderr: {plain_stderr:?}"
    );
    assert!(
        !plain_stdout.contains(stop_message),
        "plain stdout: {plain_stdout:?}"
    );

    let json = process_output("json");
    assert!(json.status.success(), "JSON exit status: {:?}", json.status);
    assert!(
        json.stderr.is_empty(),
        "JSON stderr: {:?}",
        String::from_utf8_lossy(&json.stderr)
    );
    let report = first_json_object(&String::from_utf8(json.stdout).expect("JSON stdout"));
    assert_eq!(report["stop_reason"], "max_turns_reached");
    assert_eq!(report["outcome"], "bounded_stop");
    assert_eq!(report["safety_bound"]["bound"], "max_turns");

    let ndjson = process_output("ndjson");
    assert!(
        ndjson.status.success(),
        "NDJSON exit status: {:?}",
        ndjson.status
    );
    assert!(
        ndjson.stderr.is_empty(),
        "NDJSON stderr: {:?}",
        String::from_utf8_lossy(&ndjson.stderr)
    );
    let stdout = String::from_utf8(ndjson.stdout).expect("NDJSON stdout");
    let values: Vec<serde_json::Value> = stdout
        .lines()
        .filter_map(|line| line.find('{').map(|start| &line[start..]))
        .map(|line| serde_json::from_str(line).expect("valid NDJSON line"))
        .collect();
    assert!(values.len() >= 2, "NDJSON events: {values:?}");
    let done = values.last().expect("NDJSON done event");
    assert_eq!(done["type"], "done");
    assert_eq!(done["stop_reason"], "max_turns_reached");
    assert_eq!(done["outcome"], "bounded_stop");
    assert_eq!(done["safety_bound"]["bound"], "max_turns");
}

#[tokio::test]
async fn bounded_run_stops_sequential_tool_rounds_inside_the_turn() {
    let _guard = crate::storage::lock_test_env();
    let _saved = SavedEnv::capture(&["JCODE_HOME", "JCODE_RUN_AUTO_POKE"]);
    let temp = tempfile::tempdir().expect("tempdir");
    crate::env::set_var("JCODE_HOME", temp.path());
    crate::env::set_var("JCODE_RUN_AUTO_POKE", "0");
    let calls = Arc::new(AtomicUsize::new(0));
    let provider: Arc<dyn Provider> = Arc::new(SequentialToolProvider {
        calls: Arc::clone(&calls),
    });
    let registry = Registry::new(provider.clone()).await;
    let mut agent = crate::agent::Agent::new(provider.clone(), registry);
    let mut turn_limit = run_safety::RunTurnLimit::parse(Some("1")).expect("bounded limit");

    run_safety::run_single_message_with_agent(
        &mut agent,
        provider,
        "Keep calling one tool at a time.",
        true,
        false,
        &mut turn_limit,
    )
    .await
    .expect("bounded sequential-tool run");

    assert_eq!(
        calls.load(Ordering::SeqCst),
        run_safety::MAX_TOOL_ROUNDS_PER_BOUNDED_TURN,
        "the inner loop must stop before another provider/tool round"
    );
    assert_eq!(
        turn_limit.stop_reason(),
        Some(run_safety::RunStopReason::MaxToolRoundsReached)
    );
}

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
