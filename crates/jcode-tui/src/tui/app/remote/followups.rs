use super::queue_recovery::recover_stranded_soft_interrupts;
use super::*;
use crate::tui::app::{commands, commands_dispatch, helpers};

pub(in crate::tui::app) async fn process_remote_followups(
    app: &mut App,
    remote: &mut RemoteConnection,
) {
    // A pending *server* reload must be dispatched even when the bootstrap
    // History payload was intentionally deferred. The runtime-identity /
    // stale-binary guard in the History handler sets `pending_server_reload =
    // true` and returns WITHOUT marking history as loaded, by design: we want to
    // reload the server before applying any session state. If the
    // `has_loaded_history` gate below ran first, the reload would never fire,
    // history would stay unloaded forever, and every typed prompt would be stuck
    // behind "Loading session..." until the user restarted (server/client binary
    // mismatch reload-handoff stall).
    if app.pending_server_reload && !app.is_processing {
        dispatch_pending_server_reload(app, remote).await;
        return;
    }

    // A headed fork stages its first prompt before launching the new client. We
    // can send that prompt immediately after Subscribe, without waiting for the
    // client to receive and render History: requests and events share one
    // ordered socket, so the server finishes writing the Subscribe History
    // response before it reads this Message request. Do not echo the user turn
    // locally here because the still-in-flight History payload would clear it;
    // the server's ordered Transcript event will add it immediately afterwards.
    //
    // This removes the visible, intermittent pause between the fork window
    // opening and its prompt starting, which was proportional to history payload
    // transfer/render time for large parent sessions.
    if !remote.has_loaded_history()
        && app.submit_input_on_startup
        && !app.is_processing
        && !app.remote_model_switch_in_flight
        && !app.auth_catalog_refresh_pending
        && (!app.input.is_empty() || !app.pending_images.is_empty())
    {
        app.submit_input_on_startup = false;
        app.startup_submit_deferred_reason = None;
        let prepared = input::take_prepared_input(app);
        app.last_submitted_input = Some(prepared.raw_input);
        crate::logging::info(&format!(
            "Startup auto-submit sent behind ordered Subscribe: input_chars={} pending_images={}",
            prepared.expanded.chars().count(),
            prepared.images.len(),
        ));
        if let Err(error) = begin_remote_send(
            app,
            remote,
            prepared.expanded,
            prepared.images,
            false,
            None,
            false,
            0,
        )
        .await
        {
            crate::logging::warn(&format!("Early startup auto-submit failed: {error}"));
            app.push_display_message(DisplayMessage::error(format!(
                "Failed to submit startup prompt: {}",
                error
            )));
            app.set_status_notice("Startup prompt failed");
        }
        return;
    }

    if !remote.has_loaded_history() {
        note_startup_submit_deferred(app, "remote history not loaded yet");
        return;
    }

    let _ = recover_stranded_soft_interrupts(app, remote).await;

    if app.pending_queued_dispatch {
        note_startup_submit_deferred(app, "pending_queued_dispatch in progress");
        return;
    }

    if !app.remote_model_switch_in_flight
        && !app.is_processing
        && let Some(payload) = app.pending_fallback_resend.take()
    {
        // A fallback offer was accepted and the server confirmed the route
        // switch: resend the failed turn's payload on the new route. The
        // original user message is already in the transcript from the failed
        // attempt, so do not echo it again; just clear the input box if the
        // error path restored the prompt there.
        if let Some(raw_input) = payload.raw_input.as_deref()
            && app.input == raw_input
        {
            app.input.clear();
            app.cursor_pos = 0;
        }
        app.last_submitted_input = payload.raw_input.clone();
        crate::logging::info("Resending failed turn after accepted fallback route switch");
        if let Err(error) = begin_remote_send(
            app,
            remote,
            payload.content,
            payload.images,
            payload.is_system,
            payload.system_reminder,
            payload.auto_retry,
            0,
        )
        .await
        {
            app.push_display_message(DisplayMessage::error(format!(
                "Failed to resend after fallback switch: {}",
                error
            )));
            app.set_status_notice("Fallback resend failed");
        }
        return;
    }

    if !app.remote_model_switch_in_flight
        && !app.auth_catalog_refresh_pending
        && !app.is_processing
        && let Some(prepared) = app.pending_prompt_after_model_switch.take()
    {
        if let Err(error) = submit_prepared_remote_input(app, remote, prepared).await {
            app.push_display_message(DisplayMessage::error(format!(
                "Failed to submit prompt after model switch: {}",
                error
            )));
            app.set_status_notice("Queued prompt failed");
        }
        return;
    }

    // Now that history is loaded (guaranteed by the gate above), dispatch any
    // prompt that the user submitted during the pre-history window. Submitting
    // it earlier would have been clobbered by the bootstrap History payload's
    // `session_changed` clear; deferring here fixes the intermittent
    // "first prompt vanishes / weird render" bug.
    if !app.is_processing
        && let Some(prepared) = app.pending_prompt_before_history.take()
    {
        crate::logging::info(
            "Dispatching prompt that was held until remote history finished loading",
        );
        if let Err(error) = submit_prepared_remote_input(app, remote, prepared).await {
            app.push_display_message(DisplayMessage::error(format!(
                "Failed to submit prompt after session load: {}",
                error
            )));
            app.set_status_notice("Prompt failed");
        }
        return;
    }

    let synthetic_startup_dispatch = app.is_processing
        && app.current_message_id.is_none()
        && app.remote_resume_activity.is_none()
        && (app.submit_input_on_startup
            || !app.queued_messages.is_empty()
            || !app.hidden_queued_system_messages.is_empty());

    if synthetic_startup_dispatch {
        crate::logging::info(
            "Dispatching restored startup/queued followup without active remote message id",
        );
        app.is_processing = false;
        app.status = ProcessingStatus::Idle;
        app.processing_started = None;
        app.clear_visible_turn_started();
        app.replay_processing_started_ms = None;
        app.replay_elapsed_override = None;
    }

    if app.submit_input_on_startup && !app.is_processing {
        app.submit_input_on_startup = false;
        app.startup_submit_deferred_reason = None;
        if !app.input.is_empty() || !app.pending_images.is_empty() {
            crate::logging::info(&format!(
                "Startup auto-submit firing: input_chars={} pending_images={}",
                app.input.chars().count(),
                app.pending_images.len(),
            ));
            let prepared = input::take_prepared_input(app);
            if let Err(error) = submit_prepared_remote_input(app, remote, prepared).await {
                crate::logging::warn(&format!("Startup auto-submit failed: {error}"));
                app.push_display_message(DisplayMessage::error(format!(
                    "Failed to submit startup prompt: {}",
                    error
                )));
                app.set_status_notice("Startup prompt failed");
            }
            return;
        } else {
            crate::logging::warn(
                "Startup auto-submit skipped: submit flag was set but input and pending images are both empty",
            );
        }
    } else if app.submit_input_on_startup && app.is_processing {
        note_startup_submit_deferred(app, "session still processing (is_processing=true)");
    }

    if app.pending_background_client_reload.is_some() && !app.is_processing {
        app.maybe_finish_background_client_reload();
        return;
    }

    if app.pending_split_request && !app.is_processing {
        app.pending_split_request = false;
        let flow_label = app
            .pending_split_label
            .clone()
            .unwrap_or_else(|| "Split".to_string());
        begin_remote_split_launch(app, &flow_label);
        if let Err(error) = remote.split().await {
            finish_remote_split_launch(app);
            let had_startup = app.pending_split_startup_message.take().is_some();
            app.pending_split_parent_session_id = None;
            let had_prompt = app.pending_split_prompt.take().is_some();
            let label = app.pending_split_label.take();
            app.pending_split_model_override = None;
            app.pending_split_provider_key_override = None;
            let flow_label = label.unwrap_or(flow_label);
            app.push_display_message(DisplayMessage::error(format!(
                "Failed to launch {} session: {}",
                flow_label.to_lowercase(),
                error
            )));
            if had_startup || had_prompt {
                app.set_status_notice(format!("{} launch failed", flow_label));
            }
        }
        return;
    }

    if app.pending_transfer_request && !app.is_processing {
        app.pending_transfer_request = false;
        let flow_label = app
            .pending_split_label
            .clone()
            .unwrap_or_else(|| "Transfer".to_string());
        begin_remote_split_launch(app, &flow_label);
        if let Err(error) = remote.transfer().await {
            finish_remote_split_launch(app);
            let label = app.pending_split_label.take().unwrap_or(flow_label);
            app.push_display_message(DisplayMessage::error(format!(
                "Failed to launch {} session: {}",
                label.to_lowercase(),
                error
            )));
            app.set_status_notice(format!("{} launch failed", label));
        }
        return;
    }

    if app.is_processing {
        if let Some(interleave_msg) = app.interleave_message.take()
            && !interleave_msg.trim().is_empty()
        {
            let interleave_images = std::mem::take(&mut app.interleave_images);
            let msg_clone = interleave_msg.clone();
            let expanded = match super::input::expand_file_mentions_for_submit(app, &interleave_msg)
            {
                Ok(expanded) => expanded,
                Err(notice) => {
                    app.input = interleave_msg;
                    app.cursor_pos = app.input.len();
                    app.pending_images.extend(interleave_images);
                    app.set_status_notice(notice.clone());
                    app.push_display_message(DisplayMessage::system(notice));
                    return;
                }
            };
            match remote
                .soft_interrupt(expanded, interleave_images, false)
                .await
            {
                Err(e) => {
                    app.push_display_message(DisplayMessage::error(format!(
                        "Failed to queue soft interrupt: {}",
                        e
                    )));
                }
                Ok(request_id) => {
                    app.track_pending_soft_interrupt(request_id, msg_clone);
                }
            }
        }
        return;
    }

    if let Some(interleave_msg) = app.interleave_message.take() {
        // Carry the staged attachments through. A local revert of #627 had this
        // passing `vec![]`, which silently dropped every image on an interleaved
        // send while still compiling, so the comment marks why the take matters.
        let interleave_images = std::mem::take(&mut app.interleave_images);
        if !interleave_msg.trim().is_empty() {
            let expanded = match super::input::expand_file_mentions_for_submit(app, &interleave_msg)
            {
                Ok(expanded) => expanded,
                Err(notice) => {
                    app.input = interleave_msg;
                    app.cursor_pos = app.input.len();
                    app.pending_images.extend(interleave_images);
                    app.set_status_notice(notice.clone());
                    app.push_display_message(DisplayMessage::system(notice));
                    return;
                }
            };
            app.push_display_message(DisplayMessage {
                role: "user".to_string(),
                content: interleave_msg.clone(),
                tool_calls: vec![],
                duration_secs: None,
                title: None,
                tool_data: None,
            });
            if let Err(e) = begin_remote_send(
                app,
                remote,
                expanded,
                interleave_images,
                false,
                None,
                false,
                0,
            )
            .await
            {
                app.push_display_message(DisplayMessage::error(format!(
                    "Failed to send message: {}",
                    e
                )));
            }
        }
    } else if !app.queued_messages.is_empty() {
        let queued_messages = std::mem::take(&mut app.queued_messages);
        let hidden_reminders = std::mem::take(&mut app.hidden_queued_system_messages);
        let (messages, reminder, display_system_messages) =
            helpers::partition_queued_messages(queued_messages, hidden_reminders);
        let combined = messages.join("\n\n");
        let preserve_visible_turn = commands::queued_messages_are_only_pokes(&messages);
        let auto_retry = reminder.is_some() && messages.is_empty();
        for msg in display_system_messages {
            app.push_display_message(DisplayMessage::system(msg));
        }
        for msg in &messages {
            if !commands::is_poke_message(msg) {
                app.push_display_message(DisplayMessage::user(msg.clone()));
            }
        }
        if !combined.is_empty() {
            if preserve_visible_turn {
                app.visible_turn_started.get_or_insert_with(Instant::now);
            } else {
                app.visible_turn_started = Some(Instant::now());
            }
        }
        let expanded = match super::input::expand_file_mentions_for_submit(app, &combined) {
            Ok(expanded) => expanded,
            Err(notice) => {
                if let Some(reminder) = reminder {
                    app.hidden_queued_system_messages.insert(0, reminder);
                }
                app.input = combined;
                app.cursor_pos = app.input.len();
                app.set_status_notice(notice.clone());
                app.push_display_message(DisplayMessage::system(notice));
                return;
            }
        };
        if begin_remote_send(
            app,
            remote,
            expanded,
            vec![],
            true,
            reminder.clone(),
            auto_retry,
            0,
        )
        .await
        .is_err()
        {
            // Do not drop a dequeued follow-up whose send never reached the
            // server (issue #391); restore it for redispatch after reconnect.
            crate::logging::error(
                "Failed to send queued continuation message; restoring it to the queue",
            );
            if let Some(reminder) = reminder {
                app.hidden_queued_system_messages.insert(0, reminder);
            }
            if !combined.is_empty() {
                app.queued_messages.insert(0, combined);
            }
        }
    } else if !app.hidden_queued_system_messages.is_empty() {
        let reminders = std::mem::take(&mut app.hidden_queued_system_messages);
        let combined = reminders.join("\n\n");
        if begin_remote_send(
            app,
            remote,
            String::new(),
            vec![],
            true,
            Some(combined.clone()),
            true,
            0,
        )
        .await
        .is_err()
        {
            crate::logging::error(
                "Failed to send hidden continuation reminder; restoring it to the queue",
            );
            app.hidden_queued_system_messages.insert(0, combined);
        }
    }
}

