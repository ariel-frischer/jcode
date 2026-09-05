/// Fork the current session (like `/split`) and, when given, deliver `prompt`
/// as the first message of the forked session. Shared by `/btw <question>`,
/// `/fork [prompt]`, and `/split`.
pub(super) fn fork_session_with_prompt_local(app: &mut App, prompt: Option<&str>) {
    // Images attached to the input belong to the prompt being forked off, so
    // they travel with it instead of lingering on the parent's next message.
    let images = if prompt.is_some() {
        std::mem::take(&mut app.pending_images)
    } else {
        Vec::new()
    };
    let staged = prompt.map(|prompt| (prompt.to_string(), images.clone()));
    if let Err(error) = launch_forked_session_local(app, staged) {
        if !images.is_empty() {
            app.pending_images = images;
        }
        app.push_display_message(DisplayMessage::error(format!(
            "Failed to fork session: {}",
            error
        )));
        app.set_status_notice("Fork failed");
    }
}
