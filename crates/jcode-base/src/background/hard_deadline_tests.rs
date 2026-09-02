use super::*;

#[test]
fn legacy_status_round_trips_without_managed_identity() -> Result<()> {
    let legacy = serde_json::json!({
        "task_id": "legacy-round-trip",
        "tool_name": "bash",
        "session_id": "session",
        "status": "running",
        "exit_code": null,
        "error": null,
        "started_at": Utc::now().to_rfc3339(),
        "completed_at": null,
        "duration_secs": null,
        "pid": null,
        "detached": false,
        "notify": false,
        "wake": false,
        "progress": null,
        "event_history": []
    });
    let status: TaskStatusFile = serde_json::from_value(legacy)?;
    assert_eq!(status.managed_process, None);
    assert_eq!(status.hard_deadline_at, None);
    assert_eq!(
        serde_json::from_str::<TaskStatusFile>(&serde_json::to_string(&status)?)?.task_id,
        "legacy-round-trip"
    );
    Ok(())
}

#[test]
fn managed_task_hard_deadline_round_trips_and_distinguishes_deadline_states() -> Result<()> {
    let now = Utc::now();
    let managed_process = ManagedProcessIdentity {
        pid: 42,
        process_instance: Some("process-token".to_string()),
        process_group_member: Some(ManagedProcessMemberIdentity {
            pid: 43,
            process_instance: Some("member-token".to_string()),
        }),
        owner_instance: Some("owner-token".to_string()),
        transfer_policy: ManagedProcessTransferPolicy::Transferred,
    };

    for deadline in [
        None,
        Some(now - chrono::Duration::seconds(1)),
        Some(now + chrono::Duration::seconds(60)),
    ] {
        let mut status = running_status_fixture("deadline-state", "deadline-session");
        status.managed_process = Some(managed_process.clone());
        status.hard_deadline_at = deadline;

        let encoded = serde_json::to_string(&status)?;
        let decoded: TaskStatusFile = serde_json::from_str(&encoded)?;
        assert_eq!(decoded.hard_deadline_at, deadline);
        assert_eq!(decoded.managed_process, Some(managed_process.clone()));
        assert_eq!(decoded.progress, status.progress);
        assert_eq!(decoded.event_history, status.event_history);
        assert_eq!(decoded.notify, status.notify);
        assert_eq!(decoded.wake, status.wake);
    }
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn detached_hard_deadline_terminates_group_and_records_exit_124_once() -> Result<()> {
    use std::os::unix::process::CommandExt;

    let tmp = tempfile::tempdir()?;
    let manager = BackgroundTaskManager::with_output_dir(tmp.path().to_path_buf());
    let descendant_file = tmp.path().join("descendant.pid");
    let mut command = std::process::Command::new("sh");
    command
        .args([
            "-c",
            &format!(
                "sleep 60 & descendant=$!; printf '%s' \"$descendant\" > {}; wait",
                descendant_file.display()
            ),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn()?;
    let pid = child.id();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !descendant_file.exists() && std::time::Instant::now() < deadline {
        sleep(Duration::from_millis(10)).await;
    }
    let descendant_pid = std::fs::read_to_string(&descendant_file)?.parse::<u32>()?;
    let info = manager.reserve_task_info();
    manager
        .register_detached_task_with_identity_and_deadline(
            &info,
            "bash",
            Some("deadline test".to_string()),
            "deadline-session",
            pid,
            &Utc::now().to_rfc3339(),
            true,
            true,
            Some(ManagedProcessIdentity {
                pid,
                process_instance: crate::platform::process_start_token(pid),
                process_group_member: Some(ManagedProcessMemberIdentity {
                    pid: descendant_pid,
                    process_instance: crate::platform::process_start_token(descendant_pid),
                }),
                owner_instance: Some(process_instance_token().to_string()),
                transfer_policy: ManagedProcessTransferPolicy::Transferred,
            }),
            Some(Utc::now() + chrono::Duration::milliseconds(150)),
        )
        .await;

    let final_status = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let status = manager.status(&info.task_id).await.expect("status");
            if status.status != BackgroundTaskStatus::Running {
                break status;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await?;
    assert_eq!(final_status.status, BackgroundTaskStatus::Failed);
    assert_eq!(final_status.exit_code, Some(124));
    assert_eq!(
        final_status
            .event_history
            .iter()
            .filter(|event| event.exit_code == Some(124))
            .count(),
        1
    );
    assert!(!crate::platform::is_process_group_live(pid));
    assert!(!crate::platform::is_process_running(descendant_pid));
    let _ = child.wait();
    Ok(())
}
