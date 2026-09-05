#[test]
fn swarm_effort_parses_from_toml_and_env_override() {
    let _guard = crate::storage::lock_test_env();
    let prev = std::env::var_os("JCODE_SWARM_EFFORT");
    restore_env_var("JCODE_SWARM_EFFORT", None);

    // Public config-file interface (#1165).
    let cfg: Config =
        toml::from_str("[agents]\nswarm_model = \"claude-opus-5\"\nswarm_effort = \"medium\"\n")
            .expect("config with swarm_effort parses");
    assert_eq!(cfg.agents.swarm_effort.as_deref(), Some("medium"));
    assert_eq!(Config::default().agents.swarm_effort, None);

    crate::env::set_var("JCODE_SWARM_EFFORT", "low");
    let mut cfg = Config::default();
    cfg.apply_env_overrides();
    assert_eq!(cfg.agents.swarm_effort.as_deref(), Some("low"));

    crate::env::set_var("JCODE_SWARM_EFFORT", " ");
    let mut cfg = Config::default();
    cfg.agents.swarm_effort = Some("preset".to_string());
    cfg.apply_env_overrides();
    assert_eq!(cfg.agents.swarm_effort, None);

    restore_env_var("JCODE_SWARM_EFFORT", prev);
}