/// How long a queued follow-up may sit undispatched on an idle client before
/// the starvation watchdog treats it as stranded.
pub(super) const QUEUED_FOLLOWUP_STARVATION_TIMEOUT: Duration = Duration::from_secs(30);

/// Recover the "👉 Auto-poking: N incomplete todos" + spinner-forever state
/// where no request is actually in flight.
///
/// `schedule_auto_poke_followup_if_needed` pushes the continuation onto
/// `queued_messages` and sets `pending_queued_dispatch`. The event loop clears
/// that flag and calls `process_remote_followups`, which returns early WITHOUT
/// sending whenever one of its gates is closed (history not loaded, an earlier
/// pending prompt/split/transfer branch returning first, or `is_processing`
/// still true from a turn whose terminal event was dropped). The flag is
/// already consumed by then, and nothing re-arms it: the follow-up sits in
/// `queued_messages`, `App::is_processing()` keeps reporting true because the
/// queue is non-empty, and the spinner spins while the model is idle.
///
/// `detect_and_cancel_stall` does not cover this: it only runs while
/// `app.is_processing`, which is false in this variant. So track how long a
/// queued follow-up has been idle-but-undispatched and re-arm the dispatch past
/// the timeout, logging it so a recurrence is diagnosable from logs alone.
pub(super) fn detect_starved_queued_followup(app: &mut App) -> bool {
    let starved_candidate =
        !app.is_processing && !app.pending_queued_dispatch && app.has_queued_followups();
    if !starved_candidate {
        app.queued_followup_starved_since = None;
        return false;
    }
    let since = *app
        .queued_followup_starved_since
        .get_or_insert_with(Instant::now);
    let idle_for = since.elapsed();
    if idle_for < QUEUED_FOLLOWUP_STARVATION_TIMEOUT {
        return false;
    }
    crate::logging::warn(&format!(
        "QUEUED_FOLLOWUP_STARVED queued_messages={} hidden_reminders={} interleave={} idle_for_secs={} re-arming dispatch",
        app.queued_messages.len(),
        app.hidden_queued_system_messages.len(),
        app.interleave_message.is_some(),
        idle_for.as_secs(),
    ));
    app.queued_followup_starved_since = None;
    app.pending_queued_dispatch = true;
    true
}

