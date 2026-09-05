#[test]
fn remote_clear_resets_provider_reported_context_usage() {
    let mut app = create_test_app();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();
    let mut remote = crate::tui::backend::RemoteConnection::dummy();
    remote.mark_history_loaded();
    app.is_remote = true;
    seed_stale_clear_usage(&mut app);
    seed_stale_clear_swarm_plan(&mut app);
    let image_version = seed_stale_clear_image(&mut app);
    app.input = "/clear".to_string();
    app.cursor_pos = app.input.len();

    rt.block_on(app.handle_remote_key(KeyCode::Enter, KeyModifiers::empty(), &mut remote))
        .expect("remote /clear should succeed");

    assert_clear_usage_reset(&app);
    assert_clear_swarm_plan_reset(&app);
    assert_clear_image_reset(&app, image_version);
}
