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
    let fixture = BashCommandFixture::parent_child(Duration::from_secs(60));
    let command = fixture.command().to_string();
    let tool = BashTool::new();
    let execution = tokio::spawn(async move {
        tool.execute(
            json!({"command": command, "timeout": 30000}),
            make_ctx(None),
        )
        .await
    });

    let evidence = fixture.wait_for_process_evidence(2).await;
    let shell_pid = evidence[0].pid;
    let descendant_pid = evidence[1].pid;
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
async fn soft_yielded_ordinary_command_keeps_isolated_group() {
    let fixture = BashCommandFixture::parent_child(Duration::from_millis(800));
    let command = fixture.command().to_string();
    let execution = tokio::spawn(async move {
        BashTool::new()
            .execute(
                json!({"command": command, "soft_yield_ms": 150}),
                make_ctx(None),
            )
            .await
    });

    // Capture the original shell and descendant identities while the command is
    // still foreground, then verify that adoption preserves those exact processes.
    let evidence = fixture.wait_for_process_evidence(2).await;
    let result = execution
        .await
        .expect("ordinary command execution task")
        .expect("ordinary command should be adopted on soft yield");
    let metadata = result.metadata.expect("background metadata");
    assert_eq!(metadata["soft_yielded"], true);
    let task_id = metadata["task_id"].as_str().expect("task id").to_string();
    let shell_group =
        process_group_and_session(evidence[0].pid).expect("shell process stat after adoption");
    assert_eq!(shell_group, (evidence[0].pid, evidence[0].pid));
    assert_eq!(
        process_group_and_session(evidence[1].pid).expect("descendant process stat after adoption"),
        shell_group
    );
    assert_ne!(shell_group.0, unsafe { libc::getpgrp() } as u32);
    for process in &evidence {
        assert!(linux_process_is_live(process.pid));
        assert_eq!(
            crate::platform::process_start_token(process.pid).as_deref(),
            Some(process.start_token.as_str()),
            "soft yield must preserve the original process lineage"
        );
    }

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
    assert!(output.contains("completed"), "output was: {output}");
    assert_eq!(fixture.terminal_outcome_count(), 1);
    wait_for_process_exit(evidence[0].pid).await;
    wait_for_process_exit(evidence[1].pid).await;
}

#[tokio::test]
async fn soft_yield_completion_boundary_has_one_lineage_and_terminal_outcome() {
    const ITERATIONS: usize = 100;
    let mut adopted_task_ids = std::collections::HashSet::new();

    for iteration in 0..ITERATIONS {
        let boundary = Duration::from_millis(100);
        let fixture = BashCommandFixture::boundary_parent_child();
        let command = fixture.command().to_string();
        let started = std::time::Instant::now();
        let execution = tokio::spawn(async move {
            BashTool::new()
                .execute(
                    json!({"command": command, "soft_yield_ms": 100}),
                    make_ctx(None),
                )
                .await
        });
        let evidence = fixture.wait_for_process_evidence(2).await;
        let shell_group = process_group_and_session(evidence[0].pid)
            .unwrap_or_else(|| panic!("iteration {iteration}: shell process stat"));
        assert_eq!(shell_group, (evidence[0].pid, evidence[0].pid));
        assert_eq!(
            process_group_and_session(evidence[1].pid)
                .unwrap_or_else(|| panic!("iteration {iteration}: descendant process stat")),
            shell_group
        );
        fixture.release_after(boundary.saturating_sub(started.elapsed()));

        let result = execution
            .await
            .unwrap_or_else(|error| panic!("iteration {iteration}: execution task: {error}"))
            .unwrap_or_else(|error| panic!("iteration {iteration}: bash execution: {error}"));
        match result.metadata {
            None => {}
            Some(metadata) => {
                assert_eq!(metadata["background"], true, "iteration {iteration}");
                assert_eq!(metadata["soft_yielded"], true, "iteration {iteration}");
                let task_id = metadata["task_id"]
                    .as_str()
                    .unwrap_or_else(|| panic!("iteration {iteration}: task id"))
                    .to_string();
                assert!(
                    adopted_task_ids.insert(task_id.clone()),
                    "iteration {iteration}: duplicate task identity {task_id}"
                );

                let deadline = std::time::Instant::now() + Duration::from_secs(5);
                loop {
                    let status = crate::background::global()
                        .status(&task_id)
                        .await
                        .unwrap_or_else(|| panic!("iteration {iteration}: adopted task status"));
                    if status.status != BackgroundTaskStatus::Running {
                        assert_eq!(
                            status.status,
                            BackgroundTaskStatus::Completed,
                            "iteration {iteration}: adopted terminal status"
                        );
                        break;
                    }
                    assert!(
                        std::time::Instant::now() < deadline,
                        "iteration {iteration}: adopted task did not finish"
                    );
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
                crate::background::global()
                    .output(&task_id)
                    .await
                    .unwrap_or_else(|| panic!("iteration {iteration}: adopted output"));
            }
        }

        assert_eq!(
            fixture.terminal_outcome_count(),
            1,
            "iteration {iteration}: command must record one terminal outcome"
        );
        wait_for_process_exit(evidence[0].pid).await;
        wait_for_process_exit(evidence[1].pid).await;
    }
}