/// Client-side stall budget before the TUI cancels an in-flight turn.
///
/// The server relays provider events over the local socket; when the upstream
/// model reasons silently, no events cross the socket, so a hardcoded short
/// watchdog cannot distinguish a dead connection from a healthy long think
/// (issue #434). Derive it from `[provider] stream_idle_timeout_secs`, scaled by
/// the largest reasoning-effort multiplier because effort is invisible here,
/// plus grace so the server-side idle timeout (which produces a visible error
/// event) always fires first. Never below 2 minutes.
pub(super) fn stall_timeout() -> Duration {
    const MIN_STALL_TIMEOUT: Duration = Duration::from_secs(2 * 60);
    const GRACE: Duration = Duration::from_secs(30);
    let provider_idle = crate::provider::max_stream_idle_timeout();
    provider_idle.saturating_add(GRACE).max(MIN_STALL_TIMEOUT)
}

/// Human-readable stall duration for user-facing stall messages, e.g.
/// "2 minutes", "3.5 minutes", or "90 seconds".
pub(super) fn format_stall_duration(timeout: Duration) -> String {
    let secs = timeout.as_secs();
    if secs < 120 {
        format!("{} seconds", secs)
    } else if secs.is_multiple_of(60) {
        format!("{} minutes", secs / 60)
    } else {
        format!("{:.1} minutes", secs as f64 / 60.0)
    }
}

