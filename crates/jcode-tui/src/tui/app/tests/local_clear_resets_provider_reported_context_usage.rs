#[test]
fn local_clear_resets_provider_reported_context_usage() {
    let mut app = create_test_app();
    seed_stale_clear_usage(&mut app);
    seed_stale_clear_swarm_plan(&mut app);
    let image_version = seed_stale_clear_image(&mut app);

    assert!(super::commands::handle_session_command(&mut app, "/clear"));

    assert_clear_usage_reset(&app);
    assert_clear_swarm_plan_reset(&app);
    assert_clear_image_reset(&app, image_version);
}
