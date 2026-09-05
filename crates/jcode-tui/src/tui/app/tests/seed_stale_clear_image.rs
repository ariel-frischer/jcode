fn seed_stale_clear_image(app: &mut App) -> u64 {
    app.remote_side_pane_images = vec![crate::session::RenderedImage {
        media_type: "image/png".to_string(),
        data: "stale-image".to_string(),
        label: Some("stale.png".to_string()),
        source: crate::session::RenderedImageSource::UserInput,
        anchor: None,
    }];
    let _ = crate::tui::TuiState::side_pane_images_signature(app);
    app.expanded_images_version
}