pub(super) async fn detect_and_cancel_stall(app: &mut App, remote: &mut RemoteConnection) {
    let stall_timeout = stall_timeout();
    let is_running_tool = matches!(app.status, ProcessingStatus::RunningTool(_));
    if app.is_processing && !is_running_tool {
        let stalled = app
            .last_stream_activity
            .map(|t| t.elapsed() > stall_timeout)
            .unwrap_or_else(|| {
                app.processing_started
                    .map(|t| t.elapsed() > stall_timeout)
                    .unwrap_or(false)
            });
        if stalled {
            if let Some(snapshot) = app.remote_resume_activity.clone() {
                let elapsed = app
                    .last_stream_activity
                    .map(|t| t.elapsed())
                    .or(app.processing_started.map(|t| t.elapsed()));
                crate::logging::warn(&format!(
                    "Protocol stall guard: resumed session {} is still marked processing by history snapshot (tool={:?}, snapshot_age={:?}) but no corroborating live events arrived after {:?}; deferring client-side cancel",
                    snapshot.session_id,
                    snapshot.current_tool_name,
                    snapshot.observed_at.elapsed(),
                    elapsed
                ));
                app.last_stream_activity = Some(Instant::now());
                app.status = match snapshot.current_tool_name {
                    Some(tool_name) => ProcessingStatus::RunningTool(tool_name),
                    None => ProcessingStatus::Thinking(Instant::now()),
                };
                return;
            }
            crate::logging::warn(&format!(
                "Stream stall detected: no server events for {:?}, cancelling",
                app.last_stream_activity
                    .map(|t| t.elapsed())
                    .or(app.processing_started.map(|t| t.elapsed()))
            ));
            let _ = remote.cancel_with_reason("stall_guard").await;
            app.is_processing = false;
            app.clear_visible_turn_started();
            app.status = ProcessingStatus::Idle;
            app.current_message_id = None;
            app.processing_started = None;
            app.last_stream_activity = None;
            if !app.streaming.streaming_text.is_empty() {
                let content = app.take_streaming_text();
                let content = app.collapse_reasoning_for_commit(content);
                if !content.trim().is_empty() {
                    app.push_display_message(DisplayMessage {
                        role: "assistant".to_string(),
                        content,
                        tool_calls: vec![],
                        duration_secs: None,
                        title: None,
                        tool_data: None,
                    });
                }
            }
            let stall_desc = format_stall_duration(stall_timeout);
            if !app.schedule_pending_remote_retry(&format!(
                "⚠ Stream stalled (no response for {stall_desc}). Processing cancelled.",
            )) {
                // Keep a dispatched-but-unfinished queued follow-up on the
                // queue instead of silently dropping it (issue #391).
                let recovered = recover_undelivered_queued_continuation(app, "stream stall");
                app.clear_pending_remote_retry();
                if recovered {
                    app.push_display_message(DisplayMessage::system(format!(
                        "⚠ Stream stalled (no response for {stall_desc}). Processing cancelled. Your queued follow-up stays queued.",
                    )));
                } else {
                    app.push_display_message(DisplayMessage::system(format!(
                        "⚠ Stream stalled (no response for {stall_desc}). Processing cancelled. You can resend your message. Raise `[provider] stream_idle_timeout_secs` in config.toml if your model thinks silently for longer.",
                    )));
                }
            }
        }
    }
}

