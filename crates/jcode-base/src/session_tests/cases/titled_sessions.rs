#[test]
fn session_created_with_title_is_persisted_before_first_visible_message() -> Result<()> {
    // Regression for #1144: `Session::create(_, Some(title))` was skipped by
    // the untouched-session gate, so later lookups by id found nothing.
    let _env_lock = lock_env();
    let temp_home = tempfile::Builder::new()
        .prefix("jcode-session-titled-save-test-")
        .tempdir()
        .map_err(|e| anyhow!(e))?;
    let _home = EnvVarGuard::set("JCODE_HOME", temp_home.path().as_os_str());

    let id = "session_titled_eager_save";
    let mut session = Session::create_with_id(id.to_string(), None, Some("review".to_string()));
    assert!(session.ensure_initial_session_context_message());
    session.save()?;
    assert!(session_path(id)?.exists());

    let stub = Session::load_startup_stub(id)?;
    assert_eq!(stub.title.as_deref(), Some("review"));
    Ok(())
}
