use super::*;

impl BackgroundTaskManager {
    pub(super) async fn finalize_detached_status_if_needed(
        &self,
        mut status: TaskStatusFile,
        status_path: &std::path::Path,
    ) -> TaskStatusFile {
        let _deadline_guard = self.deadline_reconciliation.lock().await;
        let Some(current_status) = self.read_status_file(status_path).await else {
            return status;
        };
        status = current_status;
        if status.status != BackgroundTaskStatus::Running || !status.detached {
            return status;
        }

        let Some(pid) = status.pid else {
            return status;
        };

        let Some(identity) = status.managed_process.as_ref() else {
            status.error = Some(
                "Managed process identity is missing; detached reconciliation failed closed"
                    .to_string(),
            );
            self.write_status_file(status_path, &status).await;
            return status;
        };
        if identity.pid != pid {
            status.error = Some(
                "Managed process PID does not match detached status; reconciliation failed closed"
                    .to_string(),
            );
            self.write_status_file(status_path, &status).await;
            return status;
        }
        let member = identity
            .process_group_member
            .as_ref()
            .and_then(|member| Some((member.pid, member.process_instance.as_deref()?)));
        let identity_check = crate::platform::verify_process_group_identity(
            identity.pid,
            identity.process_instance.as_deref(),
            member,
        );
        match identity_check {
            crate::platform::ProcessIdentityCheck::Matching => {}
            crate::platform::ProcessIdentityCheck::Stopped
                if crate::platform::is_process_group_live(identity.pid) =>
            {
                return status;
            }
            crate::platform::ProcessIdentityCheck::Stopped => {}
            check => {
                status.error = Some(format!(
                    "Managed process identity verification failed during reconciliation ({check:?})"
                ));
                self.write_status_file(status_path, &status).await;
                return status;
            }
        }

        if identity_check == crate::platform::ProcessIdentityCheck::Matching
            && status
                .hard_deadline_at
                .is_some_and(|deadline| deadline <= Utc::now())
        {
            if let Err(error) = self
                .terminate_managed_process_group(identity, Duration::from_millis(400))
                .await
            {
                status.error = Some(format!(
                    "Hard deadline expired but managed process teardown was not completed: {error:?}"
                ));
                self.write_status_file(status_path, &status).await;
                return status;
            }

            drop(crate::platform::try_reap_child_process(pid));
            let output_path = self.output_path_for(&status.task_id);
            let timeout_message = "Command timed out after its hard deadline";
            if let Ok(mut output) = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&output_path)
                .await
            {
                drop(
                    output
                        .write_all(format!("\n--- {timeout_message} ---\n").as_bytes())
                        .await,
                );
            }
            let completed_at = Utc::now();
            status.status = BackgroundTaskStatus::Failed;
            status.exit_code = Some(124);
            status.error = Some(timeout_message.to_string());
            status.completed_at = Some(completed_at.to_rfc3339());
            status.duration_secs = Self::status_duration_secs(&status.started_at, completed_at);
            push_task_event(
                &mut status,
                terminal_event_record(
                    BackgroundTaskStatus::Failed,
                    Some(124),
                    Some(timeout_message),
                ),
            );
            self.write_status_file(status_path, &status).await;
            Bus::global().publish(BusEvent::BackgroundTaskCompleted(BackgroundTaskCompleted {
                task_id: status.task_id.clone(),
                tool_name: status.tool_name.clone(),
                display_name: status.display_name.clone(),
                session_id: status.session_id.clone(),
                status: BackgroundTaskStatus::Failed,
                exit_code: Some(124),
                output_preview: timeout_message.to_string(),
                output_file: output_path,
                duration_secs: status.duration_secs.unwrap_or(0.0),
                notify: status.notify,
                wake: status.wake,
            }));
            return status;
        }

        let reaped_exit = crate::platform::try_reap_child_process(pid).unwrap_or(None);

        if reaped_exit.is_none()
            && (crate::platform::is_process_running(pid)
                || crate::platform::is_process_group_live(identity.pid))
        {
            return status;
        }

        let output_path = self.output_path_for(&status.task_id);
        let output = fs::read_to_string(&output_path)
            .await
            .unwrap_or_else(|_| String::new());
        let exit_code = reaped_exit.or_else(|| Self::parse_exit_code_from_output(&output));
        let completed_at = Utc::now();
        let duration_secs = Self::status_duration_secs(&status.started_at, completed_at);
        let final_status = if matches!(exit_code, Some(0)) {
            BackgroundTaskStatus::Completed
        } else {
            BackgroundTaskStatus::Failed
        };
        let final_error = if matches!(final_status, BackgroundTaskStatus::Failed) {
            Some(match exit_code {
                Some(code) => format!("Command exited with code {}", code),
                None => "Detached command exited without a readable exit code".to_string(),
            })
        } else {
            None
        };

        status.status = final_status.clone();
        status.exit_code = exit_code;
        status.error = final_error.clone();
        status.completed_at = Some(completed_at.to_rfc3339());
        status.duration_secs = duration_secs;
        status.pid = Some(pid);
        push_task_event(
            &mut status,
            terminal_event_record(final_status.clone(), exit_code, final_error.as_deref()),
        );

        self.write_status_file(status_path, &status).await;

        let output_preview = if output.len() > 500 {
            format!("{}...", crate::util::truncate_str(&output, 500))
        } else {
            output
        };
        Bus::global().publish(BusEvent::BackgroundTaskCompleted(BackgroundTaskCompleted {
            task_id: status.task_id.clone(),
            tool_name: status.tool_name.clone(),
            display_name: status.display_name.clone(),
            session_id: status.session_id.clone(),
            status: final_status,
            exit_code,
            output_preview,
            output_file: output_path,
            duration_secs: duration_secs.unwrap_or(0.0),
            notify: status.notify,
            wake: status.wake,
        }));

        status
    }
}
