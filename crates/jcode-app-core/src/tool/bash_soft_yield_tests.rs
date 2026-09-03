use super::*;

pub(super) fn linux_process_is_live(pid: u32) -> bool {
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return false;
    };
    stat.rsplit_once(") ")
        .and_then(|(_, fields)| fields.chars().next())
        .is_some_and(|state| state != 'Z')
}

#[cfg(target_os = "linux")]
const BASH_FIXTURE_WAIT: Duration = Duration::from_secs(5);

#[cfg(target_os = "linux")]
#[derive(Debug)]
pub(super) struct BashProcessEvidence {
    pub(super) pid: u32,
    pub(super) start_token: String,
}

/// Deterministic shell workloads shared by the Bash lifecycle tests.
///
/// Every workload records exactly one terminal marker. Process-tree workloads
/// additionally publish their shell and descendant PIDs so tests can capture
/// start tokens before exercising yield, timeout, cancellation, or reload.
#[cfg(target_os = "linux")]
pub(super) struct BashCommandFixture {
    _temp: tempfile::TempDir,
    command: String,
    pid_file: std::path::PathBuf,
    release_file: std::path::PathBuf,
    terminal_file: std::path::PathBuf,
}

#[cfg(target_os = "linux")]
impl BashCommandFixture {
    fn new(command: impl FnOnce(&str, &str, &str) -> String) -> Self {
        let temp = tempfile::tempdir().expect("bash fixture temp dir");
        let pid_file = temp.path().join("pids");
        let release_file = temp.path().join("release");
        let terminal_file = temp.path().join("terminal-outcomes");
        let pid_path = shell_single_quote(&pid_file.to_string_lossy());
        let release_path = shell_single_quote(&release_file.to_string_lossy());
        let terminal_path = shell_single_quote(&terminal_file.to_string_lossy());
        Self {
            command: command(&pid_path, &release_path, &terminal_path),
            _temp: temp,
            pid_file,
            release_file,
            terminal_file,
        }
    }

    fn fast_success() -> Self {
        Self::new(|_, _, terminal| {
            format!("printf 'fixture-fast-success\\n'; printf 'terminal\\n' >> {terminal}")
        })
    }

    fn fast_failure(exit_code: u8) -> Self {
        Self::new(|_, _, terminal| {
            format!(
                "printf 'fixture-fast-failure\\n' >&2; printf 'terminal\\n' >> {terminal}; exit {exit_code}"
            )
        })
    }

    fn silent(run_for: Duration) -> Self {
        let seconds = run_for.as_secs_f64();
        Self::new(|_, _, terminal| {
            format!("sleep {seconds:.3}; printf 'terminal\\n' >> {terminal}")
        })
    }

    fn output_heavy(lines: usize, run_for: Duration) -> Self {
        let seconds = run_for.as_secs_f64();
        Self::new(|_, _, terminal| {
            format!(
                "i=0; while [ \"$i\" -lt {lines} ]; do printf 'fixture-output-%06d-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\\n' \"$i\"; i=$((i + 1)); done; sleep {seconds:.3}; printf 'terminal\\n' >> {terminal}"
            )
        })
    }

    pub(super) fn parent_child(run_for: Duration) -> Self {
        let seconds = run_for.as_secs_f64();
        Self::new(|pids, _, terminal| {
            format!(
                "sleep {seconds:.3} & descendant=$!; printf '%s\\n%s\\n' \"$$\" \"$descendant\" > {pids}; wait \"$descendant\"; printf 'terminal\\n' >> {terminal}"
            )
        })
    }

    pub(super) fn boundary_parent_child() -> Self {
        Self::new(|pids, release, terminal| {
            format!(
                "(while [ ! -s {release} ]; do sleep 0.001; done; delay=$(cat {release}); sleep \"$delay\") & descendant=$!; printf '%s\\n%s\\n' \"$$\" \"$descendant\" > {pids}; wait \"$descendant\"; printf 'terminal\\n' >> {terminal}"
            )
        })
    }

