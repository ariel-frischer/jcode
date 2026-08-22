use crate::error::{LspError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerConfig {
    pub enabled: bool,
    pub command: String,
    pub args: Vec<String>,
    pub file_types: Vec<String>,
    pub language_id: String,
    pub root_markers: Vec<String>,
    pub startup_timeout_ms: u64,
    pub request_timeout_ms: u64,
    pub idle_timeout_ms: u64,
    pub formatter: Option<String>,
    pub linter_mode: bool,
    pub settings: Value,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            // LSP is deliberately opt-in. Installed servers can be expensive,
            // especially rust-analyzer and TypeScript language servers.
            enabled: false,
            command: String::new(),
            args: Vec::new(),
            file_types: Vec::new(),
            language_id: String::new(),
            root_markers: Vec::new(),
            startup_timeout_ms: 10_000,
            request_timeout_ms: 2_000,
            idle_timeout_ms: 600_000,
            formatter: None,
            linter_mode: false,
            settings: Value::Object(Map::new()),
        }
    }
}

impl ServerConfig {
    pub fn test(command: impl Into<String>, file_types: &[&str]) -> Self {
        Self {
            command: command.into(),
            file_types: file_types.iter().map(|value| (*value).to_owned()).collect(),
            language_id: "test".into(),
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PartialServerConfig {
    pub enabled: Option<bool>,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub file_types: Option<Vec<String>>,
    pub language_id: Option<String>,
    pub root_markers: Option<Vec<String>>,
    pub startup_timeout_ms: Option<u64>,
    pub request_timeout_ms: Option<u64>,
    pub idle_timeout_ms: Option<u64>,
    pub formatter: Option<String>,
    pub linter_mode: Option<bool>,
    pub settings: Option<Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RegistryFile {
    #[serde(default)]
    pub servers: BTreeMap<String, PartialServerConfig>,
}

#[derive(Debug, Clone)]
pub struct ResolvedServer {
    pub name: String,
    pub config: ServerConfig,
    pub root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ServerRegistry {
    servers: BTreeMap<String, ServerConfig>,
}

impl Default for ServerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ServerRegistry {
    pub fn new() -> Self {
        Self {
            servers: builtins(),
        }
    }

    pub fn from_servers(servers: BTreeMap<String, ServerConfig>) -> Self {
        Self { servers }
    }

    pub fn servers(&self) -> &BTreeMap<String, ServerConfig> {
        &self.servers
    }

    pub fn apply_layer(&mut self, layer: BTreeMap<String, PartialServerConfig>) {
        for (name, partial) in layer {
            let config = self.servers.entry(name).or_default();
            apply_partial(config, partial);
        }
    }

    pub fn with_layers(
        user: BTreeMap<String, PartialServerConfig>,
        project: BTreeMap<String, PartialServerConfig>,
    ) -> Self {
        let mut registry = Self::new();
        registry.apply_layer(user);
        registry.apply_layer(project);
        registry
    }

    pub fn load(cwd: &Path) -> Result<Self> {
        let mut registry = Self::new();
        if let Some(home) = env::var_os("HOME") {
            registry.apply_file_if_present(&PathBuf::from(home).join(".config/jcode/lsp.toml"))?;
        }
        for path in [cwd.join(".jcode/lsp.toml"), cwd.join("lsp.toml")] {
            registry.apply_file_if_present(&path)?;
        }
        Ok(registry)
    }

    fn apply_file_if_present(&mut self, path: &Path) -> Result<()> {
        if !path.is_file() {
            return Ok(());
        }
        let content = std::fs::read_to_string(path)?;
        let file: RegistryFile = toml::from_str(&content)?;
        self.apply_layer(file.servers);
        Ok(())
    }

    pub fn select(
        &self,
        cwd: &Path,
        file: &Path,
        explicit: Option<&str>,
    ) -> Result<Option<ResolvedServer>> {
        let names: Vec<&str> = if let Some(name) = explicit {
            vec![name]
        } else {
            self.servers.keys().map(String::as_str).collect()
        };
        for name in names {
            let Some(config) = self.servers.get(name) else {
                if explicit.is_some() {
                    return Err(LspError::Config(format!(
                        "unknown language server '{name}'"
                    )));
                }
                continue;
            };
            if !config.enabled || !matches_file(config, file) {
                continue;
            }
            let Some(root) = find_root(cwd, &config.root_markers) else {
                continue;
            };
            if !command_available(&config.command) {
                continue;
            }
            return Ok(Some(ResolvedServer {
                name: name.to_owned(),
                config: config.clone(),
                root,
            }));
        }
        Ok(None)
    }

    pub fn names(&self) -> Vec<String> {
        self.servers.keys().cloned().collect()
    }
}

fn apply_partial(config: &mut ServerConfig, partial: PartialServerConfig) {
    if let Some(value) = partial.enabled {
        config.enabled = value;
    }
    if let Some(value) = partial.command {
        config.command = value;
    }
    if let Some(value) = partial.args {
        config.args = value;
    }
    if let Some(value) = partial.file_types {
        config.file_types = value;
    }
    if let Some(value) = partial.language_id {
        config.language_id = value;
    }
    if let Some(value) = partial.root_markers {
        config.root_markers = value;
    }
    if let Some(value) = partial.startup_timeout_ms {
        config.startup_timeout_ms = value;
    }
    if let Some(value) = partial.request_timeout_ms {
        config.request_timeout_ms = value;
    }
    if let Some(value) = partial.idle_timeout_ms {
        config.idle_timeout_ms = value;
    }
    if let Some(value) = partial.formatter {
        config.formatter = Some(value);
    }
    if let Some(value) = partial.linter_mode {
        config.linter_mode = value;
    }
    if let Some(value) = partial.settings {
        merge_values(&mut config.settings, value);
    }
}

fn merge_values(base: &mut Value, overlay: Value) {
    match (base, overlay) {
        (Value::Object(base), Value::Object(overlay)) => {
            for (key, value) in overlay {
                if let Some(existing) = base.get_mut(&key) {
                    merge_values(existing, value);
                } else {
                    base.insert(key, value);
                }
            }
        }
        (base, overlay) => *base = overlay,
    }
}

fn matches_file(config: &ServerConfig, file: &Path) -> bool {
    if config.file_types.is_empty() {
        return true;
    }
    let name = file
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    config.file_types.iter().any(|pattern| {
        let pattern = pattern.trim_start_matches('*');
        name.ends_with(pattern)
            || file.extension().and_then(|ext| ext.to_str())
                == Some(pattern.trim_start_matches('.'))
    })
}

fn find_root(cwd: &Path, markers: &[String]) -> Option<PathBuf> {
    if markers.is_empty() {
        return Some(cwd.to_path_buf());
    }
    let mut current = if cwd.is_file() {
        cwd.parent().unwrap_or(cwd).to_path_buf()
    } else {
        cwd.to_path_buf()
    };
    loop {
        if markers.iter().any(|marker| current.join(marker).exists()) {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

pub fn command_available(command: &str) -> bool {
    if command.is_empty() {
        return false;
    }
    let path = Path::new(command);
    if path.components().count() > 1 {
        return path.is_file();
    }
    env::var_os("PATH").is_some_and(|paths| {
        env::split_paths(&paths).any(|dir| {
            let candidate = dir.join(command);
            candidate.is_file() && is_executable(&candidate)
        })
    })
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

fn builtins() -> BTreeMap<String, ServerConfig> {
    let mut servers = BTreeMap::new();
    add_builtin(
        &mut servers,
        "rust-analyzer",
        "rust-analyzer",
        &[".rs"],
        "rust",
        &["Cargo.toml"],
    );
    add_builtin(
        &mut servers,
        "typescript-language-server",
        "typescript-language-server",
        &[".ts", ".tsx", ".js", ".jsx"],
        "typescript",
        &["package.json", "tsconfig.json"],
    );
    if let Some(config) = servers.get_mut("typescript-language-server") {
        config.args = vec!["--stdio".into()];
    }
    add_builtin(
        &mut servers,
        "pyright",
        "pyright-langserver",
        &[".py", ".pyi"],
        "python",
        &["pyproject.toml", "requirements.txt"],
    );
    if let Some(config) = servers.get_mut("pyright") {
        config.args = vec!["--stdio".into()];
    }
    add_builtin(
        &mut servers,
        "gopls",
        "gopls",
        &[".go"],
        "go",
        &["go.mod", "go.work"],
    );
    add_builtin(
        &mut servers,
        "clangd",
        "clangd",
        &[".c", ".h", ".cpp", ".hpp"],
        "cpp",
        &["compile_commands.json", "CMakeLists.txt"],
    );
    add_builtin(
        &mut servers,
        "jdtls",
        "jdtls",
        &[".java"],
        "java",
        &["pom.xml", "build.gradle"],
    );
    add_builtin(
        &mut servers,
        "omnisharp",
        "omnisharp",
        &[".cs"],
        "csharp",
        &[".sln", ".csproj"],
    );
    add_builtin(
        &mut servers,
        "kotlin-language-server",
        "kotlin-language-server",
        &[".kt", ".kts"],
        "kotlin",
        &["build.gradle", "settings.gradle"],
    );
    add_builtin(
        &mut servers,
        "intelephense",
        "intelephense",
        &[".php"],
        "php",
        &["composer.json"],
    );
    add_builtin(
        &mut servers,
        "ruby-lsp",
        "ruby-lsp",
        &[".rb", ".rake"],
        "ruby",
        &["Gemfile", ".ruby-version"],
    );
    add_builtin(
        &mut servers,
        "yaml-language-server",
        "yaml-language-server",
        &[".yaml", ".yml"],
        "yaml",
        &[".yamllint", "package.json"],
    );
    if let Some(config) = servers.get_mut("yaml-language-server") {
        config.args = vec!["--stdio".into()];
    }
    servers
}

fn add_builtin(
    servers: &mut BTreeMap<String, ServerConfig>,
    name: &str,
    command: &str,
    file_types: &[&str],
    language_id: &str,
    root_markers: &[&str],
) {
    servers.insert(
        name.into(),
        ServerConfig {
            command: command.into(),
            file_types: file_types.iter().map(|value| (*value).into()).collect(),
            language_id: language_id.into(),
            root_markers: root_markers.iter().map(|value| (*value).into()).collect(),
            ..ServerConfig::default()
        },
    );
}
