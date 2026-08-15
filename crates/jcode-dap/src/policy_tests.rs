use super::policy::{Action, DapPolicy, PermissionTier};

#[test]
fn actions_have_explicit_permission_tiers() {
    assert_eq!(Action::Sessions.tier(), PermissionTier::ReadOnly);
    assert_eq!(Action::StackTrace.tier(), PermissionTier::ReadOnly);
    assert_eq!(Action::Continue.tier(), PermissionTier::ProcessControl);
    assert_eq!(Action::Evaluate.tier(), PermissionTier::Evaluate);
    assert_eq!(Action::WriteMemory.tier(), PermissionTier::MemoryWrite);
}

#[test]
fn memory_write_is_denied_by_default_but_other_explicit_tiers_are_configurable() {
    let policy = DapPolicy::default();
    assert!(policy.check(Action::StackTrace).is_ok());
    assert!(policy.check(Action::Continue).is_ok());
    assert!(policy.check(Action::Evaluate).is_ok());
    assert!(policy.check(Action::WriteMemory).is_err());
}

#[test]
fn permission_configuration_is_layered_and_bounded() {
    let policy = DapPolicy::from_toml_layers(&[
        r#"
[permissions]
allow_process_control = false
request_timeout_ms = 5000
max_output_bytes = 4096
"#,
        r#"
[permissions]
allow_process_control = true
allow_memory_write = true
"#,
    ])
    .expect("policy");

    assert!(policy.check(Action::Continue).is_ok());
    assert!(policy.check(Action::WriteMemory).is_ok());
    assert_eq!(policy.request_timeout.as_millis(), 5000);
    assert_eq!(policy.max_output_bytes, 4096);
}
