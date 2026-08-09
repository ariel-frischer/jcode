#[test]
fn legacy_config_without_run_safety_section_remains_unset() {
    let cfg: Config =
        toml::from_str("[display]\ncentered = true\n").expect("legacy config should deserialize");
    assert!(cfg.display.centered);
    assert_eq!(cfg.run_safety, RunSafetyConfig::default());
}

#[test]
fn run_safety_environment_values_are_retained_raw() {
    let _guard = crate::storage::lock_test_env();
    let keys = [
        "JCODE_RUN_MAX_TURNS",
        "JCODE_RUN_MAX_TOOL_STEPS",
        "JCODE_RUN_TOKEN_BUDGET",
        "JCODE_RUN_DEADLINE",
    ];
    let previous: Vec<_> = keys
        .iter()
        .map(|key| (*key, std::env::var_os(key)))
        .collect();
    crate::env::set_var("JCODE_RUN_MAX_TURNS", "  ");
    crate::env::set_var("JCODE_RUN_MAX_TOOL_STEPS", "0");
    crate::env::set_var("JCODE_RUN_TOKEN_BUDGET", "100");
    crate::env::set_var("JCODE_RUN_DEADLINE", "2030-01-01T00:00:00Z");

    let mut cfg = Config::default();
    cfg.apply_env_overrides();
    assert_eq!(cfg.run_safety.max_turns.as_deref(), Some("  "));
    assert_eq!(cfg.run_safety.max_tool_steps.as_deref(), Some("0"));
    assert_eq!(cfg.run_safety.token_budget.as_deref(), Some("100"));
    assert_eq!(
        cfg.run_safety.deadline.as_deref(),
        Some("2030-01-01T00:00:00Z")
    );
    for (key, value) in previous {
        restore_env_var(key, value);
    }
}

#[test]
fn run_safety_sources_returns_persisted_config_errors() {
    let _guard = crate::storage::lock_test_env();
    let temp_home = tempfile::tempdir().expect("temporary JCODE_HOME");
    let previous_home = std::env::var_os("JCODE_HOME");
    crate::env::set_var("JCODE_HOME", temp_home.path());
    std::fs::write(temp_home.path().join("config.toml"), "[run_safety\n")
        .expect("write malformed config");

    let result = Config::run_safety_sources();

    assert!(
        result.is_err(),
        "malformed persisted config must fail before run-safety resolution"
    );
    assert!(
        result
            .expect_err("the malformed config should have produced an error")
            .to_string()
            .contains("Failed to parse config file")
    );
    restore_env_var("JCODE_HOME", previous_home);
}
