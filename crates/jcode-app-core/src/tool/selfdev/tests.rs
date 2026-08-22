use super::*;
use crate::bus::BackgroundTaskStatus;
use std::ffi::OsStr;
use std::sync::Arc;

struct EnvVarGuard {
    key: &'static str,
    original: Option<std::ffi::OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: impl AsRef<OsStr>) -> Self {
        let original = std::env::var_os(key);
        crate::env::set_var(key, value);
        Self { key, original }
    }

    fn remove(key: &'static str) -> Self {
        let original = std::env::var_os(key);
        crate::env::remove_var(key);
        Self { key, original }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.original {
            Some(value) => crate::env::set_var(self.key, value),
            None => crate::env::remove_var(self.key),
        }
    }
}

fn create_test_context(session_id: &str, working_dir: Option<std::path::PathBuf>) -> ToolContext {
    ToolContext {
        session_id: session_id.to_string(),
        message_id: "test-message".to_string(),
        tool_call_id: "test-tool-call".to_string(),
        working_dir,
        stdin_request_tx: None,
        graceful_shutdown_signal: None,
        execution_mode: crate::tool::ToolExecutionMode::Direct,
    }
}

fn create_repo_fixture() -> tempfile::TempDir {
    let temp = tempfile::TempDir::new().expect("temp repo");
    std::fs::create_dir_all(temp.path().join(".git")).expect("git dir");
    std::fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname = \"jcode\"\nversion = \"0.1.0\"\n",
    )
    .expect("cargo toml");
    temp
}

fn test_source_state(repo_dir: &std::path::Path) -> build::SourceState {
    build::SourceState {
        repo_scope: "test-repo-scope".to_string(),
        worktree_scope: build::worktree_scope_key(repo_dir)
            .unwrap_or_else(|_| "test-worktree".to_string()),
        short_hash: "test-build".to_string(),
        full_hash: "test-build-full".to_string(),
        dirty: true,
        fingerprint: "test-fingerprint".to_string(),
        version_label: "test-build".to_string(),
        changed_paths: 0,
    }
}

fn request_fixture(
    request_id: &str,
    state: BuildRequestState,
    requested_at: String,
) -> BuildRequest {
    let source = test_source_state(std::path::Path::new("/tmp/jcode"));
    BuildRequest {
        request_id: request_id.to_string(),
        background_task_id: None,
        session_id: "session-history-test".to_string(),
        session_short_name: None,
        session_title: None,
        reason: request_id.to_string(),
        repo_dir: "/tmp/jcode".to_string(),
        repo_scope: source.repo_scope.clone(),
        worktree_scope: source.worktree_scope.clone(),
        command: "cargo test -p jcode-base".to_string(),
        requested_at,
        started_at: None,
        completed_at: None,
        state,
        version: None,
        dedupe_key: None,
        requested_source: Some(source),
        built_source: None,
        published_version: None,
        last_progress: None,
        validated: false,
        error: None,
        output_file: None,
        status_file: None,
        attached_to_request_id: None,
    }
}

#[test]
fn build_lock_is_removed_on_drop_and_can_be_reacquired() {
    let _env_lock = crate::storage::lock_test_env();
    let temp = tempfile::tempdir().expect("temp jcode home");
    let _home = EnvVarGuard::set("JCODE_HOME", temp.path());
    let scope = format!("lock-drop-{}", std::process::id());
    let path = SelfDevTool::build_lock_path(&scope).expect("lock path");

    let first = SelfDevTool::try_acquire_build_lock(&scope)
        .expect("first lock attempt")
        .expect("first lock acquired");
    assert!(path.exists(), "lock file should exist while held");
    drop(first);
    assert!(!path.exists(), "lock file should be removed on drop");

    let second = SelfDevTool::try_acquire_build_lock(&scope)
        .expect("second lock attempt")
        .expect("lock should be reacquirable after drop");
    drop(second);
    assert!(!path.exists(), "reacquired lock should also clean up");
}

#[test]
fn terminal_request_history_is_archived_without_touching_active_requests() {
    // One shared env lock only: `lock_test_env` is a plain non-reentrant mutex,
    // so taking a second env guard here would self-deadlock (issue #593).
    let _storage_guard = crate::storage::lock_test_env();
    let temp = tempfile::tempdir().expect("temp jcode home");
    let _home = EnvVarGuard::set("JCODE_HOME", temp.path());
    let _limit = EnvVarGuard::set("JCODE_SELFDEV_REQUEST_HISTORY_LIMIT", "2");

    let base = Utc::now() - chrono::Duration::minutes(10);
    for index in 0..4 {
        request_fixture(
            &format!("terminal-{index}"),
            BuildRequestState::Completed,
            (base + chrono::Duration::minutes(index)).to_rfc3339(),
        )
        .save()
        .expect("save terminal request");
    }
    request_fixture(
        "active-request",
        BuildRequestState::Queued,
        Utc::now().to_rfc3339(),
    )
    .save()
    .expect("save active request");

    let live = BuildRequest::load_all().expect("load live requests");
    let live_ids = live
        .iter()
        .map(|request| request.request_id.as_str())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(live.len(), 3);
    assert!(live_ids.contains("terminal-2"));
    assert!(live_ids.contains("terminal-3"));
    assert!(live_ids.contains("active-request"));

    let archive = BuildRequest::requests_dir()
        .expect("requests dir")
        .join("archive");
    assert!(archive.join("terminal-0.json").exists());
    assert!(archive.join("terminal-1.json").exists());
}

#[test]
fn optimized_test_shell_command_routes_compile_subcommands_only() {
    let shell = SelfDevTool::optimized_test_shell_command(
        "cargo test -p jcode-base && cargo fmt --all -- --check",
    );

    assert!(shell.contains("test|check|build|clippy|bench"));
    assert!(shell.contains("JCODE_DEV_CARGO_SCRIPT"));
    assert!(shell.contains("JCODE_IN_DEV_CARGO=1"));
    assert!(shell.contains("*) command cargo \"$@\" ;;"));
    assert!(shell.ends_with("cargo test -p jcode-base && cargo fmt --all -- --check"));
}

#[test]
fn versioned_request_identity_preserves_scope_source_action_and_exact_command() {
    let repo = create_repo_fixture();
    let source = test_source_state(repo.path());
    let build_command = SelfDevBuildCommand {
        program: "scripts/dev_cargo.sh".to_string(),
        args: vec!["build".to_string()],
        display: "scripts/dev_cargo.sh build --profile selfdev -p jcode".to_string(),
    };

    let build_key = SelfDevTool::build_dedupe_key(&source, &build_command);
    let test_key = SelfDevTool::eligible_test_dedupe_key(
        &source,
        "scripts/dev_cargo.sh build --profile selfdev -p jcode",
    )
    .expect("single exact dev_cargo invocation should be eligible");

    assert!(build_key.starts_with("selfdev-cargo-v1:build:"));
    assert!(test_key.starts_with("selfdev-cargo-v1:test:"));
    for dimension in [
        source.worktree_scope.as_str(),
        source.fingerprint.as_str(),
        build_command.display.as_str(),
    ] {
        assert!(build_key.contains(dimension));
        assert!(test_key.contains(dimension));
    }
    assert_ne!(
        build_key, test_key,
        "public action is an identity dimension"
    );
}

#[test]
fn eligible_test_identity_accepts_only_single_unambiguous_cargo_commands() {
    let repo = create_repo_fixture();
    let source = test_source_state(repo.path());

    for command in [
        "cargo build -p jcode",
        "cargo test -p jcode --lib",
        "cargo check -p jcode --all-targets",
        "scripts/dev_cargo.sh build --profile selfdev -p jcode",
        "./scripts/dev_cargo.sh test -p jcode-app-core",
    ] {
        let key = SelfDevTool::eligible_test_dedupe_key(&source, command)
            .unwrap_or_else(|| panic!("expected eligible command: {command}"));
        assert!(key.ends_with(command), "identity must retain exact command");
    }

    for command in [
        "",
        "cargo",
        "cargo clippy -p jcode",
        "cargo bench -p jcode",
        "env RUSTFLAGS=-Dwarnings cargo check -p jcode",
        "RUSTFLAGS=-Dwarnings cargo check -p jcode",
        "cargo test -p jcode && cargo check -p jcode",
        "cargo test -p jcode | tee test.log",
        "cargo test -p jcode > test.log",
        "cargo test -p 'jcode'",
        "bash -lc cargo test -p jcode",
        "echo cargo test -p jcode",
    ] {
        assert!(
            SelfDevTool::eligible_test_dedupe_key(&source, command).is_none(),
            "opaque or potentially side-effecting command must stay independent: {command}"
        );
    }
}

