#![cfg(target_os = "linux")]

use super::*;

#[cfg(target_os = "linux")]
struct ManagedProcessFixture {
    _temp: tempfile::TempDir,
    pid_file: std::path::PathBuf,
    terminal_file: std::path::PathBuf,
    command: String,
}

#[cfg(target_os = "linux")]
impl ManagedProcessFixture {
    fn parent_child(run_for: Duration) -> Self {
        let temp = tempfile::tempdir().expect("managed process fixture temp dir");
        let pid_file = temp.path().join("pids");
        let terminal_file = temp.path().join("terminal-outcomes");
        let seconds = run_for.as_secs_f64();
        let command = format!(
            "sleep {seconds:.3} & descendant=$!; printf '%s\\n%s\\n' \"$$\" \"$descendant\" > '{}'; wait \"$descendant\"; printf 'terminal\\n' >> '{}'",
            pid_file.display(),
            terminal_file.display()
        );
        Self {
            _temp: temp,
            pid_file,
            terminal_file,
            command,
        }
    }

    async fn wait_for_processes(&self) -> Vec<(u32, String)> {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(contents) = std::fs::read_to_string(&self.pid_file) {
                let pids = contents
                    .lines()
                    .map(str::parse::<u32>)
                    .collect::<Result<Vec<_>, _>>();
                if let Ok(pids) = pids
                    && pids.len() == 2
                    && let Some(evidence) = pids
                        .into_iter()
                        .map(|pid| {
                            crate::platform::process_start_token(pid).map(|token| (pid, token))
                        })
                        .collect::<Option<Vec<_>>>()
                {
                    return evidence;
                }
            }
            assert!(
                std::time::Instant::now() < deadline,
                "managed process fixture did not publish live identities"
            );
            sleep(Duration::from_millis(10)).await;
        }
    }

    fn terminal_outcome_count(&self) -> usize {
        std::fs::read_to_string(&self.terminal_file)
            .map(|contents| contents.lines().count())
            .unwrap_or(0)
    }
}

#[cfg(target_os = "linux")]
impl Drop for ManagedProcessFixture {
    fn drop(&mut self) {
        let Some(group_pid) = std::fs::read_to_string(&self.pid_file)
            .ok()
            .and_then(|contents| contents.lines().next()?.parse::<u32>().ok())
        else {
            return;
        };
        let _ = crate::platform::signal_detached_process_group(group_pid, libc::SIGKILL);
    }
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn managed_process_fixture_records_identity_and_one_terminal_outcome() -> Result<()> {
    let fixture = ManagedProcessFixture::parent_child(Duration::from_millis(150));
    let mut command = tokio::process::Command::new("sh");
    command.arg("-c").arg(&fixture.command);
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn()?;
    let evidence = fixture.wait_for_processes().await;
    assert_ne!(evidence[0].0, evidence[1].0);
    assert!(evidence.iter().all(|(_, token)| !token.is_empty()));
    let status = tokio::time::timeout(Duration::from_secs(5), child.wait()).await??;
    assert!(status.success());
    assert_eq!(fixture.terminal_outcome_count(), 1);
    Ok(())
}
