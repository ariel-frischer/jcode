pub(in crate::tui::app) async fn send_interleave_now(
    app: &mut App,
    content: String,
    images: Vec<(String, String)>,
    remote: &mut RemoteConnection,
) {
    if content.trim().is_empty() {
        return;
    }
    let msg_clone = content.clone();
    match remote.soft_interrupt(content, images, false).await {
        Err(e) => {
            app.push_display_message(DisplayMessage::error(format!(
                "Failed to send interleave: {}",
                e
            )));
        }
        Ok(request_id) => {
            app.track_pending_soft_interrupt(request_id, msg_clone);
            app.set_status_notice("⏭ Interleave sent");
        }
    }
}
