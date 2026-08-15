use super::config::{AdapterConfig, AdapterRegistry, TransportMode};
use std::collections::BTreeMap;
use std::path::Path;

#[test]
fn project_overrides_user_and_deep_merges_defaults() {
    let builtins = BTreeMap::from([(
        "fake".to_owned(),
        AdapterConfig {
            command: "/bin/echo".to_owned(),
            args: vec!["dap".to_owned()],
            file_types: vec![".rs".to_owned()],
            root_markers: vec!["Cargo.toml".to_owned()],
            launch_defaults: serde_json::json!({"request": "launch", "stopOnEntry": true}),
            attach_defaults: serde_json::json!({"request": "attach"}),
            ..AdapterConfig::default()
        },
    )]);
    let user = r#"
[adapters.fake]
args = ["user"]
[adapters.fake.launch_defaults]
stopOnEntry = false
"#;
    let project = r#"
[adapters.fake]
root_markers = [".git"]
[adapters.fake.launch_defaults]
custom = "project"
"#;
    let registry = AdapterRegistry::from_toml_layers(builtins, &[user, project]).expect("registry");
    let config = registry.get("fake").expect("fake adapter");
    assert_eq!(config.args, vec!["user"]);
    assert_eq!(config.root_markers, vec![".git"]);
    assert_eq!(config.launch_defaults["request"], "launch");
    assert_eq!(config.launch_defaults["stopOnEntry"], false);
    assert_eq!(config.launch_defaults["custom"], "project");
}

#[test]
fn launch_selection_is_stable_and_disabled_adapters_are_skipped() {
    let mut adapters = BTreeMap::new();
    adapters.insert("zeta".to_owned(), AdapterConfig::test("/bin/echo", vec![".rs"]));
    adapters.insert("alpha".to_owned(), AdapterConfig::test("/bin/echo", vec![".rs"]));
    adapters.get_mut("alpha").unwrap().enabled = false;
    let registry = AdapterRegistry::from_toml_layers(adapters, &[]).expect("registry");
    let selected = registry
        .select_launch(Path::new("main.rs"), Path::new("/tmp"), None)
        .expect("selected");
    assert_eq!(selected.name, "zeta");
}

#[test]
fn transport_mode_is_configurable_without_language_specific_logic() {
    let config: AdapterConfig = toml::from_str(
        r#"
command = "dlv"
transport = "socket"
"#,
    )
    .expect("config");
    assert_eq!(config.transport, TransportMode::Socket);
}