#[cfg(unix)]
#[test]
fn optimized_test_shell_command_executes_raw_cargo_test_through_wrapper() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("temp command wrapper");
    let wrapper = temp.path().join("dev_cargo.sh");
    let capture = temp.path().join("args.txt");
    std::fs::write(
        &wrapper,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" > \"$JCODE_TEST_CAPTURE\"\n",
    )
    .expect("write wrapper");
    let mut permissions = std::fs::metadata(&wrapper)
        .expect("wrapper metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&wrapper, permissions).expect("make wrapper executable");

    let status = std::process::Command::new("bash")
        .args([
            "-lc",
            &SelfDevTool::optimized_test_shell_command("cargo test -p demo --lib"),
        ])
        .env("JCODE_DEV_CARGO_SCRIPT", &wrapper)
        .env("JCODE_TEST_CAPTURE", &capture)
        .env_remove("JCODE_IN_DEV_CARGO")
        .status()
        .expect("run optimized shell command");

    assert!(status.success());
    assert_eq!(
        std::fs::read_to_string(capture).expect("captured args"),
        "test -p demo --lib\n"
    );
}

async fn wait_for_task_completion(task_id: &str) -> background::TaskStatusFile {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if let Some(status) = background::global().status(task_id).await
            && status.status != BackgroundTaskStatus::Running
        {
            return status;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for background task {}",
            task_id
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

async fn execute_concurrent_selfdev_requests(
    repo_dir: &std::path::Path,
    inputs: Vec<serde_json::Value>,
) -> Vec<ToolOutput> {
    let barrier = Arc::new(tokio::sync::Barrier::new(inputs.len()));
    let mut handles = Vec::with_capacity(inputs.len());

    for (index, input) in inputs.into_iter().enumerate() {
        let barrier = Arc::clone(&barrier);
        let repo_dir = repo_dir.to_path_buf();
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            SelfDevTool::new()
                .execute(
                    input,
                    create_test_context(&format!("concurrent-session-{index}"), Some(repo_dir)),
                )
                .await
                .expect("concurrent selfdev request")
        }));
    }

    let mut outputs = Vec::with_capacity(handles.len());
    for handle in handles {
        outputs.push(handle.await.expect("concurrent request task"));
    }
    outputs
}

fn request_for_output(output: &ToolOutput) -> BuildRequest {
    let request_id = output
        .metadata
        .as_ref()
        .and_then(|metadata| metadata["request_id"].as_str())
        .expect("request id metadata");
    BuildRequest::load(request_id)
        .expect("load request")
        .expect("request exists")
}

fn assert_coalescing_metadata(
    output: &ToolOutput,
    request: &BuildRequest,
    leader_request_id: &str,
    forbidden_values: &[&str],
) {
    let metadata = output.metadata.as_ref().expect("coalescing metadata");
    let identity_version = metadata["identity_version"]
        .as_str()
        .expect("visible identity version");
    assert!(!identity_version.is_empty());
    assert!(
        identity_version.len() <= 32,
        "identity version must be bounded"
    );

    let expected_role = if request.attached_to_request_id.is_some() {
        "follower"
    } else {
        "leader"
    };
    assert_eq!(metadata["role"].as_str(), Some(expected_role));
    assert_eq!(
        metadata["coalesced"].as_bool(),
        Some(expected_role == "follower")
    );

    if expected_role == "follower" {
        assert_eq!(
            metadata["duplicate_of"]["request_id"].as_str(),
            Some(leader_request_id)
        );
    }

    let serialized = serde_json::to_string(metadata).expect("serialize metadata");
    for forbidden in forbidden_values {
        assert!(
            !serialized.contains(forbidden),
            "coalescing metadata exposed forbidden value: {forbidden}"
        );
    }
}

#[test]
fn delivery_metadata_update_preserves_terminal_follower_state() {
    let _storage_guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    let _home_guard = EnvVarGuard::set("JCODE_HOME", temp_home.path());
    let mut request = request_fixture(
        "completed-follower",
        BuildRequestState::Completed,
        chrono::Utc::now().to_rfc3339(),
    );
    request.completed_at = Some("terminal-time".to_string());
    request.error = Some("producer result".to_string());
    request.save().expect("save terminal follower");

    BuildRequest::save_delivery_metadata(
        &request.request_id,
        "watcher-task",
        "/tmp/watcher-output",
        "/tmp/watcher-status",
    )
    .expect("save watcher delivery metadata");

    let reloaded = BuildRequest::load(&request.request_id)
        .expect("load follower")
        .expect("follower exists");
    assert_eq!(reloaded.state, BuildRequestState::Completed);
    assert_eq!(reloaded.completed_at.as_deref(), Some("terminal-time"));
    assert_eq!(reloaded.error.as_deref(), Some("producer result"));
    assert_eq!(reloaded.background_task_id.as_deref(), Some("watcher-task"));
    assert_eq!(reloaded.output_file.as_deref(), Some("/tmp/watcher-output"));
    assert_eq!(reloaded.status_file.as_deref(), Some("/tmp/watcher-status"));
}

#[test]
fn test_reload_context_serialization() {
    // Create test context with task info
    let ctx = ReloadContext {
        task_context: Some("Testing the reload feature".to_string()),
        version_before: "v0.1.100".to_string(),
        version_after: "abc1234".to_string(),
        session_id: "test-session-123".to_string(),
        timestamp: "2025-01-20T00:00:00Z".to_string(),
    };

    // Serialize and deserialize
    let json = serde_json::to_string(&ctx).unwrap();
    let loaded: ReloadContext = serde_json::from_str(&json).unwrap();

    assert_eq!(
        loaded.task_context,
        Some("Testing the reload feature".to_string())
    );
    assert_eq!(loaded.version_before, "v0.1.100");
    assert_eq!(loaded.version_after, "abc1234");
    assert_eq!(loaded.session_id, "test-session-123");
}

#[test]
fn test_reload_context_path() {
    // Just verify the session-scoped path function works
    let path = ReloadContext::path_for_session("test-session-123");
    assert!(path.is_ok());
    let path = path.unwrap();
    let path_str = path.to_string_lossy();
    assert!(path_str.contains("reload-context-test-session-123.json"));
}

#[test]
fn test_reload_context_save_and_load_for_session_uses_session_scoped_file() {
    let _storage_guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    let _home_guard = EnvVarGuard::set("JCODE_HOME", temp_home.path());

    let ctx = ReloadContext {
        task_context: Some("Testing scoped reload context".to_string()),
        version_before: "v0.1.100".to_string(),
        version_after: "abc1234".to_string(),
        session_id: "test-session-123".to_string(),
        timestamp: "2025-01-20T00:00:00Z".to_string(),
    };

    ctx.save().expect("save reload context");

    let path = ReloadContext::path_for_session("test-session-123").expect("context path");
    assert!(
        path.exists(),
        "session-scoped reload context file should exist"
    );

    let peeked = ReloadContext::peek_for_session("test-session-123")
        .expect("peek should succeed")
        .expect("context should exist");
    assert_eq!(peeked.session_id, "test-session-123");

    let loaded = ReloadContext::load_for_session("test-session-123")
        .expect("load should succeed")
        .expect("context should exist");
    assert_eq!(loaded.session_id, "test-session-123");
    assert!(
        !path.exists(),
        "load_for_session should consume the context file"
    );
}

#[test]
fn test_recovery_directive_prefers_reload_context_when_present() {
    let ctx = ReloadContext {
        task_context: Some("Resume a self-dev reload".to_string()),
        version_before: "old-build".to_string(),
        version_after: "new-build".to_string(),
        session_id: "session-123".to_string(),
        timestamp: "2026-04-19T00:00:00Z".to_string(),
    };

    let directive = ReloadContext::recovery_directive(
        Some(&ctx),
        true,
        "\nPersisted background task(s) detected.",
        Some(12),
    )
    .expect("directive should exist");

    assert_eq!(
        directive.reconnect_notice.as_deref(),
        Some("Reloaded with build new-build")
    );
    assert!(directive.continuation_message.contains("Reload succeeded"));
    assert!(
        directive
            .continuation_message
            .contains("Persisted background task(s)")
    );
    assert!(
        directive
            .continuation_message
            .contains("Session restored with 12 turns")
    );
}

#[test]
fn test_recovery_directive_uses_interrupted_message_without_reload_context() {
    let directive = ReloadContext::recovery_directive(None, true, "", None)
        .expect("interrupted sessions should get a directive");

    assert!(directive.reconnect_notice.is_none());
    assert!(
        directive
            .continuation_message
            .contains("interrupted by a server reload while a tool was running")
    );
}

#[test]
fn test_recovery_directive_returns_none_when_no_reload_recovery_needed() {
    assert!(ReloadContext::recovery_directive(None, false, "", None).is_none());
}

#[test]
fn reload_timeout_secs_defaults_to_15() {
    let _storage_guard = crate::storage::lock_test_env();
    let _guard = EnvVarGuard::remove("JCODE_SELFDEV_RELOAD_TIMEOUT_SECS");
    assert_eq!(SelfDevTool::reload_timeout_secs(), 15);
}

#[test]
fn reload_timeout_secs_honors_valid_env_override() {
    let _storage_guard = crate::storage::lock_test_env();
    let _guard = EnvVarGuard::set("JCODE_SELFDEV_RELOAD_TIMEOUT_SECS", "27");
    assert_eq!(SelfDevTool::reload_timeout_secs(), 27);
}