pub(super) fn handle_mouse_event(app: &mut App, mouse: MouseEvent) {
    app.handle_mouse_event(mouse);
}

pub(super) async fn handle_debug_command(
    app: &mut App,
    cmd: &str,
    remote: &mut RemoteConnection,
) -> String {
    let cmd = cmd.trim();
    if cmd.starts_with("message:") {
        let msg = cmd.strip_prefix("message:").unwrap_or("");
        app.input = msg.to_string();
        let result = handle_remote_key(app, KeyCode::Enter, KeyModifiers::empty(), remote).await;
        if let Err(e) = result {
            return format!("ERR: {}", e);
        }
        app.debug_trace
            .record("message", format!("submitted:{}", msg));
        return format!("OK: queued message '{}'", msg);
    }
    if cmd == "reload" {
        app.input = "/reload".to_string();
        let result = handle_remote_key(app, KeyCode::Enter, KeyModifiers::empty(), remote).await;
        if let Err(e) = result {
            return format!("ERR: {}", e);
        }
        app.debug_trace.record("reload", "triggered".to_string());
        return "OK: reload triggered".to_string();
    }
    if cmd == "state" {
        return serde_json::json!({
            "processing": app.is_processing,
            "messages": app.messages.len(),
            "display_messages": app.display_messages.len(),
            "input": app.input,
            "cursor_pos": app.cursor_pos,
            "scroll_offset": app.scroll_offset,
            "queued_messages": app.queued_messages.len(),
            "provider_session_id": app.provider_session_id,
            "provider_name": app.remote_provider_name.clone(),
            "model": app.remote_provider_model.as_deref().unwrap_or(app.provider.name()),
            "connection_type": app.connection_type.clone(),
            "remote_transport": app.remote_transport.clone(),
            "diagram_mode": format!("{:?}", app.diagram_mode),
            "diagram_focus": app.diagram_focus,
            "diagram_index": app.diagram_index,
            "diagram_scroll": [app.diagram_scroll_x, app.diagram_scroll_y],
            "diagram_pane_ratio": app.diagram_pane_ratio_target,
            "diagram_pane_enabled": app.diagram_pane_enabled,
            "diagram_pane_position": format!("{:?}", app.diagram_pane_position),
            "diagram_zoom": app.diagram_zoom,
            "diagram_count": crate::tui::mermaid::get_active_diagrams().len(),
            "remote": true,
            "server_version": app.remote_server_version.clone(),
            "server_has_update": app.remote_server_has_update,
            "version": jcode_build_meta::version(),
            "diagram_mode": format!("{:?}", app.diagram_mode),
        })
        .to_string();
    }
    if cmd.starts_with("keys:") {
        let keys_str = cmd.strip_prefix("keys:").unwrap_or("");
        let mut results = Vec::new();
        for key_spec in keys_str.split(',') {
            match parse_and_inject_key(app, key_spec.trim(), remote).await {
                Ok(desc) => {
                    app.debug_trace.record("key", desc.clone());
                    results.push(format!("OK: {}", desc));
                }
                Err(e) => results.push(format!("ERR: {}", e)),
            }
        }
        return results.join("\n");
    }
    if cmd == "submit" {
        if app.input.is_empty() {
            return "submit error: input is empty".to_string();
        }
        let result = handle_remote_key(app, KeyCode::Enter, KeyModifiers::empty(), remote).await;
        if let Err(e) = result {
            return format!("ERR: {}", e);
        }
        app.debug_trace.record("input", "submitted".to_string());
        return "OK: submitted".to_string();
    }
    if cmd.starts_with("run:") || cmd.starts_with("script:") {
        return "ERR: script/run not supported in remote debug mode".to_string();
    }
    app.handle_debug_command(cmd)
}

