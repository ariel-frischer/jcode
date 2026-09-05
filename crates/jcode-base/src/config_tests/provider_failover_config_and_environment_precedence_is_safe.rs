#[test]
fn provider_failover_config_and_environment_precedence_is_safe() {
    let _guard = crate::storage::lock_test_env();
    let previous = std::env::var_os("JCODE_CROSS_PROVIDER_FAILOVER");

    let from_config: Config =
        toml::from_str("[provider]\ncross_provider_failover = \"countdown\"\n")
            .expect("config failover mode should parse");
    assert_eq!(
        from_config.provider.cross_provider_failover,
        super::CrossProviderFailoverMode::Countdown
    );

    crate::env::set_var("JCODE_CROSS_PROVIDER_FAILOVER", "manual");
    let mut overridden = from_config;
    overridden.apply_env_overrides();
    assert_eq!(
        overridden.provider.cross_provider_failover,
        super::CrossProviderFailoverMode::Manual
    );

    crate::env::set_var("JCODE_CROSS_PROVIDER_FAILOVER", "not-a-mode");
    let mut invalid = Config::default();
    invalid.apply_env_overrides();
    assert_eq!(
        invalid.provider.cross_provider_failover,
        super::CrossProviderFailoverMode::Manual,
        "invalid environment values must not weaken the safe default"
    );

    restore_env_var("JCODE_CROSS_PROVIDER_FAILOVER", previous);
}
