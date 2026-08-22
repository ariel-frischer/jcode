use super::config::{PartialServerConfig, ServerConfig, ServerRegistry};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;

#[test]
fn catalog_contains_the_common_language_ecosystems() {
    let registry = ServerRegistry::new();
    for name in [
        "rust-analyzer",
        "typescript-language-server",
        "pyright",
        "gopls",
        "clangd",
        "jdtls",
        "omnisharp",
        "kotlin-language-server",
        "intelephense",
        "ruby-lsp",
        "yaml-language-server",
    ] {
        let config = registry.servers().get(name).expect("catalog entry");
        assert!(!config.enabled, "catalog entry {name} must be opt-in");
    }
}

#[test]
fn project_layer_overrides_user_layer_and_merges_settings() {
    let mut user = BTreeMap::new();
    user.insert(
        "custom".into(),
        PartialServerConfig {
            command: Some("user-server".into()),
            settings: Some(json!({"a": {"b": 1}, "user": true})),
            ..Default::default()
        },
    );
    let mut project = BTreeMap::new();
    project.insert(
        "custom".into(),
        PartialServerConfig {
            command: Some("project-server".into()),
            enabled: Some(false),
            settings: Some(json!({"a": {"c": 2}})),
            ..Default::default()
        },
    );
    let registry = ServerRegistry::with_layers(user, project);
    let config = registry.servers().get("custom").expect("custom server");
    assert_eq!(config.command, "project-server");
    assert!(!config.enabled);
    assert_eq!(
        config.settings,
        json!({"a": {"b": 1, "c": 2}, "user": true})
    );
}

#[test]
fn selection_requires_matching_root_and_executable() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname='fixture'\n",
    )
    .expect("marker");
    let mut servers = BTreeMap::new();
    servers.insert(
        "fixture".into(),
        ServerConfig {
            enabled: true,
            command: "/bin/sh".into(),
            file_types: vec![".rs".into()],
            root_markers: vec!["Cargo.toml".into()],
            ..ServerConfig::default()
        },
    );
    let registry = ServerRegistry::from_servers(servers);
    let selected = registry
        .select(temp.path(), Path::new("src/main.rs"), None)
        .expect("select");
    assert_eq!(selected.expect("server").name, "fixture");
}

#[test]
fn missing_executable_is_not_activated() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut servers = BTreeMap::new();
    servers.insert(
        "missing".into(),
        ServerConfig::test("definitely-not-installed-jcode-lsp", &[".rs"]),
    );
    let registry = ServerRegistry::from_servers(servers);
    assert!(
        registry
            .select(temp.path(), Path::new("main.rs"), None)
            .expect("select")
            .is_none()
    );
}