pub(super) async fn parse_and_inject_key(
    app: &mut App,
    key_spec: &str,
    remote: &mut RemoteConnection,
) -> std::result::Result<String, String> {
    let (key_code, modifiers) = app.parse_key_spec(key_spec)?;
    handle_remote_key(app, key_code, modifiers, remote)
        .await
        .map_err(|e| e.to_string())?;
    Ok(format!("injected {:?} with {:?}", key_code, modifiers))
}

fn handle_disconnected_local_command(app: &mut App, trimmed: &str) -> bool {
    let handled = commands_dispatch::dispatch_local_command(app, trimmed);

    if handled {
        if trimmed.starts_with('/') {
            crate::telemetry::record_command_family(trimmed);
        }
        app.input.clear();
        app.cursor_pos = 0;
        app.reset_tab_completion();
        app.sync_model_picker_preview_from_input();
        app.clear_input_undo_history();
    }

    handled
}

pub(super) fn queue_message_for_reconnect(app: &mut App) {
    input::promote_dropped_images(app);
    let trimmed = app.input.trim().to_string();
    if trimmed.is_empty() {
        return;
    }

    if trimmed.starts_with('/') {
        if handle_disconnected_local_command(app, &trimmed) {
            return;
        }
        app.set_status_notice("This command requires a live connection");
        return;
    }

    let prepared = input::take_prepared_input(app);
    app.queued_messages.push(prepared.expanded);

    let queued_count = app.queued_messages.len();
    app.set_status_notice(format!(
        "Queued for send after reconnect ({} message{})",
        queued_count,
        if queued_count == 1 { "" } else { "s" }
    ));
}
