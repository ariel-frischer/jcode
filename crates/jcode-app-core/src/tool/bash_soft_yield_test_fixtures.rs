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
