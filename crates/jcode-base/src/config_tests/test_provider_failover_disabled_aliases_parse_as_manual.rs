#[test]
fn test_provider_failover_disabled_aliases_parse_as_manual() {
    for value in ["off", "false", "disabled", "none"] {
        let cfg: Config = toml::from_str(&format!(
            "[provider]\ncross_provider_failover = \"{value}\"\n"
        ))
        .unwrap_or_else(|error| panic!("{value} should parse: {error}"));
        assert_eq!(
            cfg.provider.cross_provider_failover,
            super::CrossProviderFailoverMode::Manual
        );
        assert_eq!(
            super::CrossProviderFailoverMode::parse(value),
            Some(super::CrossProviderFailoverMode::Manual)
        );
    }
}
