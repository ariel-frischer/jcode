use super::super::App;
use crate::protocol::{
    QueuedMessageEditorDirection, QueuedMessageEditorOperation, QueuedMessageEditorOutcome,
    QueuedMessageEditorPlacement, QueuedMessageEditorSelection, RecallableSoftInterrupt,
};
use crate::tui::backend::RemoteConnection;

/// Owner-scoped client state for one authoritative queued-message editor session.
///
/// The active navigation and pending operation identities deliberately survive
/// transport loss. A reconnect can therefore retry the same operation without
/// accepting a stale or foreign result into the composer.
#[derive(Debug, Default)]
pub(in crate::tui::app) struct QueuedMessageEditorClientState {
    navigation_session_id: Option<String>,
    selected_message_id: Option<String>,
    pending_operation_id: Option<String>,
    pending_operation: Option<QueuedMessageEditorOperation>,
}

impl QueuedMessageEditorClientState {
    fn begin_start(&mut self) -> Option<(String, String, QueuedMessageEditorOperation)> {
        if self.navigation_session_id.is_some() || self.pending_operation_id.is_some() {
            return None;
        }
        let navigation_session_id = format!(
            "tui-queued-message-navigation-{:032x}",
            rand::random::<u128>()
        );
        let operation_id = format!("tui-queued-message-start-{:032x}", rand::random::<u128>());
        let operation = QueuedMessageEditorOperation::Start;
        self.navigation_session_id = Some(navigation_session_id.clone());
        self.pending_operation_id = Some(operation_id.clone());
        self.pending_operation = Some(operation.clone());
        Some((navigation_session_id, operation_id, operation))
    }

    fn begin_move(
        &mut self,
        direction: QueuedMessageEditorDirection,
        draft: RecallableSoftInterrupt,
    ) -> Option<(String, String, QueuedMessageEditorOperation)> {
        if self.pending_operation_id.is_some() {
            return None;
        }
        let navigation_session_id = self.navigation_session_id.clone()?;
        let selected_message_id = self.selected_message_id.clone()?;
        let operation_id = format!("tui-queued-message-move-{:032x}", rand::random::<u128>());
        let operation = QueuedMessageEditorOperation::Move {
            direction,
            selected_message_id,
            draft,
        };
        self.pending_operation_id = Some(operation_id.clone());
        self.pending_operation = Some(operation.clone());
        Some((navigation_session_id, operation_id, operation))
    }

    /// Record the stable identities for an operation before it is sent.
    // The phase-4 result path lands before the follow-up request-routing tasks.
    #[allow(dead_code)]
    pub(in crate::tui::app) fn begin(
        &mut self,
        navigation_session_id: &str,
        operation_id: &str,
    ) -> bool {
        if navigation_session_id.trim().is_empty() || operation_id.trim().is_empty() {
            return false;
        }
        if self.pending_operation_id.is_some() {
            return false;
        }
        if self
            .navigation_session_id
            .as_deref()
            .is_some_and(|active| active != navigation_session_id)
        {
            return false;
        }
        self.navigation_session_id = Some(navigation_session_id.to_string());
        self.pending_operation_id = Some(operation_id.to_string());
        self.pending_operation = None;
        true
    }

    fn begin_finish(
        &mut self,
        draft: RecallableSoftInterrupt,
    ) -> Option<(String, String, QueuedMessageEditorOperation)> {
        if self.pending_operation_id.is_some() {
            return None;
        }
        let navigation_session_id = self.navigation_session_id.clone()?;
        let selected_message_id = self.selected_message_id.clone()?;
        let operation_id = format!("tui-queued-message-finish-{:032x}", rand::random::<u128>());
        let operation = QueuedMessageEditorOperation::Finish {
            selected_message_id,
            draft,
        };
        self.pending_operation_id = Some(operation_id.clone());
        self.pending_operation = Some(operation.clone());
        Some((navigation_session_id, operation_id, operation))
    }

    fn pending_request(&self) -> Option<(String, String, QueuedMessageEditorOperation)> {
        Some((
            self.navigation_session_id.clone()?,
            self.pending_operation_id.clone()?,
            self.pending_operation.clone()?,
        ))
    }

