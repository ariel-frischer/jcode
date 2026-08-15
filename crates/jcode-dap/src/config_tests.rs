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
    adapters.insert(
        "zeta".to_owned(),
        AdapterConfig::test("/bin/echo", vec![".rs"]),
    );
    adapters.insert(
        "alpha".to_owned(),
        AdapterConfig::test("/bin/echo", vec![".rs"]),
    );
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

    let config: AdapterConfig = toml::from_str(
        r#"
command = "js-debug-adapter"
transport = "tcp_listen"
args = ["${port}"]
"#,
    )
    .expect("listener config");
    assert_eq!(config.transport, TransportMode::TcpListen);
}

#[test]
fn policy_only_layers_do_not_become_adapter_entries() {
    let registry = AdapterRegistry::from_toml_layers(
        BTreeMap::new(),
        &["[permissions]\nallow_memory_write = true\n"],
    )
    .expect("policy-only layer");
    assert!(registry.adapters().is_empty());
}

#[test]
fn nearest_root_marker_wins_over_an_ancestor_marker() {
    let temp = tempfile::tempdir().expect("temp directory");
    let project = temp.path().join("project");
    let nested = project.join("nested");
    std::fs::create_dir_all(&nested).expect("nested directory");
    std::fs::write(project.join("Cargo.toml"), "[package]\nname = \"root\"\n")
        .expect("root marker");
    std::fs::write(nested.join(".debug-root"), "").expect("nested marker");
    let program = nested.join("main.rs");
    std::fs::write(&program, "fn main() {}\n").expect("program");

    let mut adapters = BTreeMap::new();
    adapters.insert(
        "ancestor".to_owned(),
        AdapterConfig {
            command: "/bin/echo".into(),
            file_types: vec![".rs".into()],
            root_markers: vec!["Cargo.toml".into()],
            ..AdapterConfig::default()
        },
    );
    adapters.insert(
        "nested".to_owned(),
        AdapterConfig {
            command: "/bin/echo".into(),
            file_types: vec![".rs".into()],
            root_markers: vec![".debug-root".into()],
            ..AdapterConfig::default()
        },
    );
    let registry = AdapterRegistry::from_toml_layers(adapters, &[]).expect("registry");
    let selected = registry
        .select_launch(&program, &nested, None)
        .expect("selected adapter");
    assert_eq!(selected.name, "nested");
}

#[test]
fn project_config_scan_stops_at_nearest_vcs_root() {
    let temp = tempfile::tempdir().expect("temp directory");
    let project = temp.path().join("project");
    let nested = project.join("nested");
    std::fs::create_dir_all(&nested).expect("nested directory");
    std::fs::create_dir(project.join(".git")).expect("vcs marker");
    std::fs::write(
        project.join("dap.toml"),
        "[adapters.fake]\ncommand = \"/bin/echo\"\n",
    )
    .expect("project config");

    let (_, project) = AdapterRegistry::load_scoped_config_layers(&nested).expect("layers");
    assert_eq!(project.len(), 1);
    assert!(project[0].contains("[adapters.fake]"));
}