#[test]
fn reload_timeout_secs_ignores_empty_invalid_and_zero_values() {
    let _storage_guard = crate::storage::lock_test_env();
    let _guard = EnvVarGuard::set("JCODE_SELFDEV_RELOAD_TIMEOUT_SECS", "   ");
    assert_eq!(SelfDevTool::reload_timeout_secs(), 15);
    drop(_guard);

    let _guard = EnvVarGuard::set("JCODE_SELFDEV_RELOAD_TIMEOUT_SECS", "abc");
    assert_eq!(SelfDevTool::reload_timeout_secs(), 15);
    drop(_guard);

    let _guard = EnvVarGuard::set("JCODE_SELFDEV_RELOAD_TIMEOUT_SECS", "0");
    assert_eq!(SelfDevTool::reload_timeout_secs(), 15);
}

#[test]
fn schema_only_advertises_core_selfdev_fields() {
    // The full (self-dev) schema exposes the build/test/reload surface.
    let schema = SelfDevTool::schema_for(true);
    let props = schema["properties"]
        .as_object()
        .expect("selfdev schema should have properties");

    assert!(props.contains_key("action"));
    assert!(props.contains_key("prompt"));
    assert!(props.contains_key("context"));
    assert!(props.contains_key("reason"));
    assert!(props.contains_key("target"));
    assert!(props.contains_key("command"));
    assert!(props.contains_key("request_id"));
    assert!(props.contains_key("task_id"));
    assert!(!props.contains_key("notify"));
    assert!(!props.contains_key("wake"));

    let actions: Vec<&str> = schema["properties"]["action"]["enum"]
        .as_array()
        .expect("action enum")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    for expected in [
        "enter",
        "setup",
        "build",
        "build-reload",
        "test",
        "cancel-build",
        "reload",
        "status",
        "find-config",
        "socket-info",
        "socket-help",
    ] {
        assert!(actions.contains(&expected), "missing action {expected}");
    }
}

#[test]
fn non_selfdev_schema_only_exposes_onramp_actions() {
    // The default schema (what a regular session advertises) is the on-ramp
    // surface: no build/test/socket actions, only enter/setup/reload/status/
    // find-config.
    let default_schema = SelfDevTool::new().parameters_schema();
    let onramp_schema = SelfDevTool::schema_for(false);
    assert_eq!(default_schema, onramp_schema);

    let props = onramp_schema["properties"]
        .as_object()
        .expect("schema properties");
    assert!(props.contains_key("action"));
    assert!(props.contains_key("prompt"));
    // Build/test-only fields are hidden outside self-dev mode.
    assert!(!props.contains_key("reason"));
    assert!(!props.contains_key("target"));
    assert!(!props.contains_key("command"));
    assert!(!props.contains_key("request_id"));
    assert!(!props.contains_key("task_id"));

    let actions: Vec<&str> = onramp_schema["properties"]["action"]["enum"]
        .as_array()
        .expect("action enum")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    let mut sorted = actions.clone();
    sorted.sort_unstable();
    assert_eq!(
        sorted,
        vec!["enter", "find-config", "reload", "setup", "status"]
    );
    for hidden in [
        "build",
        "build-reload",
        "test",
        "cancel-build",
        "socket-info",
        "socket-help",
    ] {
        assert!(
            !actions.contains(&hidden),
            "on-ramp schema should not expose {hidden}"
        );
    }
}

#[tokio::test]
async fn test_action_queues_command_in_test_mode() {
    let _storage_guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    let _home_guard = EnvVarGuard::set("JCODE_HOME", temp_home.path());
    let _test_guard = EnvVarGuard::set("JCODE_TEST_SESSION", "1");
    let repo = create_repo_fixture();

    let tool = SelfDevTool::new();
    let ctx = create_test_context(
        "session-selfdev-test-action",
        Some(repo.path().to_path_buf()),
    );
    let output = tool
        .execute(
            json!({
                "action": "test",
                "command": "cargo test -p jcode selfdev_build_command",
                "reason": "verify selfdev test queue"
            }),
            ctx,
        )
        .await
        .expect("selfdev test should queue");

    assert!(output.output.contains("Self-dev test queued"));
    assert!(
        output
            .output
            .contains("cargo test -p jcode selfdev_build_command")
    );
}

#[tokio::test]
async fn do_reload_returns_after_ack_in_direct_mode() {
    let request_id = server::send_reload_signal("direct-hash".to_string(), None, true);
    let waiter = tokio::spawn({
        let request_id = request_id.clone();
        async move { server::wait_for_reload_ack(&request_id, std::time::Duration::from_secs(1)).await }
    });

    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    server::acknowledge_reload_signal(&crate::server::ReloadSignal {
        hash: "direct-hash".to_string(),
        triggering_session: None,
        prefer_selfdev_binary: true,
        request_id: "ignored".to_string(),
    });

    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    server::acknowledge_reload_signal(&crate::server::ReloadSignal {
        hash: "direct-hash".to_string(),
        triggering_session: None,
        prefer_selfdev_binary: true,
        request_id,
    });

    let ack = waiter
        .await
        .expect("waiter task should complete")
        .expect("ack should be received");
    assert_eq!(ack.hash, "direct-hash");
}

#[test]
fn reload_repo_resolver_uses_working_dir_when_primary_detection_fails() {
    let repo = create_repo_fixture();
    let nested = repo.path().join("crates").join("jcode-build-support");
    std::fs::create_dir_all(&nested).expect("nested dir");

    let resolved = reload::resolve_selfdev_reload_repo_dir_from(None, Some(&nested));
    assert_eq!(resolved.as_deref(), Some(repo.path()));
}

#[tokio::test]
async fn enter_creates_selfdev_session_in_test_mode() {
    let _storage_guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    let _home_guard = EnvVarGuard::set("JCODE_HOME", temp_home.path());
    let _test_guard = EnvVarGuard::set("JCODE_TEST_SESSION", "1");
    let repo = create_repo_fixture();

    let mut parent = session::Session::create(None, Some("Origin Session".to_string()));
    parent.working_dir = Some("/tmp/origin-project".to_string());
    parent.model = Some("gpt-test".to_string());
    parent.provider_key = Some("openai".to_string());
    parent.subagent_model = Some("gpt-subagent".to_string());
    parent.add_message(
        crate::message::Role::User,
        vec![crate::message::ContentBlock::Text {
            text: "hello from parent".to_string(),
            cache_control: None,
        }],
    );
    parent.compaction = Some(session::StoredCompactionState {
        summary_text: "summary".to_string(),
        openai_encrypted_content: None,
        covers_up_to_turn: 1,
        original_turn_count: 1,
        compacted_count: 1,
    });
    parent.record_replay_display_message("system", None, "remember this context");
    parent.save().expect("save parent session");

    let tool = SelfDevTool::new();
    let ctx = create_test_context(&parent.id, Some(repo.path().to_path_buf()));
    let output = tool
        .execute(
            json!({"action": "enter", "prompt": "Work on jcode itself"}),
            ctx,
        )
        .await
        .expect("selfdev enter should succeed in test mode");

    assert!(output.output.contains("Created self-dev session"));
    assert!(
        output
            .output
            .contains("Test mode skipped launching a new terminal")
    );
    assert!(
        output.output.contains("Seed prompt captured"),
        "test-mode enter should still report captured prompt"
    );

    let metadata = output.metadata.expect("metadata");
    let session_id = metadata["session_id"]
        .as_str()
        .expect("session id metadata");
    assert_eq!(metadata["inherited_context"].as_bool(), Some(true));
    let session = session::Session::load(session_id).expect("load spawned session");
    assert!(
        session.is_canary,
        "spawned session should be canary/self-dev"
    );
    assert_eq!(session.testing_build.as_deref(), Some("self-dev"));
    assert_eq!(
        session.working_dir.as_deref(),
        Some(repo.path().to_string_lossy().as_ref())
    );
    assert_eq!(session.parent_id.as_deref(), Some(parent.id.as_str()));
    assert_eq!(session.messages.len(), parent.messages.len());
    assert_eq!(session.messages[0].content_preview(), "hello from parent");
    assert_eq!(session.compaction, parent.compaction);
    assert_eq!(session.model, parent.model);
    assert_eq!(session.provider_key, parent.provider_key);
    assert_eq!(session.subagent_model, parent.subagent_model);
    assert_eq!(session.replay_events, parent.replay_events);
}

#[tokio::test]
async fn enter_falls_back_to_fresh_session_when_parent_missing() {
    let _storage_guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    let _home_guard = EnvVarGuard::set("JCODE_HOME", temp_home.path());
    let _test_guard = EnvVarGuard::set("JCODE_TEST_SESSION", "1");
    let repo = create_repo_fixture();

    let tool = SelfDevTool::new();
    let ctx = create_test_context("missing-parent", Some(repo.path().to_path_buf()));
    let output = tool
        .execute(json!({"action": "enter"}), ctx)
        .await
        .expect("selfdev enter should succeed without a persisted parent session");

    let metadata = output.metadata.expect("metadata");
    let session_id = metadata["session_id"]
        .as_str()
        .expect("session id metadata");
    assert_eq!(metadata["inherited_context"].as_bool(), Some(false));

    let session = session::Session::load(session_id).expect("load spawned session");
    assert!(session.messages.is_empty());
    assert!(session.parent_id.is_none());
    assert_eq!(
        session.working_dir.as_deref(),
        Some(repo.path().to_string_lossy().as_ref())
    );
}