    pub(super) fn release_after(&self, delay: Duration) {
        std::fs::write(&self.release_file, format!("{:.6}\n", delay.as_secs_f64()))
            .expect("release boundary fixture");
    }

    pub(super) fn command(&self) -> &str {
        &self.command
    }

    pub(super) async fn wait_for_process_evidence(
        &self,
        expected: usize,
    ) -> Vec<BashProcessEvidence> {
        let deadline = std::time::Instant::now() + BASH_FIXTURE_WAIT;
        loop {
            if let Ok(contents) = std::fs::read_to_string(&self.pid_file) {
                let pids = contents
                    .lines()
                    .map(str::parse::<u32>)
                    .collect::<Result<Vec<_>, _>>();
                if let Ok(pids) = pids
                    && pids.len() == expected
                    && let Some(evidence) = pids
                        .into_iter()
                        .map(|pid| {
                            crate::platform::process_start_token(pid)
                                .map(|start_token| BashProcessEvidence { pid, start_token })
                        })
                        .collect::<Option<Vec<_>>>()
                {
                    return evidence;
                }
            }
            assert!(
                std::time::Instant::now() < deadline,
                "fixture did not publish {expected} live process identities"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    pub(super) fn terminal_outcome_count(&self) -> usize {
        std::fs::read_to_string(&self.terminal_file)
            .map(|contents| contents.lines().count())
            .unwrap_or(0)
    }
}

#[cfg(target_os = "linux")]
impl Drop for BashCommandFixture {
    fn drop(&mut self) {
        let Some(group_pid) = std::fs::read_to_string(&self.pid_file)
            .ok()
            .and_then(|contents| contents.lines().next()?.parse::<u32>().ok())
        else {
            return;
        };
        if linux_process_is_live(group_pid) {
            let _ = crate::platform::signal_detached_process_group(group_pid, libc::SIGKILL);
        }
    }
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn deterministic_bash_fixtures_cover_required_workloads() {
    let success = BashCommandFixture::fast_success();
    let result = BashTool::new()
        .execute(
            json!({"command": success.command(), "timeout": 1000}),
            make_ctx(None),
        )
        .await
        .expect("fast success fixture");
    assert!(result.output.contains("fixture-fast-success"));
    assert!(
        result.metadata.is_none(),
        "fast success must remain a direct foreground result"
    );
    assert_eq!(success.terminal_outcome_count(), 1);

    let failure = BashCommandFixture::fast_failure(17);
    let result = BashTool::new()
        .execute(
            json!({"command": failure.command(), "timeout": 1000}),
            make_ctx(None),
        )
        .await
        .expect("fast failure fixture");
    assert!(result.output.contains("fixture-fast-failure"));
    assert!(result.output.contains("17"));
    let failure_metadata = result.metadata.expect("direct failure exit metadata");
    assert_eq!(failure_metadata["exit_code"], 17);
    assert_ne!(failure_metadata["background"], true);
    assert!(failure_metadata.get("task_id").is_none());
    assert_eq!(failure.terminal_outcome_count(), 1);

    let silent = BashCommandFixture::silent(Duration::from_millis(40));
    let result = BashTool::new()
        .execute(
            json!({"command": silent.command(), "timeout": 1000}),
            make_ctx(None),
        )
        .await
        .expect("silent fixture");
    assert!(result.metadata.is_none());
    assert_eq!(silent.terminal_outcome_count(), 1);

    let output_heavy = BashCommandFixture::output_heavy(128, Duration::from_millis(10));
    let result = BashTool::new()
        .execute(
            json!({"command": output_heavy.command(), "timeout": 1000}),
            make_ctx(None),
        )
        .await
        .expect("output-heavy fixture");
    assert!(result.output.contains("fixture-output-000000"));
    assert!(result.output.contains("fixture-output-000127"));
    assert_eq!(output_heavy.terminal_outcome_count(), 1);

    let process_tree = BashCommandFixture::parent_child(Duration::from_millis(250));
    let command = process_tree.command().to_string();
    let execution = tokio::spawn(async move {
        BashTool::new()
            .execute(json!({"command": command, "timeout": 1000}), make_ctx(None))
            .await
    });
    let evidence = process_tree.wait_for_process_evidence(2).await;
    assert_ne!(evidence[0].pid, evidence[1].pid);
    assert!(evidence.iter().all(|item| !item.start_token.is_empty()));
    execution
        .await
        .expect("process-tree fixture task")
        .expect("process-tree fixture");
    assert_eq!(process_tree.terminal_outcome_count(), 1);
}

#[tokio::test]
async fn soft_yield_configuration_matrix_preserves_fast_and_direct_completion() {
    let fast_cases = [
        (json!({"command": "printf omitted_fast"}), "omitted_fast"),
        (
            json!({"command": "printf shorter_fast", "soft_yield_ms": 500}),
            "shorter_fast",
        ),
        (
            json!({"command": "printf longer_fast", "soft_yield_ms": 5_000}),
            "longer_fast",
        ),
        (
            json!({"command": "printf disabled_fast", "soft_yield_ms": 0}),
            "disabled_fast",
        ),
    ];

    for (input, marker) in fast_cases {
        let result = BashTool::new()
            .execute(input, make_ctx(None))
            .await
            .expect("fast command should complete directly");
        assert!(result.metadata.is_none(), "{marker} unexpectedly yielded");
        assert!(
            result.output.contains(marker),
            "output was: {}",
            result.output
        );
    }

    let yielded = BashTool::new()
        .execute(
            json!({
                "command": "sleep 0.15; printf shorter_window_completed",
                "soft_yield_ms": 20,
            }),
            make_ctx(None),
        )
        .await
        .expect("short soft-yield window should return managed work");
    let yielded_metadata = yielded.metadata.expect("short window should soft yield");
    assert_eq!(yielded_metadata["background"], true);
    assert_eq!(yielded_metadata["soft_yielded"], true);
    assert_eq!(yielded_metadata["soft_yield_ms"], 20);
    let yielded_task_id = yielded_metadata["task_id"].as_str().expect("task id");
    let completed = crate::background::global()
        .wait(yielded_task_id, Duration::from_secs(3), false)
        .await
        .expect("yielded task should remain manageable");
    assert_eq!(completed.task.status, BackgroundTaskStatus::Completed);

    for (soft_yield_ms, marker) in [(500, "longer_window_direct"), (0, "disabled_direct")] {
        let result = BashTool::new()
            .execute(
                json!({
                    "command": format!("sleep 0.05; printf {marker}"),
                    "soft_yield_ms": soft_yield_ms,
                }),
                make_ctx(None),
            )
            .await
            .expect("longer or disabled window should await natural completion");
        assert!(result.metadata.is_none(), "{marker} unexpectedly yielded");
        assert!(
            result.output.contains(marker),
            "output was: {}",
            result.output
        );
    }
}

#[tokio::test]
async fn default_soft_yield_adds_at_most_twenty_ms_median_fast_command_overhead() {
    const SAMPLE_COUNT: usize = 21;
    let mut added_overhead = Vec::with_capacity(SAMPLE_COUNT);

    for sample in 0..SAMPLE_COUNT {
        let run = |soft_yield_ms: Option<u64>| async move {
            let input = match soft_yield_ms {
                Some(value) => json!({"command": "true", "soft_yield_ms": value}),
                None => json!({"command": "true"}),
            };
            let started = std::time::Instant::now();
            let result = BashTool::new()
                .execute(input, make_ctx(None))
                .await
                .expect("fast timing command should complete directly");
            assert!(result.metadata.is_none());
            started.elapsed()
        };

        let (baseline, with_default) = if sample % 2 == 0 {
            (run(Some(0)).await, run(None).await)
        } else {
            let with_default = run(None).await;
            let baseline = run(Some(0)).await;
            (baseline, with_default)
        };
        added_overhead.push(with_default.saturating_sub(baseline));
    }

    added_overhead.sort_unstable();
    let median = added_overhead[SAMPLE_COUNT / 2];
    assert!(
        median <= Duration::from_millis(20),
        "default soft yield added {median:?} median overhead; samples: {added_overhead:?}"
    );
}
#[tokio::test]
async fn test_command_soft_yield_with_stdin_channel() {
    let (tx, _rx) = mpsc::unbounded_channel::<StdinInputRequest>();
    let tool = BashTool::new();

    // `cat` blocks forever on stdin. With a short soft-yield window and no stdin
    // response, the command should be adopted by the background manager without
    // being killed.
    let input = json!({"command": "cat", "soft_yield_ms": 100});
    let ctx = make_ctx(Some(tx));

    let result = tool
        .execute(input, ctx)
        .await
        .expect("soft yield should adopt the command, not error");
    assert!(
        result.output.contains("continuing in background"),
        "output should explain background promotion: {}",
        result.output
    );
    let metadata = result.metadata.expect("expected background metadata");
    assert_eq!(metadata["background"], true);
    assert_eq!(metadata["soft_yielded"], true);
    assert_eq!(metadata["soft_yield_ms"], 100);

    // Clean up the still-running background task so it does not linger.
    let task_id = metadata["task_id"]
        .as_str()
        .expect("task_id should be present");
    let _ = crate::background::global().cancel(task_id).await;
}

#[tokio::test]
async fn test_foreground_soft_yield_promotes_and_command_keeps_running() {
    let tool = BashTool::new();
    // No stdin channel and Direct mode -> plain foreground path. The command runs
    // longer than the soft-yield window, so the same process should be adopted by
    // the background manager and continue to completion.
    let input = json!({"command": "sleep 0.5; echo fg_promote_ok", "soft_yield_ms": 100});
    let ctx = make_ctx(None);

    let result = tool
        .execute(input, ctx)
        .await
        .expect("timeout should promote the still-running command to background");
    assert!(
        result.output.contains("continuing in background"),
        "output should explain background promotion: {}",
        result.output
    );
    let metadata = result.metadata.expect("expected background metadata");
    assert_eq!(metadata["background"], true);
    assert_eq!(metadata["soft_yielded"], true);
    let task_id = metadata["task_id"]
        .as_str()
        .expect("task_id should be present")
        .to_string();

    // Wait for the promoted command to finish on its own.
    let mut final_status = None;
    for _ in 0..40 {
        if let Some(status) = crate::background::global().status(&task_id).await
            && status.status != BackgroundTaskStatus::Running
        {
            final_status = Some(status);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let status = final_status.expect("promoted background task should finish");
    assert_eq!(status.status, BackgroundTaskStatus::Completed);

    let output = crate::background::global()
        .output(&task_id)
        .await
        .expect("output should exist");
    assert!(
        output.contains("fg_promote_ok"),
        "command should have continued after soft yield: {output}"
    );
}

#[tokio::test]
async fn soft_yielded_command_defaults_to_terminal_wake_and_inherits_management_options() {
    let tool = BashTool::new();
    let result = tool
        .execute(
            json!({
                "command": "echo 'progress 10% done'; sleep 0.4; echo lifecycle_done",
                "soft_yield_ms": 100,
                "notify": false,
                "stall_wake_seconds": 1,
            }),
            make_ctx(None),
        )
        .await
        .expect("soft yield should adopt the running command");
    let metadata = result.metadata.expect("expected background metadata");
    let task_id = metadata["task_id"].as_str().expect("task id").to_string();

    let status = crate::background::global()
        .status(&task_id)
        .await
        .expect("adopted task status");
    assert!(
        status.notify,
        "default completion wake must imply notification"
    );
    assert!(
        status.wake,
        "automatic soft yield must default to one terminal wake"
    );
    let wait = crate::background::global()
        .wait(&task_id, Duration::from_secs(5), false)
        .await
        .expect("adopted task should be waitable");
    assert_eq!(wait.task.task_id, task_id);
    assert_eq!(
        wait.task.stall_wake_seconds,
        Some(crate::background::BackgroundTaskManager::MIN_STALL_WAKE_SECONDS),
        "automatic adoption must retain the clamped stall watchdog configuration through terminal finalization"
    );
    assert!(
        crate::background::global()
            .output(&task_id)
            .await
            .is_some_and(|output| output.contains("lifecycle_done")),
        "completed adopted task output must remain addressable"
    );
}

#[tokio::test]
async fn explicit_background_keeps_caller_selected_wake_default() {
    let result = BashTool::new()
        .execute(
            json!({
                "command": "sleep 0.2; echo explicit_background_done",
                "run_in_background": true,
            }),
            make_ctx(None),
        )
        .await
        .expect("explicit background command should start");
    let metadata = result.metadata.expect("expected background metadata");
    let task_id = metadata["task_id"].as_str().expect("task id");

    let status = crate::background::global()
        .status(task_id)
        .await
        .expect("explicit background task status");
    assert!(status.notify, "completion notification remains enabled");
    assert!(
        !status.wake,
        "explicit background execution must preserve its false wake default"
    );

    crate::background::global()
        .wait(task_id, Duration::from_secs(3), false)
        .await
        .expect("explicit background task should finish");
}

#[tokio::test]
async fn output_heavy_soft_yield_remains_bounded_and_retrievable() {
    let tool = BashTool::new();
    let result = tool
        .execute(
            json!({
                "command": "head -c 50000 /dev/zero | tr '\\0' x; sleep 0.3; echo output_heavy_done",
                "soft_yield_ms": 100,
            }),
            make_ctx(None),
        )
        .await
        .expect("output-heavy command should soft yield");
    assert!(
        result.output.len() < 5_000,
        "yield response must not embed accumulated command output"
    );
    let metadata = result.metadata.expect("expected background metadata");
    let task_id = metadata["task_id"].as_str().expect("task id").to_string();

    let finished = crate::background::global()
        .wait(&task_id, Duration::from_secs(5), false)
        .await
        .expect("output-heavy task should remain waitable");
    assert_ne!(finished.task.status, BackgroundTaskStatus::Running);
    let output = crate::background::global()
        .output(&task_id)
        .await
        .expect("bounded terminal output should remain retrievable");
    assert!(
        output.len() <= MAX_OUTPUT_LEN + 64,
        "output was {} bytes",
        output.len()
    );
    assert!(output.contains("... (output truncated)"));
}
#[tokio::test]
async fn test_reload_persistable_bash_soft_yields_to_background() {
    let tool = BashTool::new();
    let signal = jcode_agent_runtime::InterruptSignal::new();
    let ctx = make_agent_ctx(signal);

    let result = tool
        .execute(
            json!({"command": "sleep 0.4; echo timeout_promote_ok", "soft_yield_ms": 100}),
            ctx,
        )
        .await
        .expect("timeout should promote the still-running command to background");

    assert!(
        result.output.contains("continuing in background"),
        "output should explain background promotion: {}",
        result.output
    );
    assert!(
        result.output.contains("do not rerun"),
        "output should tell the agent not to rerun duplicate work: {}",
        result.output
    );

    let metadata = result.metadata.expect("expected background metadata");
    assert_eq!(metadata["background"], true);
    assert_eq!(metadata["soft_yielded"], true);
    assert_eq!(metadata["soft_yield_ms"], 100);
    let task_id = metadata["task_id"]
        .as_str()
        .expect("task_id should be present")
        .to_string();
    let output_file = std::path::PathBuf::from(
        metadata["output_file"]
            .as_str()
            .expect("output_file should be present"),
    );
    let status_file = std::path::PathBuf::from(
        metadata["status_file"]
            .as_str()
            .expect("status_file should be present"),
    );

    let initial_status = crate::background::global()
        .status(&task_id)
        .await
        .expect("status should exist");
    assert_eq!(initial_status.status, BackgroundTaskStatus::Running);

    let mut final_status = None;
    for _ in 0..40 {
        let status = crate::background::global()
            .status(&task_id)
            .await
            .expect("status should exist");
        if status.status != BackgroundTaskStatus::Running {
            final_status = Some(status);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    let status = final_status.expect("promoted background task should finish");
    assert_eq!(status.status, BackgroundTaskStatus::Completed);
    assert_eq!(status.exit_code, Some(0));

    let output = crate::background::global()
        .output(&task_id)
        .await
        .expect("output should exist");
    assert!(
        output.contains("timeout_promote_ok"),
        "command should have continued after soft yield: {output}"
    );

    let _ = tokio::fs::remove_file(output_file).await;
    let _ = tokio::fs::remove_file(status_file).await;
}

#[tokio::test]
async fn reload_persistable_soft_yield_records_future_hard_deadline() {
    let tool = BashTool::new();
    let signal = jcode_agent_runtime::InterruptSignal::new();
    let result = tool
        .execute(
            json!({
                "command": "sleep 60",
                "soft_yield_ms": 100,
                "timeout": 5_000,
            }),
            make_agent_ctx(signal),
        )
        .await
        .expect("reload-persistable command should soft yield");
    let metadata = result.metadata.expect("expected background metadata");
    let task_id = metadata["task_id"].as_str().expect("task id");
    let status = crate::background::global()
        .status(task_id)
        .await
        .expect("persisted task status");
    assert!(status.detached);
    assert!(
        status
            .hard_deadline_at
            .is_some_and(|deadline| deadline > chrono::Utc::now()),
        "explicit hard timeout must survive automatic detached promotion"
    );

    let _ = crate::background::global().cancel(task_id).await;
}
/// The bug this guards against: a foreground command adopted by the background
/// manager at soft yield showed 0% until it completed, because nothing parsed its
/// output for progress. Both the update emitted *before* adoption and updates
/// emitted *after* adoption must reach the background task's status.
#[tokio::test]
async fn test_soft_yielded_command_reports_intermediate_progress() {
    let tool = BashTool::new();
    // Emits 10% before the 300ms soft-yield window, then 80% about 2s in.
    let input = json!({
        "command": "echo 'progress 10% done'; sleep 2; echo 'progress 80% done'; sleep 1",
        "soft_yield_ms": 300,
    });
    let ctx = make_ctx(None);

    let result = tool
        .execute(input, ctx)
        .await
        .expect("soft yield should adopt the command");
    let metadata = result.metadata.expect("expected background metadata");
    assert_eq!(metadata["soft_yielded"], true);
    let task_id = metadata["task_id"]
        .as_str()
        .expect("task_id should be present")
        .to_string();

    // The pre-promotion update (10%) must be attached at promotion time, and
    // the post-promotion update (80%) must stream in while still running.
    let mut observed: Vec<f32> = Vec::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        let status = crate::background::global()
            .status(&task_id)
            .await
            .expect("status should exist");
        if let Some(percent) = status.progress.as_ref().and_then(|p| p.percent)
            && observed.last() != Some(&percent)
        {
            observed.push(percent);
        }
        if observed.contains(&80.0) {
            break;
        }
        if status.status != BackgroundTaskStatus::Running {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    assert!(
        observed.contains(&80.0),
        "promoted task should reach 80% via parsed output, saw {observed:?}"
    );
    assert!(
        observed.contains(&10.0),
        "the pre-promotion 10% update should be flushed at promotion, saw {observed:?}"
    );

    let _ = crate::background::global().cancel(&task_id).await;
}
