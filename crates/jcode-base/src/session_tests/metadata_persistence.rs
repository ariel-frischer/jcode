use super::*;
use anyhow::Result;

#[test]
fn explicit_metadata_only_session_is_persisted_without_a_prompt() -> Result<()> {
    let _lock = lock_env();
    let home = tempfile::tempdir()?;
    let _home = EnvVarGuard::set("JCODE_HOME", home.path());
    let mut session = Session::create_with_id("explicit-metadata".into(), None, None);
    session.model = Some("gpt-6-astra".into());
    session.provider_key = Some("openai".into());
    session.reasoning_effort = Some("low".into());
    session.route_api_method = Some("openai-oauth".into());
    session.save()?;

    let loaded = Session::load(&session.id)?;
    assert_eq!(loaded.model, session.model);
    assert_eq!(loaded.provider_key, session.provider_key);
    assert_eq!(loaded.reasoning_effort, session.reasoning_effort);
    assert_eq!(loaded.route_api_method, session.route_api_method);
    Ok(())
}

#[test]
fn startup_context_with_default_provider_metadata_stays_unpersisted() -> Result<()> {
    let _lock = lock_env();
    let home = tempfile::tempdir()?;
    let _home = EnvVarGuard::set("JCODE_HOME", home.path());
    let mut session = Session::create_with_id("untouched-startup".into(), None, None);
    session.model = Some("gpt-6-astra".into());
    session.provider_key = Some("openai".into());
    assert!(session.ensure_initial_session_context_message());
    session.save()?;

    assert!(!session_path(&session.id)?.exists());
    Ok(())
}

#[test]
fn startup_context_keeps_explicit_profile_metadata_before_a_prompt() -> Result<()> {
    let _lock = lock_env();
    let home = tempfile::tempdir()?;
    let _home = EnvVarGuard::set("JCODE_HOME", home.path());
    let mut session = Session::create_with_id("profiled-startup".into(), None, None);
    session.profile_name = Some("astra-low".into());
    session.profile_restore_status = Some(crate::config::ProfileRestoreStatus::Matching);
    assert!(session.ensure_initial_session_context_message());
    session.save()?;

    let loaded = Session::load(&session.id)?;
    assert_eq!(loaded.profile_name, session.profile_name);
    assert_eq!(
        loaded.profile_restore_status,
        session.profile_restore_status
    );
    Ok(())
}
