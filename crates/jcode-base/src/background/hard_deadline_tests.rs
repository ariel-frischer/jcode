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