    fn set_selection(&mut self, message_id: String) {
        self.selected_message_id = Some(message_id);
    }

    fn accepts(&self, navigation_session_id: &str, operation_id: &str) -> bool {
        self.navigation_session_id.as_deref() == Some(navigation_session_id)
            && self.pending_operation_id.as_deref() == Some(operation_id)
    }

    fn complete_operation(&mut self, keep_session: bool) {
        self.pending_operation_id = None;
        self.pending_operation = None;
        if !keep_session {
            self.navigation_session_id = None;
            self.selected_message_id = None;
        }
    }

    pub(in crate::tui::app) fn is_active(&self) -> bool {
        self.navigation_session_id.is_some()
    }

    #[cfg(test)]
    pub(in crate::tui::app) fn has_pending_operation(&self) -> bool {
        self.pending_operation_id.is_some()
    }

    #[cfg(test)]
    pub(in crate::tui::app) fn activate_for_test(
        &mut self,
        navigation_session_id: &str,
        selected_message_id: &str,
    ) {
        self.navigation_session_id = Some(navigation_session_id.to_string());
        self.selected_message_id = Some(selected_message_id.to_string());
        self.pending_operation_id = None;
        self.pending_operation = None;
    }
}

pub(super) async fn start(app: &mut App, remote: &mut RemoteConnection) -> anyhow::Result<bool> {
    if app.remote_queued_message_editor.is_active()
        || !remote.supports_queued_message_navigation()
        || app.pending_soft_interrupts.is_empty()
        || !app.input.is_empty()
        || !app.pending_images.is_empty()
    {
        return Ok(false);
    }
    let Some((navigation_session_id, operation_id, operation)) =
        app.remote_queued_message_editor.begin_start()
    else {
        return Ok(true);
    };
    remote
        .queued_message_editor(&navigation_session_id, &operation_id, operation)
        .await?;
    app.set_status_notice("Opening queued message editor...");
    Ok(true)
}

async fn move_selection(
    app: &mut App,
    remote: &mut RemoteConnection,
    direction: QueuedMessageEditorDirection,
) -> anyhow::Result<bool> {
    if !app.remote_queued_message_editor.is_active() {
        return Ok(false);
    }
    let draft = RecallableSoftInterrupt {
        content: app.input.clone(),
        images: app.pending_images.clone(),
    };
    let Some((navigation_session_id, operation_id, operation)) = app
        .remote_queued_message_editor
        .begin_move(direction, draft)
    else {
        app.set_status_notice("Queued message navigation already pending...");
        return Ok(true);
    };
    remote
        .queued_message_editor(&navigation_session_id, &operation_id, operation)
        .await?;
    app.set_status_notice(match direction {
        QueuedMessageEditorDirection::Older => "Moving to older queued message...",
        QueuedMessageEditorDirection::Newer => "Moving to newer queued message...",
    });
    Ok(true)
}

pub(super) async fn move_older(
    app: &mut App,
    remote: &mut RemoteConnection,
) -> anyhow::Result<bool> {
    move_selection(app, remote, QueuedMessageEditorDirection::Older).await
}

pub(super) async fn move_newer(
    app: &mut App,
    remote: &mut RemoteConnection,
) -> anyhow::Result<bool> {
    move_selection(app, remote, QueuedMessageEditorDirection::Newer).await
}

fn apply_selection(app: &mut App, selection: QueuedMessageEditorSelection) {
    app.remote_queued_message_editor
        .set_selection(selection.message_id.clone());
    app.input = selection.content;
    app.cursor_pos = app.input.len();
    app.pending_images = selection.images;
}

