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

async fn wait_for_terminal_status(task_id: &str) -> crate::background::TaskStatusFile {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = crate::background::global().status(task_id).await
            && status.status != BackgroundTaskStatus::Running
        {
            return status;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "managed task {task_id} did not reach a terminal state"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
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
async fn hard_timeout_before_soft_yield_kills_foreground_process_group() {
    let fixture = BashCommandFixture::parent_child(Duration::from_secs(60));
    let command = fixture.command().to_string();
    let execution = tokio::spawn(async move {
        BashTool::new()
            .execute(
                json!({"command": command, "soft_yield_ms": 2_000, "timeout": 500}),
                make_ctx(None),
            )
            .await
    });

    let evidence = fixture.wait_for_process_evidence(2).await;
    let result = execution
        .await
        .expect("foreground timeout execution task")
        .expect("foreground timeout result");
    let metadata = result.metadata.expect("hard-timeout metadata");
    assert_eq!(metadata["exit_code"], 124);
    assert_eq!(metadata["timed_out"], true);
    assert_ne!(metadata["background"], true);
    assert!(metadata.get("task_id").is_none());
    assert!(result.output.contains("timed out after 500ms"));
    assert_eq!(fixture.terminal_outcome_count(), 0);
    for process in evidence {
        wait_for_process_exit(process.pid).await;
    }
}

#[tokio::test]
async fn hard_timeout_after_soft_yield_kills_original_process_group_once() {
    let fixture = BashCommandFixture::parent_child(Duration::from_secs(60));
    let command = fixture.command().to_string();
    let execution = tokio::spawn(async move {
        BashTool::new()
            .execute(
                json!({"command": command, "soft_yield_ms": 100, "timeout": 700}),
                make_ctx(None),
            )
            .await
    });

    let evidence = fixture.wait_for_process_evidence(2).await;
    let result = execution
        .await
        .expect("soft-yield timeout execution task")
        .expect("soft-yield timeout result");
    let metadata = result.metadata.expect("soft-yield metadata");
    assert_eq!(metadata["soft_yielded"], true);
    let task_id = metadata["task_id"].as_str().expect("task id").to_string();
    for process in &evidence {
        assert!(linux_process_is_live(process.pid));
        assert_eq!(
            crate::platform::process_start_token(process.pid).as_deref(),
            Some(process.start_token.as_str()),
            "adoption must preserve the original process lineage"
        );
    }

    let status = wait_for_terminal_status(&task_id).await;
    assert_eq!(status.status, BackgroundTaskStatus::Failed);
    assert_eq!(status.exit_code, Some(124));
    assert!(
        status
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("timed out"),
        "timeout failure should be recorded: {status:?}"
    );
    assert_eq!(fixture.terminal_outcome_count(), 0);
    for process in evidence {
        wait_for_process_exit(process.pid).await;
    }
}

#[tokio::test]
async fn immediate_background_hard_timeout_returns_promptly_and_kills_process_group() {
    let fixture = BashCommandFixture::parent_child(Duration::from_secs(60));
    let started = std::time::Instant::now();
    let result = BashTool::new()
        .execute(
            json!({
                "command": fixture.command(),
                "run_in_background": true,
                "timeout": 700,
                "notify": false,
                "wake": false,
            }),
            make_ctx(None),
        )
        .await
        .expect("immediate background result");
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "immediate backgrounding waited too long: {:?}",
        started.elapsed()
    );
    let metadata = result.metadata.expect("background metadata");
    assert_eq!(metadata["background"], true);
    let task_id = metadata["task_id"].as_str().expect("task id").to_string();
    let evidence = fixture.wait_for_process_evidence(2).await;

    let status = wait_for_terminal_status(&task_id).await;
    assert_eq!(status.status, BackgroundTaskStatus::Failed);
    assert_eq!(status.exit_code, Some(124));
    assert_eq!(fixture.terminal_outcome_count(), 0);
    for process in evidence {
        wait_for_process_exit(process.pid).await;
    }
}

#[tokio::test]
async fn timeout_completion_boundary_records_one_authoritative_outcome() {
    const ITERATIONS: usize = 20;

    for iteration in 0..ITERATIONS {
        let fixture = BashCommandFixture::boundary_parent_child();
        let command = fixture.command().to_string();
        let started = std::time::Instant::now();
        let execution = tokio::spawn(async move {
            BashTool::new()
                .execute(
                    json!({"command": command, "soft_yield_ms": 0, "timeout": 200}),
                    make_ctx(None),
                )
                .await
        });
        let evidence = fixture.wait_for_process_evidence(2).await;
        let completion_wins = iteration % 2 == 0;
        let release_at = if completion_wins {
            Duration::from_millis(50)
        } else {
            Duration::from_millis(350)
        };
        fixture.release_after(release_at.saturating_sub(started.elapsed()));

        let result = execution
            .await
            .unwrap_or_else(|error| panic!("iteration {iteration}: execution task: {error}"))
            .unwrap_or_else(|error| panic!("iteration {iteration}: bash execution: {error}"));
        if completion_wins {
            assert!(
                result.metadata.is_none(),
                "iteration {iteration}: {result:?}"
            );
            assert_eq!(fixture.terminal_outcome_count(), 1, "iteration {iteration}");
        } else {
            let metadata = result
                .metadata
                .unwrap_or_else(|| panic!("iteration {iteration}: timeout metadata"));
            assert_eq!(metadata["exit_code"], 124, "iteration {iteration}");
            assert_eq!(metadata["timed_out"], true, "iteration {iteration}");
            assert_eq!(fixture.terminal_outcome_count(), 0, "iteration {iteration}");
        }
        for process in evidence {
            wait_for_process_exit(process.pid).await;
        }
    }
}

#[tokio::test]
async fn timeout_yield_boundary_uses_one_task_and_one_process_lineage() {
    const ITERATIONS: usize = 12;

    for iteration in 0..ITERATIONS {
        let fixture = BashCommandFixture::parent_child(Duration::from_secs(60));
        let command = fixture.command().to_string();
        let yield_wins = iteration % 2 == 0;
        let (soft_yield_ms, timeout_ms) = if yield_wins { (50, 200) } else { (200, 50) };
        let execution = tokio::spawn(async move {
            BashTool::new()
                .execute(
                    json!({
                        "command": command,
                        "soft_yield_ms": soft_yield_ms,
                        "timeout": timeout_ms,
                    }),
                    make_ctx(None),
                )
                .await
        });
        let evidence = fixture.wait_for_process_evidence(2).await;
        let result = execution
            .await
            .unwrap_or_else(|error| panic!("iteration {iteration}: execution task: {error}"))
            .unwrap_or_else(|error| panic!("iteration {iteration}: bash execution: {error}"));

        if yield_wins {
            let metadata = result
                .metadata
                .unwrap_or_else(|| panic!("iteration {iteration}: soft-yield metadata"));
            assert_eq!(metadata["soft_yielded"], true, "iteration {iteration}");
            let task_id = metadata["task_id"]
                .as_str()
                .unwrap_or_else(|| panic!("iteration {iteration}: task id"));
            let status = wait_for_terminal_status(task_id).await;
            assert_eq!(
                status.status,
                BackgroundTaskStatus::Failed,
                "iteration {iteration}"
            );
            assert_eq!(status.exit_code, Some(124), "iteration {iteration}");
        } else {
            let metadata = result
                .metadata
                .unwrap_or_else(|| panic!("iteration {iteration}: timeout metadata"));
            assert_eq!(metadata["exit_code"], 124, "iteration {iteration}");
            assert_eq!(metadata["timed_out"], true, "iteration {iteration}");
            assert!(metadata.get("task_id").is_none(), "iteration {iteration}");
        }
        assert_eq!(fixture.terminal_outcome_count(), 0, "iteration {iteration}");
        for process in evidence {
            wait_for_process_exit(process.pid).await;
        }
    }
}

#[tokio::test]
async fn cancellation_immediately_after_soft_yield_stops_original_process_group_once() {
    const ITERATIONS: usize = 10;

    for iteration in 0..ITERATIONS {
        let fixture = BashCommandFixture::parent_child(Duration::from_secs(60));
        let command = fixture.command().to_string();
        let execution = tokio::spawn(async move {
            BashTool::new()
                .execute(
                    json!({"command": command, "soft_yield_ms": 50, "timeout": 5_000}),
                    make_ctx(None),
                )
                .await
        });
        let evidence = fixture.wait_for_process_evidence(2).await;
        let result = execution
            .await
            .unwrap_or_else(|error| panic!("iteration {iteration}: execution task: {error}"))
            .unwrap_or_else(|error| panic!("iteration {iteration}: bash execution: {error}"));
        let metadata = result
            .metadata
            .unwrap_or_else(|| panic!("iteration {iteration}: soft-yield metadata"));
        let task_id = metadata["task_id"]
            .as_str()
            .unwrap_or_else(|| panic!("iteration {iteration}: task id"));

        assert_eq!(
            crate::background::global()
                .cancel(task_id)
                .await
                .unwrap_or_else(|error| panic!("iteration {iteration}: cancellation: {error}")),
            crate::background::BackgroundTaskCancellation::FullyStopped,
            "iteration {iteration}"
        );
        let status = wait_for_terminal_status(task_id).await;
        assert_eq!(
            status.status,
            BackgroundTaskStatus::Failed,
            "iteration {iteration}"
        );
        assert_ne!(status.exit_code, Some(124), "iteration {iteration}");
        assert_eq!(fixture.terminal_outcome_count(), 0, "iteration {iteration}");
        for process in evidence {
            wait_for_process_exit(process.pid).await;
        }
    }
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
