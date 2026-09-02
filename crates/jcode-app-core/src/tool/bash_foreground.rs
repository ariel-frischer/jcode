use super::*;

impl BashTool {
    pub(super) async fn execute_foreground(
        &self,
        params: &BashInput,
        ctx: &ToolContext,
        policy: BashExecutionPolicy,
    ) -> Result<ToolOutput> {
        #[cfg(unix)]
        if self.supports_reload_persistence(ctx) {
            return self
                .execute_reload_persistable_foreground(params, ctx, policy)
                .await;
        }

        let hard_timeout = policy.hard_timeout;
        let has_stdin_channel = ctx.stdin_request_tx.is_some();
        let mut command = build_shell_command(&params.command);
        #[cfg(unix)]
        process_guard::isolate_process_group(&mut command);
        command
            .kill_on_drop(true)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if has_stdin_channel {
            command.stdin(Stdio::piped());
        }
        if let Some(ref dir) = ctx.working_dir {
            command.current_dir(dir);
        }
        let mut child = command.spawn()?;
        let child_pid = child.id().unwrap_or(0);
        let hard_timeout_deadline = policy
            .hard_timeout
            .map(|duration| tokio::time::Instant::now() + duration);
        #[cfg(unix)]
        let mut foreground_guard = ForegroundProcessGuard::new(child.id());
        let stdin_handle = child.stdin.take();
        let stdout_handle = child.stdout.take();
        let stderr_handle = child.stderr.take();

        // Owned copies let soft yield move the work to the background.
        let title = params
            .intent
            .clone()
            .unwrap_or_else(|| params.command.clone());
        let stdin_tx = ctx.stdin_request_tx.clone();
        let tool_call_id = ctx.tool_call_id.clone();
        let title_for_work = title.clone();
        // Track progress parsed from output so a soft-yield adoption starts the
        // background task at the real percentage instead of 0%.
        let promoted_progress = std::sync::Arc::new(PromotedCommandProgress::default());
        let stdout_progress = std::sync::Arc::clone(&promoted_progress);
        let stderr_progress = std::sync::Arc::clone(&promoted_progress);

        // A dedicated task can be handed to the background manager on timeout
        // instead of killing the still-running command.
        let mut work_handle: tokio::task::JoinHandle<Result<ToolOutput>> =
            tokio::spawn(async move {
                #[cfg(unix)]
                let mut process_group_guard = ProcessGroupKillGuard::new(child.id());
                let stdout_task = tokio::spawn(collect_output_reporting_progress(
                    stdout_handle,
                    stdout_progress,
                ));

                let stderr_task = tokio::spawn(collect_output_reporting_progress(
                    stderr_handle,
                    stderr_progress,
                ));

                let stdin_task = if has_stdin_channel {
                    Some(tokio::spawn(async move {
                        if let (Some(mut stdin_pipe), Some(stdin_tx)) = (stdin_handle, stdin_tx) {
                            tokio::time::sleep(Duration::from_millis(STDIN_INITIAL_DELAY_MS)).await;

                            let mut request_counter = 0u32;
                            loop {
                                #[cfg(target_os = "linux")]
                                let state = stdin_detect::linux::check_process_tree(child_pid);
                                #[cfg(not(target_os = "linux"))]
                                let state = stdin_detect::is_waiting_for_stdin(child_pid);

                                if state == StdinState::Reading {
                                    request_counter += 1;
                                    let request_id =
                                        format!("stdin-{}-{}", tool_call_id, request_counter);
                                    let (response_tx, response_rx) =
                                        tokio::sync::oneshot::channel();

                                    let request = StdinInputRequest {
                                        request_id,
                                        prompt: String::new(),
                                        is_password: false,
                                        response_tx,
                                    };

                                    if stdin_tx.send(request).is_err() {
                                        break;
                                    }

                                    match response_rx.await {
                                        Ok(input) => {
                                            let line = if input.ends_with('\n') {
                                                input
                                            } else {
                                                format!("{}\n", input)
                                            };
                                            if stdin_pipe.write_all(line.as_bytes()).await.is_err()
                                            {
                                                break;
                                            }
                                            if stdin_pipe.flush().await.is_err() {
                                                break;
                                            }
                                        }
                                        Err(_) => break,
                                    }

                                    tokio::time::sleep(Duration::from_millis(100)).await;
                                } else {
                                    tokio::time::sleep(Duration::from_millis(
                                        STDIN_POLL_INTERVAL_MS,
                                    ))
                                    .await;
                                }
                            }
                        }
                    }))
                } else {
                    drop(stdin_handle);
                    None
                };

                let timeout_sleep = hard_timeout_deadline.map(tokio::time::sleep_until);
                tokio::pin!(timeout_sleep);
                let mut timed_out = false;
                let status = tokio::select! {
                    status = child.wait() => status?,
                    _ = async {
                        match timeout_sleep.as_mut().as_pin_mut() {
                            Some(sleep) => sleep.await,
                            None => std::future::pending().await,
                        }
                    }, if hard_timeout.is_some() => {
                        timed_out = true;
                        #[cfg(unix)]
                        {
                            if process_group_guard.terminate_verified()
                                != crate::platform::ProcessIdentityCheck::Matching
                            {
                                crate::logging::info(&format!(
                                    "failed to terminate timed-out bash process group {child_pid}"
                                ));
                            }
                        }
                        #[cfg(not(unix))]
                        {
                            if let Err(err) = child.start_kill() {
                                crate::logging::info(&format!(
                                    "failed to terminate timed-out bash process: {err}"
                                ));
                            }
                        }
                        child.wait().await?
                    }
                };
                #[cfg(unix)]
                process_group_guard.disarm();

                if let Some(task) = stdin_task {
                    task.abort();
                }

                let stdout = stdout_task.await.unwrap_or_else(|_| String::new());
                let stderr = stderr_task.await.unwrap_or_else(|_| String::new());

                let mut output = String::new();
                if !stdout.is_empty() {
                    output.push_str(&stdout);
                }
                if !stderr.is_empty() {
                    if !output.is_empty() {
                        output.push('\n');
                    }
                    output.push_str(&stderr);
                }
                let exit_code = if timed_out { Some(124) } else { status.code() };
                if timed_out {
                    let timeout_ms = hard_timeout.map(duration_millis_u64).unwrap_or(0);
                    if !output.is_empty() {
                        output.push('\n');
                    }
                    output.push_str(&timeout_message(timeout_ms));
                }
                let output = format_command_output(output, exit_code);
                let mut tool_output = ToolOutput::new(output).with_title(title_for_work);
                if exit_code.is_some_and(|code| code != 0) {
                    tool_output = tool_output.with_metadata(json!({
                        "exit_code": exit_code,
                        "timed_out": timed_out,
                    }));
                }
                Ok(tool_output)
            });
        #[cfg(unix)]
        foreground_guard.attach_task(work_handle.abort_handle());

        let join_result = if let Some(soft_yield) = policy.soft_yield {
            tokio::time::timeout(soft_yield, &mut work_handle)
                .await
                .map_or(None, Some)
        } else {
            Some((&mut work_handle).await)
        };

        match join_result {
            Some(join_result) => {
                #[cfg(unix)]
                foreground_guard.disarm();
                match join_result {
                    Ok(Ok(output)) => Ok(output),
                    Ok(Err(e)) => Err(anyhow::anyhow!("Command failed: {}", e)),
                    Err(join_err) => Err(anyhow::anyhow!("Command task panicked: {}", join_err)),
                }
            }
            None => {
                // The soft-yield window elapsed while the original command stayed
                // healthy. Adopt that exact work future without killing or respawning it.
                let display_name =
                    summarize_background_command(params.intent.as_deref(), &params.command);
                let managed_process = Some(crate::background::ManagedProcessIdentity {
                    pid: child_pid,
                    process_instance: crate::platform::process_start_token(child_pid),
                    process_group_member: crate::platform::process_group_member_identity(child_pid)
                        .map(
                            |(pid, token)| crate::background::ManagedProcessMemberIdentity {
                                pid,
                                process_instance: Some(token),
                            },
                        ),
                    owner_instance: Some(crate::background::process_instance_token().to_string()),
                    transfer_policy: crate::background::ManagedProcessTransferPolicy::OwnerBound,
                });
                let info = crate::background::global()
                    .adopt_with_options_and_identity(
                        "bash",
                        Some(display_name.clone()),
                        &ctx.session_id,
                        params.requested_notify(),
                        params.soft_yield_wake(),
                        work_handle,
                        managed_process,
                    )
                    .await;
                if let Some(stall_wake_seconds) = params.stall_wake_seconds {
                    let _armed_stall_wake = crate::background::global()
                        .arm_stall_watchdog(&info.task_id, stall_wake_seconds)
                        .await;
                }
                #[cfg(unix)]
                // Keep the task-local kill guard armed for explicit cancellation,
                // but remove transferred work from foreground runtime cleanup.
                foreground_guard.disarm();
                // Route progress parsed from the still-running command's output
                // to the new background task, including any update seen before
                // promotion, so the task row shows real progress from the start.
                promoted_progress.attach_task(&info.task_id).await;

                let output = format!(
                    "Command soft-yielded after {:.1}s and is continuing in background (not killed).\n\n\
                     Task ID: {}\n\
                     Name: {}\n\
                     Output file: {}\n\
                     Status file: {}\n\n\
                     The command is still running; do not rerun it unless you intentionally want a second copy.\n\
                     Use `bg` with action=\"wait\" and task_id=\"{}\" to wait for completion or the next progress checkpoint.\n\
                     Use `bg` with action=\"output\" and task_id=\"{}\" to inspect output.\n\
                     Soft yield only returns control; an explicit `timeout` remains a hard deadline with exit status 124.",
                    policy
                        .soft_yield
                        .map_or(0.0, |duration| duration.as_secs_f64()),
                    info.task_id,
                    display_name,
                    info.output_file.display(),
                    info.status_file.display(),
                    info.task_id,
                    info.task_id,
                );

                Ok(ToolOutput::new(output)
                    .with_title(title)
                    .with_metadata(json!({
                        "background": true,
                        "task_id": info.task_id,
                        "display_name": display_name,
                        "output_file": info.output_file.to_string_lossy(),
                        "status_file": info.status_file.to_string_lossy(),
                        "soft_yielded": true,
                        "soft_yield_ms": policy.soft_yield.map(|duration| duration.as_millis()),
                    })))
            }
        }
    }
}
