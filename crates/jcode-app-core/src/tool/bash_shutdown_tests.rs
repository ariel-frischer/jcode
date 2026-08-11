use super::*;

#[cfg(target_os = "linux")]
#[test]
fn abrupt_runtime_shutdown_kills_foreground_but_preserves_transferred_processes() {
    let output = std::process::Command::new(std::env::current_exe().expect("current test binary"))
        .args([
            "--exact",
            "tool::bash::tests::shutdown_tests::abrupt_runtime_shutdown_process_helper",
            "--ignored",
            "--nocapture",
        ])
        .output()
        .expect("spawn isolated shutdown regression helper");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "isolated shutdown regression failed\nstdout:\n{}\nstderr:\n{}",
        stdout,
        stderr
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore = "isolated subprocess helper"]
async fn abrupt_runtime_shutdown_process_helper() {
    let tool = BashTool::new();
    let signal = jcode_agent_runtime::InterruptSignal::new();
    let ctx = make_agent_ctx(signal);
    let temp = tempfile::Builder::new()
        .prefix("bash-runtime-shutdown-")
        .tempdir_in(env!("CARGO_MANIFEST_DIR"))
        .expect("temp dir");
    let pid_file = temp.path().join("foreground-pids");
    let command = format!(
        "sleep 60 & descendant=$!; printf '%s\\n%s\\n' \"$$\" \"$descendant\" > {}; wait",
        shell_single_quote(&pid_file.to_string_lossy())
    );

    let handle = tokio::spawn(async move {
        tool.execute(json!({"command": command, "timeout": 10000}), ctx)
            .await
    });

    let startup_deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !pid_file.exists() && std::time::Instant::now() < startup_deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let pids = std::fs::read_to_string(&pid_file).expect("command should record process ids");
    let mut pids = pids.lines().map(|pid| {
        pid.parse::<u32>()
            .expect("command should record numeric process ids")
    });
    let parent_pid = pids.next().expect("parent pid");
    let descendant_pid = pids.next().expect("descendant pid");
    assert!(linux_process_is_live(parent_pid));
    assert!(linux_process_is_live(descendant_pid));
    assert!(
        !handle.is_finished(),
        "regression must invoke runtime cleanup while the tool future is still live"
    );

    crate::tool::bash::terminate_owned_foreground_process_groups();

    let teardown_deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < teardown_deadline
        && (linux_process_is_live(parent_pid) || linux_process_is_live(descendant_pid))
    {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        !linux_process_is_live(parent_pid) && !linux_process_is_live(descendant_pid),
        "runtime shutdown left foreground process group alive: parent={parent_pid}, descendant={descendant_pid}"
    );
    handle.abort();
    let _ = handle.await;

    let tool = BashTool::new();
    let signal = jcode_agent_runtime::InterruptSignal::new();
    let ctx = make_agent_ctx(signal.clone());
    let transferred_pid_file = temp.path().join("transferred-pids");
    let transferred_command = format!(
        "sleep 60 & descendant=$!; printf '%s\\n%s\\n' \"$$\" \"$descendant\" > {}; wait",
        shell_single_quote(&transferred_pid_file.to_string_lossy())
    );
    let signal_task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        signal.fire();
    });
    let result = tool
        .execute(
            json!({"command": transferred_command, "timeout": 10000}),
            ctx,
        )
        .await
        .expect("reload transfer should succeed");
    signal_task.await.expect("signal task");
    let metadata = result.metadata.expect("background metadata");
    let transferred_parent = metadata["pid"].as_u64().expect("transferred pid") as u32;
    let output_file = metadata["output_file"]
        .as_str()
        .map(std::path::PathBuf::from);
    let status_file = metadata["status_file"]
        .as_str()
        .map(std::path::PathBuf::from);
    let transferred_pids =
        std::fs::read_to_string(&transferred_pid_file).expect("transferred process ids");
    let transferred_descendant = transferred_pids
        .lines()
        .nth(1)
        .expect("transferred descendant")
        .parse::<u32>()
        .expect("numeric transferred descendant");

    crate::tool::bash::terminate_owned_foreground_process_groups();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        linux_process_is_live(transferred_parent),
        "runtime cleanup must preserve transferred parent"
    );
    assert!(
        linux_process_is_live(transferred_descendant),
        "runtime cleanup must preserve transferred descendant"
    );

    let _ = crate::platform::signal_detached_process_group(transferred_parent, libc::SIGKILL);
    if let Some(path) = output_file {
        let _ = std::fs::remove_file(path);
    }
    if let Some(path) = status_file {
        let _ = std::fs::remove_file(path);
    }
}
