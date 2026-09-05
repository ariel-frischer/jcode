#[test]
fn untouched_session_is_not_persisted_until_real_conversation_starts() -> Result<()> {
    let _env_lock = lock_env();
    let temp_home = tempfile::Builder::new()
        .prefix("jcode-session-lazy-save-test-")
        .tempdir()
        .map_err(|e| anyhow!(e))?;
    let _home = EnvVarGuard::set("JCODE_HOME", temp_home.path().as_os_str());

    let id = "session_untouched_lazy_save";
    let mut session = Session::create_with_id(id.to_string(), None, None);
    assert!(session.ensure_initial_session_context_message());
    session.save()?;
    assert!(!session_path(id)?.exists());

    session.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "hello".to_string(),
            cache_control: None,
        }],
    );
    session.save()?;
    assert!(session_path(id)?.exists());
    Ok(())
}
