fn assert_clear_image_reset(app: &App, previous_version: u64) {
    assert!(app.remote_side_pane_images.is_empty());
    assert_eq!(app.side_pane_images_signature_cache.get(), None);
    assert!(app.expanded_images.is_empty());
    assert_eq!(
        app.expanded_images_version,
        previous_version.wrapping_add(1)
    );
}
