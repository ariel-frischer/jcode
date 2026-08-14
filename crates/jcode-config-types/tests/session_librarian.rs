use jcode_config_types::SessionLibrarianConfig;

#[test]
fn omitted_librarian_config_preserves_the_legacy_empty_shape() {
    let config: SessionLibrarianConfig = toml::from_str("").expect("legacy config");

    assert!(config.is_empty());
    assert_eq!(toml::to_string(&config).expect("serialize config"), "");
}

#[test]
fn librarian_config_round_trips_explicit_invalid_values_for_boundary_validation() {
    let config: SessionLibrarianConfig = toml::from_str(
        r#"
        provider = ""
        model = "gpt-5.6-luna"
        reasoning_effort = "unsupported"
        max_input_tokens = "0"
        max_output_tokens = "many"
        max_requests = "2"
        max_cost_usd = "NaN"
        deadline_seconds = "-1"
        "#,
    )
    .expect("persistence must retain values for resolver validation");

    assert_eq!(config.provider.as_deref(), Some(""));
    assert_eq!(config.max_input_tokens.as_deref(), Some("0"));
    assert_eq!(config.max_cost_usd.as_deref(), Some("NaN"));

    let encoded = toml::to_string(&config).expect("serialize config");
    let decoded: SessionLibrarianConfig = toml::from_str(&encoded).expect("deserialize config");
    assert_eq!(decoded, config);
}
