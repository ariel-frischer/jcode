use super::{Config, config_env_fingerprint};

#[test]
fn swarm_completion_wake_environment_overrides_preserve_invalid_values() {
    let _lock = crate::storage::lock_test_env();
    let key = "JCODE_SWARM_COMPLETION_WAKE";
    let previous = std::env::var_os(key);
    let mut config: Config =
        toml::from_str("[agents]\nswarm_completion_wake = true\n").expect("completion wake config");
    assert!(
        config
            .display_string()
            .contains("- Swarm completion wake: true")
    );
    for (value, expected) in [("off", false), ("on", true), ("", true), ("invalid", true)] {
        crate::env::set_var(key, value);
        let fingerprint = config_env_fingerprint();
        config.apply_env_overrides();
        let actual = config.agents.swarm_completion_wake;
        match previous.as_ref() {
            Some(value) => crate::env::set_var(key, value),
            None => crate::env::remove_var(key),
        }
        assert_eq!(actual, expected, "environment value {value:?}");
        assert!(fingerprint.contains(&(key.to_string(), value.to_string())));
    }
}