#[tokio::test]
async fn reload_in_non_selfdev_session_is_upgrade_in_place() {
    let _storage_guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    let _home_guard = EnvVarGuard::set("JCODE_HOME", temp_home.path());
    // Test mode short-circuits the actual server reload signal.
    let _test_guard = EnvVarGuard::set("JCODE_TEST_SESSION", "1");

    let mut session = session::Session::create(None, Some("Normal Session".to_string()));
    session.save().expect("save session");

    let tool = SelfDevTool::new();
    let ctx = create_test_context(&session.id, session.working_dir.clone().map(Into::into));
    let output = tool
        .execute(json!({"action": "reload"}), ctx)
        .await
        .expect("reload should route to upgrade-in-place");

    // It must NOT be the old "only available inside a self-dev session" error;
    // a regular session can reload into a newer installed build.
    assert!(
        !output
            .output
            .contains("only available inside a self-dev session")
    );
    assert!(output.output.contains("Test mode"));
}

#[tokio::test]
async fn socket_actions_require_selfdev_session() {
    let _storage_guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    let _home_guard = EnvVarGuard::set("JCODE_HOME", temp_home.path());

    let mut session = session::Session::create(None, Some("Normal Session".to_string()));
    session.save().expect("save session");

    let tool = SelfDevTool::new();
    for action in ["socket-info", "socket-help"] {
        let ctx = create_test_context(&session.id, session.working_dir.clone().map(Into::into));
        let output = tool
            .execute(json!({"action": action}), ctx)
            .await
            .expect("socket action should return guidance instead of failing");
        assert!(
            output
                .output
                .contains("only available inside a self-dev session"),
            "{action} should be gated"
        );
        assert!(output.output.contains("selfdev enter"));
    }
}

#[tokio::test]
async fn find_config_reports_key_paths() {
    let _storage_guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    let _home_guard = EnvVarGuard::set("JCODE_HOME", temp_home.path());

    let mut session = session::Session::create(None, Some("Normal Session".to_string()));
    session.save().expect("save session");

    let tool = SelfDevTool::new();
    let ctx = create_test_context(&session.id, None);
    let output = tool
        .execute(json!({"action": "find-config"}), ctx)
        .await
        .expect("find-config should succeed");

    assert!(output.output.contains("Config file:"));
    assert!(output.output.contains("config.toml"));
    assert!(output.output.contains("Build channels"));
    let metadata = output.metadata.expect("find-config metadata");
    assert!(metadata["config_path"].as_str().is_some());
}

#[tokio::test]
async fn setup_reports_dependency_checks() {
    let _storage_guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    let _home_guard = EnvVarGuard::set("JCODE_HOME", temp_home.path());
    // Test mode avoids attempting a real git clone when no repo is detected.
    let _test_guard = EnvVarGuard::set("JCODE_TEST_SESSION", "1");
    let repo = create_repo_fixture();

    let mut session = session::Session::create(None, Some("Normal Session".to_string()));
    session.save().expect("save session");

    let tool = SelfDevTool::new();
    let ctx = create_test_context(&session.id, Some(repo.path().to_path_buf()));
    let output = tool
        .execute(json!({"action": "setup"}), ctx)
        .await
        .expect("setup should succeed");

    assert!(output.output.contains("Self-dev setup"));
    assert!(output.output.contains("**cargo**") || output.output.contains("cargo"));
    assert!(output.output.contains("repository"));
    let metadata = output.metadata.expect("setup metadata");
    assert!(metadata["checks"].as_array().is_some());
    // The fixture repo should be detected as the repository.
    assert_eq!(
        metadata["repo_dir"].as_str(),
        Some(repo.path().to_string_lossy().as_ref())
    );
}

#[tokio::test]
async fn build_requires_reason() {
    let _storage_guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    let _home_guard = EnvVarGuard::set("JCODE_HOME", temp_home.path());
    let _test_guard = EnvVarGuard::set("JCODE_TEST_SESSION", "1");
    let repo = create_repo_fixture();

    let tool = SelfDevTool::new();
    let ctx = create_test_context("build-session", Some(repo.path().to_path_buf()));
    let err = tool
        .execute(json!({"action": "build"}), ctx)
        .await
        .expect_err("build without reason should fail");

    assert!(err.to_string().contains("requires a non-empty `reason`"));
}

#[tokio::test]
async fn build_queues_background_tasks_and_reports_queue_status() {
    let _storage_guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    let _home_guard = EnvVarGuard::set("JCODE_HOME", temp_home.path());
    let _test_guard = EnvVarGuard::set("JCODE_TEST_SESSION", "1");
    let repo = create_repo_fixture();

    let mut session_one = session::Session::create(None, Some("First build session".to_string()));
    session_one.short_name = Some("alpha".to_string());
    session_one.save().expect("save session one");

    let mut session_two = session::Session::create(None, Some("Second build session".to_string()));
    session_two.short_name = Some("beta".to_string());
    session_two.save().expect("save session two");

    let tool = SelfDevTool::new();
    let first = tool
        .execute(
            json!({"action": "build", "reason": "first reason"}),
            create_test_context(&session_one.id, Some(repo.path().to_path_buf())),
        )
        .await
        .expect("first build should queue");
    let second = tool
        .execute(
            json!({"action": "build", "reason": "second reason"}),
            create_test_context(&session_two.id, Some(repo.path().to_path_buf())),
        )
        .await
        .expect("second build should queue");

    let first_meta = first.metadata.expect("first metadata");
    let second_meta = second.metadata.expect("second metadata");
    let first_task_id = first_meta["task_id"].as_str().expect("first task id");
    let second_task_id = second_meta["task_id"].as_str().expect("second task id");

    assert_eq!(first_meta["queue_position"].as_u64(), Some(1));
    assert_eq!(second_meta["deduped"].as_bool(), Some(true));
    assert!(
        second
            .output
            .contains("attached instead of spawning a duplicate build")
    );

    let status_output = selfdev_status_output().expect("status output");
    assert!(status_output.output.contains("## Build Queue"));
    assert!(status_output.output.contains("first reason"));
    assert!(status_output.output.contains("Attached watchers: 1"));
    assert!(
        status_output
            .output
            .contains("Target version: `test-build`")
    );

    let first_status = wait_for_task_completion(first_task_id).await;
    let second_status = wait_for_task_completion(second_task_id).await;
    assert_eq!(first_status.status, BackgroundTaskStatus::Completed);
    assert_eq!(second_status.status, BackgroundTaskStatus::Completed);

    let request_one =
        BuildRequest::load(first_meta["request_id"].as_str().expect("first request id"))
            .expect("load request one")
            .expect("request one exists");
    let request_two = BuildRequest::load(
        second_meta["request_id"]
            .as_str()
            .expect("second request id"),
    )
    .expect("load request two")
    .expect("request two exists");
    assert_eq!(request_one.state, BuildRequestState::Completed);
    assert_eq!(request_two.state, BuildRequestState::Completed);
}

#[tokio::test]
async fn build_reload_waits_for_build_then_reloads() {
    let _storage_guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    let _home_guard = EnvVarGuard::set("JCODE_HOME", temp_home.path());
    let _test_guard = EnvVarGuard::set("JCODE_TEST_SESSION", "1");
    let repo = create_repo_fixture();

    let mut session = session::Session::create(None, Some("Build+reload session".to_string()));
    session.is_canary = true;
    session.short_name = Some("gamma".to_string());
    session.save().expect("save session");

    // The reload phase blocks on a server ack. Spawn a watcher that mirrors the
    // server: it observes reload signals and acknowledges them so the inline
    // reload can complete deterministically in test mode. It keeps acking every
    // signal it sees (the RELOAD_SIGNAL channel is a process-global shared by
    // parallel tests, and `wait_for_reload_ack` matches by request id, so acking
    // unrelated/stale signals is harmless).
    let mut signal_rx = server::subscribe_reload_signal_for_tests();
    let acker = tokio::spawn(async move {
        if let Some(signal) = signal_rx.borrow_and_update().clone() {
            server::acknowledge_reload_signal(&signal);
        }
        while signal_rx.changed().await.is_ok() {
            if let Some(signal) = signal_rx.borrow_and_update().clone() {
                server::acknowledge_reload_signal(&signal);
            }
        }
    });

    let tool = SelfDevTool::new();
    let output = tool
        .execute(
            json!({"action": "build-reload", "reason": "combined build and reload"}),
            create_test_context(&session.id, Some(repo.path().to_path_buf())),
        )
        .await
        .expect("build-reload should succeed");

    acker.abort();

    assert!(
        output.output.contains("Build completed successfully"),
        "unexpected output: {}",
        output.output
    );
    let meta = output.metadata.expect("build-reload metadata");
    assert_eq!(meta["phase"].as_str(), Some("reload"));
    assert_eq!(meta["build_finished"].as_bool(), Some(true));
    assert_eq!(meta["build_succeeded"].as_bool(), Some(true));

    let request_id = meta["request_id"].as_str().expect("request id in metadata");
    let request = BuildRequest::load(request_id)
        .expect("load request")
        .expect("request exists");
    assert_eq!(request.state, BuildRequestState::Completed);
}