/// Apply one authoritative result only when both stable identities match.
/// Boundary, conflict, malformed, and NotApplied results never overwrite the
/// composer, so the complete local draft remains available for retry/recovery.
pub(super) fn handle_server_result(
    app: &mut App,
    navigation_session_id: &str,
    operation_id: &str,
    outcome: QueuedMessageEditorOutcome,
    selection: Option<QueuedMessageEditorSelection>,
    placement: QueuedMessageEditorPlacement,
    server_message: Option<&str>,
) -> bool {
    if !app
        .remote_queued_message_editor
        .accepts(navigation_session_id, operation_id)
    {
        return false;
    }

    if placement == QueuedMessageEditorPlacement::NotApplied
        || outcome == QueuedMessageEditorOutcome::Conflict
    {
        app.remote_queued_message_editor.complete_operation(true);
        app.set_status_notice(
            server_message
                .unwrap_or("Queued message changed; your draft was not applied")
                .to_string(),
        );
        return true;
    }

    match outcome {
        QueuedMessageEditorOutcome::Started => {
            let Some(selection) = selection else {
                app.remote_queued_message_editor.complete_operation(true);
                app.set_status_notice(
                    "Queued message editor result was incomplete; draft preserved",
                );
                return true;
            };
            apply_selection(app, selection);
            app.remote_queued_message_editor.complete_operation(true);
            app.set_status_notice("Opened queued message editor");
        }
        QueuedMessageEditorOutcome::Moved => {
            let Some(selection) = selection else {
                app.remote_queued_message_editor.complete_operation(true);
                app.set_status_notice(
                    "Queued message navigation result was incomplete; draft preserved",
                );
                return true;
            };
            apply_selection(app, selection);
            app.remote_queued_message_editor.complete_operation(true);
            app.set_status_notice("Moved to queued message");
        }
        QueuedMessageEditorOutcome::Boundary => {
            app.remote_queued_message_editor.complete_operation(true);
            app.set_status_notice(
                server_message
                    .unwrap_or("Already at the queued message boundary")
                    .to_string(),
            );
        }
        QueuedMessageEditorOutcome::Committed => {
            app.remote_queued_message_editor.complete_operation(false);
            app.input.clear();
            app.cursor_pos = 0;
            app.pending_images.clear();
            app.set_status_notice("Updated queued message");
        }
        QueuedMessageEditorOutcome::Deleted => {
            app.remote_queued_message_editor.complete_operation(false);
            app.input.clear();
            app.cursor_pos = 0;
            app.pending_images.clear();
            app.set_status_notice("Deleted queued message");
        }
        QueuedMessageEditorOutcome::Released => {
            app.remote_queued_message_editor.complete_operation(false);
            app.set_status_notice("Released queued message editor");
        }
        QueuedMessageEditorOutcome::StalePlacement => {
            app.remote_queued_message_editor.complete_operation(false);
            app.input.clear();
            app.cursor_pos = 0;
            app.pending_images.clear();
            app.set_status_notice("Updated queued message using stale placement");
        }
        QueuedMessageEditorOutcome::Replay => {
            if let Some(selection) = selection {
                apply_selection(app, selection);
                app.remote_queued_message_editor.complete_operation(true);
            } else {
                app.remote_queued_message_editor.complete_operation(false);
            }
            app.set_status_notice("Replayed queued message editor result");
        }
        QueuedMessageEditorOutcome::Conflict => unreachable!("handled above"),
    }
    true
}

/// Send Enter as a finish operation while an authoritative editor is active.
/// The composer is intentionally left untouched until a terminal server result.
pub(super) async fn finish(app: &mut App, remote: &mut RemoteConnection) -> anyhow::Result<bool> {
    if !app.remote_queued_message_editor.is_active() {
        return Ok(false);
    }
    let draft = RecallableSoftInterrupt {
        content: app.input.clone(),
        images: app.pending_images.clone(),
    };
    let Some((navigation_session_id, operation_id, operation)) =
        app.remote_queued_message_editor.begin_finish(draft)
    else {
        return Ok(true);
    };
    remote
        .queued_message_editor(&navigation_session_id, &operation_id, operation)
        .await?;
    app.set_status_notice("Finishing queued message edit...");
    Ok(true)
}

/// Retry the exact unresolved operation after reconnect without changing the draft.
pub(in crate::tui::app) async fn retry_pending_after_reconnect(
    app: &mut App,
    remote: &mut RemoteConnection,
) -> anyhow::Result<bool> {
    let Some((navigation_session_id, operation_id, operation)) =
        app.remote_queued_message_editor.pending_request()
    else {
        return Ok(false);
    };
    remote
        .queued_message_editor(&navigation_session_id, &operation_id, operation)
        .await?;
    app.set_status_notice("Retrying queued message edit...");
    Ok(true)
}
