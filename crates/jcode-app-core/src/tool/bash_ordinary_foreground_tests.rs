use super::*;

fn process_group_and_session(pid: u32) -> Option<(u32, u32)> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, fields) = stat.rsplit_once(") ")?;
    let mut fields = fields.split_whitespace();
    let _state = fields.next()?;
    let _parent_pid = fields.next()?;
    let process_group = fields.next()?.parse().ok()?;
    let session = fields.next()?.parse().ok()?;
    Some((process_group, session))
}

async fn wait_for_process_exit(pid: u32) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if !linux_process_is_live(pid) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("process {pid} survived foreground ownership cleanup");
}

#[tokio::test]
async fn ordinary_foreground_drop_kills_isolated_shell_and_descendant() {
    let temp = tempfile::tempdir().expect("temp dir");
    let pid_file = temp.path().join("pids");
    let command = format!(
        "sleep 60 & descendant=$!; printf '%s\\n%s\\n' \"$$\" \"$descendant\" > {}; wait \"$descendant\"",
        shell_single_quote(&pid_file.to_string_lossy())
    );
    let tool = BashTool::new();
    let execution = tokio::spawn(async move {
        tool.execute(
            json!({"command": command, "timeout": 30000}),
            make_ctx(None),
        )
        .await
    });

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let pids = loop {
        if let Ok(contents) = std::fs::read_to_string(&pid_file) {
            let parsed = contents
                .lines()
                .map(str::parse::<u32>)
                .collect::<Result<Vec<_>, _>>();
            if let Ok(pids) = parsed
                && pids.len() == 2
            {
                break pids;
            }
        }
        assert!(
            std::time::Instant::now() < deadline,
            "ordinary foreground command did not publish its pids"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    let shell_pid = pids[0];
    let descendant_pid = pids[1];
    let shell_group = process_group_and_session(shell_pid).expect("shell process stat");
    let descendant_group =
        process_group_and_session(descendant_pid).expect("descendant process stat");

    execution.abort();
    let _ = execution.await;

    assert_eq!(shell_group, (shell_pid, shell_pid));
    assert_eq!(descendant_group, shell_group);
    assert_ne!(shell_group.0, unsafe { libc::getpgrp() } as u32);
    wait_for_process_exit(shell_pid).await;
    wait_for_process_exit(descendant_pid).await;
}

#[tokio::test]
async fn timeout_promoted_ordinary_command_keeps_isolated_group() {
    let temp = tempfile::tempdir().expect("temp dir");
    let pid_file = temp.path().join("pids");
    let command = format!(
        "sleep 0.5 & descendant=$!; printf '%s\\n%s\\n' \"$$\" \"$descendant\" > {}; wait \"$descendant\"; echo ordinary_promote_ok",
        shell_single_quote(&pid_file.to_string_lossy())
    );

    let result = BashTool::new()
        .execute(json!({"command": command, "timeout": 100}), make_ctx(None))
        .await
        .expect("ordinary command should promote on timeout");
    let metadata = result.metadata.expect("background metadata");
    assert_eq!(metadata["timeout_promoted"], true);
    let task_id = metadata["task_id"].as_str().expect("task id").to_string();
    let pids = std::fs::read_to_string(&pid_file)
        .expect("promoted command pid file")
        .lines()
        .map(|line| line.parse::<u32>().expect("numeric pid"))
        .collect::<Vec<_>>();
    assert_eq!(pids.len(), 2);
    let shell_group = process_group_and_session(pids[0]).expect("shell process stat");
    assert_eq!(shell_group, (pids[0], pids[0]));
    assert_eq!(
        process_group_and_session(pids[1]).expect("descendant process stat"),
        shell_group
    );
    assert_ne!(shell_group.0, unsafe { libc::getpgrp() } as u32);
    assert!(linux_process_is_live(pids[0]));
    assert!(linux_process_is_live(pids[1]));

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let status = crate::background::global()
            .status(&task_id)
            .await
            .expect("promoted task status");
        if status.status != BackgroundTaskStatus::Running {
            assert_eq!(status.status, BackgroundTaskStatus::Completed);
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "promoted ordinary command did not finish"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let output = crate::background::global()
        .output(&task_id)
        .await
        .expect("promoted output");
    assert!(
        output.contains("ordinary_promote_ok"),
        "output was: {output}"
    );
}