#[tokio::test]
async fn build_dedupes_identical_reason_and_version_with_attached_watcher() {
    let _storage_guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    let _home_guard = EnvVarGuard::set("JCODE_HOME", temp_home.path());
    let _test_guard = EnvVarGuard::set("JCODE_TEST_SESSION", "1");
    let repo = create_repo_fixture();

    let mut session_one = session::Session::create(None, Some("Build A".to_string()));
    session_one.short_name = Some("alpha".to_string());
    session_one.save().expect("save session one");

    let mut session_two = session::Session::create(None, Some("Build B".to_string()));
    session_two.short_name = Some("beta".to_string());
    session_two.save().expect("save session two");

    let tool = SelfDevTool::new();
    let first = tool
        .execute(
            json!({"action": "build", "reason": "same reason"}),
            create_test_context(&session_one.id, Some(repo.path().to_path_buf())),
        )
        .await
        .expect("first build should queue");
    let second = tool
        .execute(
            json!({"action": "build", "reason": "same reason"}),
            create_test_context(&session_two.id, Some(repo.path().to_path_buf())),
        )
        .await
        .expect("second build should attach");

    let first_meta = first.metadata.expect("first metadata");
    let second_meta = second.metadata.expect("second metadata");
    assert_eq!(second_meta["deduped"].as_bool(), Some(true));
    assert_eq!(
        second_meta["duplicate_of"]["request_id"].as_str(),
        first_meta["request_id"].as_str()
    );

    let status_output = selfdev_status_output().expect("status output");
    assert!(status_output.output.contains("Attached watchers: 1"));
    assert!(status_output.output.contains("alpha"));
    assert!(status_output.output.contains("beta"));

    let first_status = wait_for_task_completion(first_meta["task_id"].as_str().unwrap()).await;
    let second_status = wait_for_task_completion(second_meta["task_id"].as_str().unwrap()).await;
    assert_eq!(first_status.status, BackgroundTaskStatus::Completed);
    assert_eq!(second_status.status, BackgroundTaskStatus::Completed);

    let watcher_request = BuildRequest::load(second_meta["request_id"].as_str().unwrap())
        .expect("load watcher request")
        .expect("watcher request exists");
    assert_eq!(watcher_request.state, BuildRequestState::Completed);
    assert_eq!(
        watcher_request.attached_to_request_id.as_deref(),
        first_meta["request_id"].as_str()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn atomic_claim_selects_exactly_one_leader() {
    let _storage_guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    let _home_guard = EnvVarGuard::set("JCODE_HOME", temp_home.path());
    let _test_guard = EnvVarGuard::set("JCODE_TEST_SESSION", "1");
    let repo = create_repo_fixture();
    std::fs::write(
        repo.path().join("private-source.txt"),
        "RAW-SOURCE-SENTINEL",
    )
    .expect("private source fixture");

    let inputs = (0..24)
        .map(|_| json!({"action": "build", "reason": "atomic claim test"}))
        .collect();
    let outputs = execute_concurrent_selfdev_requests(repo.path(), inputs).await;
    let requests = outputs.iter().map(request_for_output).collect::<Vec<_>>();
    let leaders = requests
        .iter()
        .filter(|request| request.attached_to_request_id.is_none())
        .collect::<Vec<_>>();
    let followers = requests
        .iter()
        .filter(|request| request.attached_to_request_id.is_some())
        .count();

    assert_eq!(
        leaders.len(),
        1,
        "concurrent find-then-save admitted multiple leaders: {:?}",
        leaders
            .iter()
            .map(|request| request.request_id.as_str())
            .collect::<Vec<_>>()
    );
    assert_eq!(followers, outputs.len() - 1);

    let leader_request_id = leaders[0].request_id.clone();
    for (output, request) in outputs.iter().zip(&requests) {
        assert_coalescing_metadata(
            output,
            request,
            &leader_request_id,
            &["RAW-SOURCE-SENTINEL", &repo.path().display().to_string()],
        );
        let task_id = output
            .metadata
            .as_ref()
            .and_then(|metadata| metadata["task_id"].as_str())
            .expect("task id metadata");
        let status = wait_for_task_completion(task_id).await;
        assert_eq!(status.status, BackgroundTaskStatus::Completed);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn exact_eligible_cargo_requests_attach_and_propagate_terminal_result() {
    let _storage_guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    let _home_guard = EnvVarGuard::set("JCODE_HOME", temp_home.path());
    let _test_guard = EnvVarGuard::set("JCODE_TEST_SESSION", "1");
    let repo = create_repo_fixture();
    std::fs::write(
        repo.path().join("private-source.txt"),
        "RAW-SOURCE-SENTINEL",
    )
    .expect("private source fixture");

    for command in [
        "cargo build -p jcode",
        "cargo test -p jcode --lib",
        "cargo check -p jcode --all-targets",
    ] {
        let inputs = (0..8)
            .map(|_| {
                json!({
                    "action": "test",
                    "command": command,
                    "reason": "exact eligible cargo request"
                })
            })
            .collect();
        let outputs = execute_concurrent_selfdev_requests(repo.path(), inputs).await;
        let requests = outputs.iter().map(request_for_output).collect::<Vec<_>>();
        let leaders = requests
            .iter()
            .filter(|request| request.attached_to_request_id.is_none())
            .collect::<Vec<_>>();

        assert_eq!(
            leaders.len(),
            1,
            "{command} should have exactly one producer"
        );
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.attached_to_request_id.is_some())
                .count(),
            outputs.len() - 1,
            "{command} should attach every duplicate session"
        );

        let leader_request_id = leaders[0].request_id.clone();
        let leader_error = leaders[0].error.clone();
        for (output, request) in outputs.iter().zip(&requests) {
            assert_coalescing_metadata(
                output,
                request,
                &leader_request_id,
                &["RAW-SOURCE-SENTINEL", &repo.path().display().to_string()],
            );
            let task_id = output
                .metadata
                .as_ref()
                .and_then(|metadata| metadata["task_id"].as_str())
                .expect("task id metadata");
            let status = wait_for_task_completion(task_id).await;
            assert_eq!(
                status.status,
                BackgroundTaskStatus::Completed,
                "{command} follower did not receive the producer terminal result"
            );
        }

        for request in requests {
            let terminal = BuildRequest::load(&request.request_id)
                .expect("reload terminal request")
                .expect("terminal request exists");
            assert_eq!(terminal.state, BuildRequestState::Completed);
            assert_eq!(terminal.error, leader_error);
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn eligible_cargo_near_misses_remain_independent() {
    let _storage_guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    let _home_guard = EnvVarGuard::set("JCODE_HOME", temp_home.path());
    let _test_guard = EnvVarGuard::set("JCODE_TEST_SESSION", "1");
    let repo = create_repo_fixture();
    let lock = SelfDevTool::try_acquire_build_lock("test-worktree-scope")
        .expect("lock attempt")
        .expect("hold queue lock");

    let commands = [
        "cargo build -p jcode",
        "cargo build -p jcode --profile selfdev",
        "cargo build -p jcode --features desktop",
        "cargo build -p jcode-app-core",
        "cargo build -p jcode --bin jcode",
        "cargo build -p jcode --lib",
        "cargo test -p jcode",
        "cargo check -p jcode",
    ];
    let outputs = execute_concurrent_selfdev_requests(
        repo.path(),
        commands
            .iter()
            .map(|command| {
                json!({
                    "action": "test",
                    "command": command,
                    "reason": "eligible near miss"
                })
            })
            .chain(std::iter::once(json!({
                "action": "build",
                "reason": "different public action"
            })))
            .collect(),
    )
    .await;
    let requests = outputs.iter().map(request_for_output).collect::<Vec<_>>();

    assert!(
        requests
            .iter()
            .all(|request| request.attached_to_request_id.is_none()),
        "action, profile, features, package, target, and arguments are exact identity dimensions: {:?}",
        requests
            .iter()
            .map(|request| (&request.command, &request.attached_to_request_id))
            .collect::<Vec<_>>()
    );
    for request in &requests {
        let source = request
            .requested_source
            .as_ref()
            .expect("request source identity");
        let key = request
            .dedupe_key
            .as_deref()
            .expect("eligible request should persist a dedupe key");
        assert!(key.starts_with("selfdev-cargo-v1:"));
        assert!(key.contains(&source.worktree_scope));
        assert!(key.contains(&source.fingerprint));
        assert!(key.ends_with(&request.command));
    }

    drop(lock);
    for output in &outputs {
        let task_id = output
            .metadata
            .as_ref()
            .and_then(|metadata| metadata["task_id"].as_str())
            .expect("task id metadata");
        assert_eq!(
            wait_for_task_completion(task_id).await.status,
            BackgroundTaskStatus::Completed
        );
    }
}

#[tokio::test]
async fn different_source_fingerprints_do_not_attach() {
    let _storage_guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    let _home_guard = EnvVarGuard::set("JCODE_HOME", temp_home.path());
    let _test_guard = EnvVarGuard::set("JCODE_TEST_SESSION", "1");
    let repo = create_repo_fixture();
    let lock = SelfDevTool::try_acquire_build_lock("test-worktree-scope")
        .expect("lock attempt")
        .expect("hold queue lock");
    let tool = SelfDevTool::new();

    let first = tool
        .execute(
            json!({"action": "build", "reason": "source fingerprint A"}),
            create_test_context("source-a", Some(repo.path().to_path_buf())),
        )
        .await
        .expect("first build should queue");
    let mut first_request = request_for_output(&first);
    first_request
        .requested_source
        .as_mut()
        .expect("requested source")
        .fingerprint = "different-source-fingerprint".to_string();
    let changed_source = first_request
        .requested_source
        .as_ref()
        .expect("changed requested source");
    first_request.dedupe_key = Some(SelfDevTool::build_dedupe_key(
        changed_source,
        &SelfDevBuildCommand {
            program: String::new(),
            args: Vec::new(),
            display: first_request.command.clone(),
        },
    ));
    first_request
        .save()
        .expect("persist changed source identity");

    let second = tool
        .execute(
            json!({"action": "build", "reason": "source fingerprint B"}),
            create_test_context("source-b", Some(repo.path().to_path_buf())),
        )
        .await
        .expect("second build should queue independently");
    let second_request = request_for_output(&second);
    assert!(second_request.attached_to_request_id.is_none());
    assert_ne!(
        first_request
            .requested_source
            .as_ref()
            .map(|source| source.fingerprint.as_str()),
        second_request
            .requested_source
            .as_ref()
            .map(|source| source.fingerprint.as_str())
    );

    drop(lock);
    for output in [&first, &second] {
        let task_id = output.metadata.as_ref().unwrap()["task_id"]
            .as_str()
            .expect("task id");
        let _ = wait_for_task_completion(task_id).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn opaque_shell_commands_remain_independent_and_keep_exact_semantics() {
    let _storage_guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    let _home_guard = EnvVarGuard::set("JCODE_HOME", temp_home.path());
    let _test_guard = EnvVarGuard::set("JCODE_TEST_SESSION", "1");
    let repo = create_repo_fixture();
    let lock = SelfDevTool::try_acquire_build_lock("test-worktree-scope")
        .expect("lock attempt")
        .expect("hold queue lock");
    let commands = [
        "cargo test -p jcode && printf done",
        "cargo test -p jcode > test.log",
        "cargo test -p jcode | tee test.log",
        "RUSTFLAGS=-Dwarnings cargo test -p jcode",
        "'cargo' test -p jcode",
        "cargo test -p jcode; cargo check -p jcode",
        "cargo clippy -p jcode",
        "cargo bench -p jcode",
    ];
    let outputs = execute_concurrent_selfdev_requests(
        repo.path(),
        commands
            .iter()
            .map(|command| {
                json!({
                    "action": "test",
                    "command": command,
                    "reason": "opaque command compatibility"
                })
            })
            .collect(),
    )
    .await;
    let requests = outputs.iter().map(request_for_output).collect::<Vec<_>>();

    for (request, expected_command) in requests.iter().zip(commands) {
        assert!(request.attached_to_request_id.is_none());
        assert_eq!(request.command, expected_command);
        assert!(
            request.dedupe_key.is_none(),
            "opaque commands must persist no reusable identity"
        );
    }

    drop(lock);
    for output in &outputs {
        let task_id = output.metadata.as_ref().unwrap()["task_id"]
            .as_str()
            .expect("task id");
        assert_eq!(
            wait_for_task_completion(task_id).await.status,
            BackgroundTaskStatus::Completed
        );
    }
}

#[tokio::test]
async fn dirty_source_drift_supersedes_eligible_test_before_launch() {
    let _storage_guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    let _home_guard = EnvVarGuard::set("JCODE_HOME", temp_home.path());
    let _test_guard = EnvVarGuard::set("JCODE_TEST_SESSION", "1");
    let repo = create_repo_fixture();
    let lock = SelfDevTool::try_acquire_build_lock("test-worktree-scope")
        .expect("lock attempt")
        .expect("hold queue lock");
    let mut unrelated = request_fixture(
        "unrelated-active-work",
        BuildRequestState::Building,
        Utc::now().to_rfc3339(),
    );
    unrelated.worktree_scope = "unrelated-worktree-scope".to_string();
    unrelated.save().expect("save unrelated active work");

    let output = SelfDevTool::new()
        .execute(
            json!({
                "action": "test",
                "command": "cargo test -p jcode --lib",
                "reason": "queued source drift"
            }),
            create_test_context("dirty-drift", Some(repo.path().to_path_buf())),
        )
        .await
        .expect("eligible test should queue");
    let mut request = request_for_output(&output);
    request
        .requested_source
        .as_mut()
        .expect("requested source")
        .fingerprint = "obsolete-dirty-fingerprint".to_string();
    request.save().expect("persist drifted requested source");
    drop(lock);

    let task_id = output.metadata.as_ref().unwrap()["task_id"]
        .as_str()
        .expect("task id");
    let status = wait_for_task_completion(task_id).await;
    assert_eq!(status.status, BackgroundTaskStatus::Superseded);
    let terminal = BuildRequest::load(&request.request_id)
        .expect("reload request")
        .expect("request exists");
    assert_eq!(terminal.state, BuildRequestState::Superseded);
    let command_output = std::fs::read_to_string(
        terminal
            .output_file
            .as_ref()
            .expect("output file for request"),
    )
    .expect("read request output");
    assert!(
        !command_output.contains("Simulated selfdev test"),
        "superseded validation must not launch its command"
    );
    assert_eq!(
        BuildRequest::load(&unrelated.request_id)
            .expect("reload unrelated work")
            .expect("unrelated work exists")
            .state,
        BuildRequestState::Building,
        "superseding stale work must not interrupt unrelated active work"
    );
}

#[tokio::test]
async fn cancelling_attached_follower_does_not_cancel_producer() {
    let _storage_guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    let _home_guard = EnvVarGuard::set("JCODE_HOME", temp_home.path());
    let _test_guard = EnvVarGuard::set("JCODE_TEST_SESSION", "1");
    let repo = create_repo_fixture();
    let tool = SelfDevTool::new();

    let leader = tool
        .execute(
            json!({"action": "build", "reason": "shared producer"}),
            create_test_context("producer-session", Some(repo.path().to_path_buf())),
        )
        .await
        .expect("leader should queue");
    let follower = tool
        .execute(
            json!({"action": "build", "reason": "shared follower"}),
            create_test_context("follower-session", Some(repo.path().to_path_buf())),
        )
        .await
        .expect("follower should attach");
    let follower_request = request_for_output(&follower);
    assert!(follower_request.attached_to_request_id.is_some());

    tool.execute(
        json!({"action": "cancel-build", "request_id": follower_request.request_id}),
        create_test_context("follower-session", Some(repo.path().to_path_buf())),
    )
    .await
    .expect("follower cancellation should succeed");

    let leader_task_id = leader.metadata.as_ref().unwrap()["task_id"]
        .as_str()
        .expect("leader task id");
    assert_eq!(
        wait_for_task_completion(leader_task_id).await.status,
        BackgroundTaskStatus::Completed
    );
    assert_eq!(
        request_for_output(&leader).state,
        BuildRequestState::Completed
    );
    assert_eq!(
        BuildRequest::load(&follower_request.request_id)
            .expect("reload follower")
            .expect("follower exists")
            .state,
        BuildRequestState::Cancelled
    );
}

#[tokio::test]
async fn attached_follower_receives_producer_failure() {
    let _storage_guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    let _home_guard = EnvVarGuard::set("JCODE_HOME", temp_home.path());
    let _test_guard = EnvVarGuard::set("JCODE_TEST_SESSION", "1");
    let repo = create_repo_fixture();
    let lock = SelfDevTool::try_acquire_build_lock("test-worktree-scope")
        .expect("lock attempt")
        .expect("hold queue lock");
    let tool = SelfDevTool::new();
    let leader = tool
        .execute(
            json!({"action": "build", "reason": "producer failure"}),
            create_test_context("failed-producer", Some(repo.path().to_path_buf())),
        )
        .await
        .expect("producer should queue");
    let follower = tool
        .execute(
            json!({"action": "build", "reason": "failure follower"}),
            create_test_context("failure-follower", Some(repo.path().to_path_buf())),
        )
        .await
        .expect("follower should attach");
    let mut producer = request_for_output(&leader);
    let follower_request = request_for_output(&follower);
    assert_eq!(
        follower_request.attached_to_request_id.as_deref(),
        Some(producer.request_id.as_str())
    );

    producer.state = BuildRequestState::Failed;
    producer.error = Some("producer failed sentinel".to_string());
    producer.completed_at = Some(Utc::now().to_rfc3339());
    producer.save().expect("persist producer failure");

    let follower_task_id = follower.metadata.as_ref().unwrap()["task_id"]
        .as_str()
        .expect("follower task id");
    let result = wait_for_task_completion(follower_task_id).await;
    assert_eq!(result.status, BackgroundTaskStatus::Failed);
    assert_eq!(result.error.as_deref(), Some("producer failed sentinel"));
    let terminal = BuildRequest::load(&follower_request.request_id)
        .expect("reload follower")
        .expect("follower exists");
    assert_eq!(terminal.state, BuildRequestState::Failed);
    assert_eq!(terminal.error.as_deref(), Some("producer failed sentinel"));

    let leader_task_id = leader.metadata.as_ref().unwrap()["task_id"]
        .as_str()
        .expect("leader task id");
    let _ = background::global().cancel(leader_task_id).await;
    drop(lock);
}

#[test]
fn stale_persisted_producer_is_not_reused() {
    let _storage_guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    let _home_guard = EnvVarGuard::set("JCODE_HOME", temp_home.path());
    let mut stale = request_fixture(
        "stale-owner",
        BuildRequestState::Building,
        (Utc::now() - chrono::Duration::minutes(2)).to_rfc3339(),
    );
    stale.background_task_id = Some("missing-background-task".to_string());
    stale.dedupe_key = Some("stale-dedupe-key".to_string());
    stale.save().expect("save stale producer");

    assert!(
        BuildRequest::find_duplicate_pending(&stale.worktree_scope, "stale-dedupe-key")
            .expect("reconcile duplicate lookup")
            .is_none()
    );
    let reconciled = BuildRequest::load(&stale.request_id)
        .expect("reload stale producer")
        .expect("stale producer exists");
    assert_eq!(reconciled.state, BuildRequestState::Failed);
    assert!(
        reconciled
            .error
            .as_deref()
            .is_some_and(|error| error.contains("status file is missing"))
    );
}

#[tokio::test]
async fn cancel_build_marks_request_cancelled_and_removes_it_from_queue() {
    let _storage_guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    let _home_guard = EnvVarGuard::set("JCODE_HOME", temp_home.path());
    let _test_guard = EnvVarGuard::set("JCODE_TEST_SESSION", "1");
    let repo = create_repo_fixture();

    let mut session_one = session::Session::create(None, Some("Build A".to_string()));
    session_one.short_name = Some("alpha".to_string());
    session_one.save().expect("save session one");

    let mut session_two = session::Session::create(None, Some("Build B".to_string()));
    session_two.short_name = Some("beta".to_string());
    session_two.save().expect("save session two");

    let tool = SelfDevTool::new();
    let first = tool
        .execute(
            json!({"action": "build", "reason": "keep building"}),
            create_test_context(&session_one.id, Some(repo.path().to_path_buf())),
        )
        .await
        .expect("first build should queue");
    let second = tool
        .execute(
            json!({"action": "build", "reason": "cancel me"}),
            create_test_context(&session_two.id, Some(repo.path().to_path_buf())),
        )
        .await
        .expect("second build should queue");

    let second_meta = second.metadata.expect("second metadata");
    let cancel = tool
        .execute(
            json!({
                "action": "cancel-build",
                "request_id": second_meta["request_id"].as_str().unwrap()
            }),
            create_test_context(&session_two.id, Some(repo.path().to_path_buf())),
        )
        .await
        .expect("cancel should succeed");

    assert!(cancel.output.contains("Cancelled self-dev build request"));

    let second_status = wait_for_task_completion(second_meta["task_id"].as_str().unwrap()).await;
    assert_eq!(second_status.status, BackgroundTaskStatus::Failed);

    let cancelled_request = BuildRequest::load(second_meta["request_id"].as_str().unwrap())
        .expect("load cancelled request")
        .expect("cancelled request exists");
    assert_eq!(cancelled_request.state, BuildRequestState::Cancelled);

    let status_output = selfdev_status_output().expect("status output");
    assert!(status_output.output.contains("keep building"));
    assert!(!status_output.output.contains("cancel me"));

    let first_meta = first.metadata.expect("first metadata");
    let first_status = wait_for_task_completion(first_meta["task_id"].as_str().unwrap()).await;
    assert_eq!(first_status.status, BackgroundTaskStatus::Completed);
}

#[test]
fn status_output_prunes_stale_pending_requests() {
    let _storage_guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    let _home_guard = EnvVarGuard::set("JCODE_HOME", temp_home.path());

    let mut session = session::Session::create(None, Some("Stale Build".to_string()));
    session.short_name = Some("ghost".to_string());
    session.save().expect("save session");

    let stale_status_path = temp_home.path().join("missing-selfdev.status.json");
    let source = test_source_state(std::path::Path::new("/tmp/jcode"));
    let request = BuildRequest {
        request_id: "stale-request".to_string(),
        background_task_id: Some("missing-task".to_string()),
        session_id: session.id.clone(),
        session_short_name: session.short_name.clone(),
        session_title: Some("Stale Build".to_string()),
        reason: "stale reason".to_string(),
        repo_dir: "/tmp/jcode".to_string(),
        repo_scope: source.repo_scope.clone(),
        worktree_scope: source.worktree_scope.clone(),
        command: "scripts/dev_cargo.sh build --profile selfdev -p jcode --bin jcode".to_string(),
        // Outside the bootstrap grace window: a request with a missing status
        // file is only pruned once it is old enough that the queue handler
        // cannot still be mid-spawn.
        requested_at: (Utc::now() - chrono::Duration::minutes(10)).to_rfc3339(),
        started_at: Some(Utc::now().to_rfc3339()),
        completed_at: None,
        state: BuildRequestState::Building,
        version: Some("stale-build".to_string()),
        dedupe_key: Some("stale-dedupe".to_string()),
        requested_source: Some(source),
        built_source: None,
        published_version: None,
        last_progress: Some("building".to_string()),
        validated: false,
        error: None,
        output_file: None,
        status_file: Some(stale_status_path.display().to_string()),
        attached_to_request_id: None,
    };
    request.save().expect("save stale request");

    let status_output = selfdev_status_output().expect("status output");
    assert!(
        !status_output.output.contains("stale reason"),
        "stale request should be pruned from queue output"
    );

    let request = BuildRequest::load("stale-request")
        .expect("load stale request")
        .expect("stale request exists");
    assert_eq!(request.state, BuildRequestState::Failed);
    assert!(
        request
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("pruning stale self-dev build request"),
        "stale request should record why it was pruned"
    );
}

#[test]
fn freshly_queued_request_survives_reconcile_before_task_metadata_exists() {
    // Regression: the queue handler saves the request *before* spawning its
    // background task, so for a moment it has no task id / status file. A
    // concurrent reconcile (status output, another agent's queue poll, or the
    // task's own first wait_for_turn iteration) used to prune it as stale,
    // killing the build instantly with "Queued build request disappeared".
    let _storage_guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    let _home_guard = EnvVarGuard::set("JCODE_HOME", temp_home.path());

    let mut session = session::Session::create(None, Some("Fresh Build".to_string()));
    session.save().expect("save session");

    let source = test_source_state(std::path::Path::new("/tmp/jcode"));
    let request = BuildRequest {
        request_id: "fresh-request".to_string(),
        // No background task metadata yet: mid-bootstrap.
        background_task_id: None,
        session_id: session.id.clone(),
        session_short_name: session.short_name.clone(),
        session_title: Some("Fresh Build".to_string()),
        reason: "fresh reason".to_string(),
        repo_dir: "/tmp/jcode".to_string(),
        repo_scope: source.repo_scope.clone(),
        worktree_scope: source.worktree_scope.clone(),
        command: "scripts/dev_cargo.sh build --profile selfdev -p jcode --bin jcode".to_string(),
        requested_at: Utc::now().to_rfc3339(),
        started_at: None,
        completed_at: None,
        state: BuildRequestState::Queued,
        version: Some("fresh-build".to_string()),
        dedupe_key: Some("fresh-dedupe".to_string()),
        requested_source: Some(source.clone()),
        built_source: None,
        published_version: None,
        last_progress: Some("queued".to_string()),
        validated: false,
        error: None,
        output_file: None,
        status_file: None,
        attached_to_request_id: None,
    };
    request.save().expect("save fresh request");

    let pending =
        BuildRequest::pending_requests_for_scope(&source.worktree_scope).expect("pending requests");
    assert!(
        pending
            .iter()
            .any(|request| request.request_id == "fresh-request"),
        "freshly queued request must stay pending during the bootstrap grace window"
    );

    let reloaded = BuildRequest::load("fresh-request")
        .expect("load fresh request")
        .expect("fresh request exists");
    assert_eq!(reloaded.state, BuildRequestState::Queued);
    assert!(reloaded.error.is_none());
}

#[tokio::test]
async fn build_ignores_stale_pending_requests_when_computing_queue_position() {
    let _storage_guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    let _home_guard = EnvVarGuard::set("JCODE_HOME", temp_home.path());
    let _test_guard = EnvVarGuard::set("JCODE_TEST_SESSION", "1");
    let repo = create_repo_fixture();

    let mut stale_session = session::Session::create(None, Some("Stale Build".to_string()));
    stale_session.short_name = Some("ghost".to_string());
    stale_session.save().expect("save stale session");

    let stale_status_path = temp_home.path().join("stale-running.status.json");
    storage::write_json(
        &stale_status_path,
        &background::TaskStatusFile {
            task_id: "stale-task".to_string(),
            tool_name: "selfdev-build".to_string(),
            display_name: Some("selfdev build".to_string()),
            session_id: stale_session.id.clone(),
            status: BackgroundTaskStatus::Running,
            exit_code: None,
            error: None,
            started_at: Utc::now().to_rfc3339(),
            completed_at: None,
            duration_secs: None,
            pid: None,
            owner_pid: None,
            owner_instance: None,
            detached: false,
            notify: true,
            wake: true,
            progress: None,
            event_history: Vec::new(),
            stall_wake_seconds: None,
            managed_process: None,
        },
    )
    .expect("write stale status file");

    let source = test_source_state(repo.path());
    let stale_request = BuildRequest {
        request_id: "stale-queued-request".to_string(),
        background_task_id: Some("stale-task".to_string()),
        session_id: stale_session.id.clone(),
        session_short_name: stale_session.short_name.clone(),
        session_title: Some("Stale Build".to_string()),
        reason: "stale blocker".to_string(),
        repo_dir: repo.path().display().to_string(),
        repo_scope: source.repo_scope.clone(),
        worktree_scope: source.worktree_scope.clone(),
        command: "scripts/dev_cargo.sh build --profile selfdev -p jcode --bin jcode".to_string(),
        // Backdated beyond the 30s bootstrap grace so reconciliation treats the
        // dead-task request as genuinely stale (a fresh timestamp would keep it
        // alive and Queued, which is the bootstrap-race protection, not the
        // staleness path under test).
        requested_at: (Utc::now() - chrono::Duration::seconds(120)).to_rfc3339(),
        started_at: Some(Utc::now().to_rfc3339()),
        completed_at: None,
        state: BuildRequestState::Queued,
        version: Some("test-build".to_string()),
        dedupe_key: Some("stale-dedupe".to_string()),
        requested_source: Some(source),
        built_source: None,
        published_version: None,
        last_progress: Some("queued".to_string()),
        validated: false,
        error: None,
        output_file: None,
        status_file: Some(stale_status_path.display().to_string()),
        attached_to_request_id: None,
    };
    stale_request.save().expect("save stale queued request");

    let mut live_session = session::Session::create(None, Some("Live Build".to_string()));
    live_session.short_name = Some("alpha".to_string());
    live_session.save().expect("save live session");

    let tool = SelfDevTool::new();
    let output = tool
        .execute(
            json!({"action": "build", "reason": "fresh build"}),
            create_test_context(&live_session.id, Some(repo.path().to_path_buf())),
        )
        .await
        .expect("build should queue");

    let metadata = output.metadata.expect("build metadata");
    assert_eq!(metadata["queue_position"].as_u64(), Some(1));
    assert!(
        !output.output.contains("Currently blocked by"),
        "stale queued requests should not block new builds"
    );

    let stale_request = BuildRequest::load("stale-queued-request")
        .expect("load stale queued request")
        .expect("stale queued request exists");
    assert_eq!(stale_request.state, BuildRequestState::Failed);

    let task_id = metadata["task_id"].as_str().expect("task id");
    let status = wait_for_task_completion(task_id).await;
    assert_eq!(status.status, BackgroundTaskStatus::Completed);
}

#[test]
fn reconcile_pending_state_maps_superseded_background_status() {
    let _storage_guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    let _home_guard = EnvVarGuard::set("JCODE_HOME", temp_home.path());

    let mut session = session::Session::create(None, Some("Superseded Build".to_string()));
    session.short_name = Some("alpha".to_string());
    session.save().expect("save session");

    let status_path = temp_home.path().join("superseded.status.json");
    storage::write_json(
        &status_path,
        &background::TaskStatusFile {
            task_id: "superseded-task".to_string(),
            tool_name: "selfdev-build".to_string(),
            display_name: Some("selfdev build".to_string()),
            session_id: session.id.clone(),
            status: BackgroundTaskStatus::Superseded,
            exit_code: Some(0),
            error: Some("Build completed, but source changed before activation".to_string()),
            started_at: Utc::now().to_rfc3339(),
            completed_at: Some(Utc::now().to_rfc3339()),
            duration_secs: Some(1.0),
            pid: None,
            owner_pid: None,
            owner_instance: None,
            detached: false,
            notify: true,
            wake: true,
            progress: None,
            event_history: Vec::new(),
            stall_wake_seconds: None,
            managed_process: None,
        },
    )
    .expect("write superseded status file");

    let source = test_source_state(std::path::Path::new("/tmp/jcode"));
    let request = BuildRequest {
        request_id: "superseded-request".to_string(),
        background_task_id: Some("superseded-task".to_string()),
        session_id: session.id.clone(),
        session_short_name: session.short_name.clone(),
        session_title: Some("Superseded Build".to_string()),
        reason: "superseded reason".to_string(),
        repo_dir: "/tmp/jcode".to_string(),
        repo_scope: source.repo_scope.clone(),
        worktree_scope: source.worktree_scope.clone(),
        command: "scripts/dev_cargo.sh build --profile selfdev -p jcode --bin jcode".to_string(),
        requested_at: Utc::now().to_rfc3339(),
        started_at: Some(Utc::now().to_rfc3339()),
        completed_at: None,
        state: BuildRequestState::Building,
        version: Some("superseded-build".to_string()),
        dedupe_key: Some("superseded-dedupe".to_string()),
        requested_source: Some(source),
        built_source: None,
        published_version: None,
        last_progress: Some("building".to_string()),
        validated: false,
        error: None,
        output_file: None,
        status_file: Some(status_path.display().to_string()),
        attached_to_request_id: None,
    };
    request.save().expect("save superseded request");

    let mut request = BuildRequest::load("superseded-request")
        .expect("load superseded request")
        .expect("request exists");
    assert!(
        !request
            .reconcile_pending_state()
            .expect("reconcile superseded request")
    );
    assert_eq!(request.state, BuildRequestState::Superseded);
    assert!(
        request
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("source changed before activation")
    );
}

#[test]
fn reconcile_keeps_running_request_not_yet_registered_in_live_task_map() {
    // Regression: spawn_with_notify writes the Running status file and starts
    // the build future *before* inserting the task into the in-process task
    // map. The build's own first wait_for_turn iteration (or another agent's
    // queue poll) could then see status=Running + is_live_task=false and prune
    // the request instantly: "Queued build request disappeared". Within the
    // bootstrap grace window a Running-but-unregistered task must survive.
    let _storage_guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    let _home_guard = EnvVarGuard::set("JCODE_HOME", temp_home.path());

    let mut session = session::Session::create(None, Some("Racing Build".to_string()));
    session.save().expect("save session");

    let status_path = temp_home.path().join("racing.status.json");
    storage::write_json(
        &status_path,
        &background::TaskStatusFile {
            task_id: "racing-task-not-in-live-map".to_string(),
            tool_name: "selfdev-build".to_string(),
            display_name: Some("selfdev build".to_string()),
            session_id: session.id.clone(),
            status: BackgroundTaskStatus::Running,
            exit_code: None,
            error: None,
            started_at: Utc::now().to_rfc3339(),
            completed_at: None,
            duration_secs: None,
            pid: None,
            owner_pid: None,
            owner_instance: None,
            detached: false,
            notify: true,
            wake: true,
            progress: None,
            event_history: Vec::new(),
            stall_wake_seconds: None,
            managed_process: None,
        },
    )
    .expect("write running status file");

    let source = test_source_state(std::path::Path::new("/tmp/jcode"));
    let request = BuildRequest {
        request_id: "racing-request".to_string(),
        background_task_id: Some("racing-task-not-in-live-map".to_string()),
        session_id: session.id.clone(),
        session_short_name: session.short_name.clone(),
        session_title: Some("Racing Build".to_string()),
        reason: "racing reason".to_string(),
        repo_dir: "/tmp/jcode".to_string(),
        repo_scope: source.repo_scope.clone(),
        worktree_scope: source.worktree_scope.clone(),
        command: "scripts/dev_cargo.sh build --profile selfdev -p jcode --bin jcode".to_string(),
        requested_at: Utc::now().to_rfc3339(),
        started_at: None,
        completed_at: None,
        state: BuildRequestState::Queued,
        version: Some("racing-build".to_string()),
        dedupe_key: Some("racing-dedupe".to_string()),
        requested_source: Some(source.clone()),
        built_source: None,
        published_version: None,
        last_progress: Some("queued".to_string()),
        validated: false,
        error: None,
        output_file: None,
        status_file: Some(status_path.display().to_string()),
        attached_to_request_id: None,
    };
    request.save().expect("save racing request");

    let pending =
        BuildRequest::pending_requests_for_scope(&source.worktree_scope).expect("pending requests");
    assert!(
        pending
            .iter()
            .any(|request| request.request_id == "racing-request"),
        "running-but-unregistered request must stay pending during bootstrap grace"
    );

    let reloaded = BuildRequest::load("racing-request")
        .expect("load racing request")
        .expect("racing request exists");
    assert_eq!(reloaded.state, BuildRequestState::Queued);
    assert!(reloaded.error.is_none());
}
