use crate::error::{DapError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum TransportMode {
    #[default]
    Stdio,
    Tcp,
    Socket,
    #[serde(rename = "tcp_listen", alias = "tcp-listen")]
    TcpListen,
    #[serde(rename = "socket_listen", alias = "socket-listen")]
    SocketListen,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AdapterConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub languages: Vec<String>,
    #[serde(default)]
    pub file_types: Vec<String>,
    #[serde(default)]
    pub root_markers: Vec<String>,
    #[serde(default = "empty_object")]
    pub launch_defaults: Value,
    #[serde(default = "empty_object")]
    pub attach_defaults: Value,
    #[serde(default)]
    pub transport: TransportMode,
    #[serde(default)]
    pub accepts_directory_program: bool,
}

fn default_enabled() -> bool {
    true
}

fn empty_object() -> Value {
    Value::Object(Map::new())
}

impl Default for AdapterConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            command: String::new(),
            args: Vec::new(),
            languages: Vec::new(),
            file_types: Vec::new(),
            root_markers: Vec::new(),
            launch_defaults: empty_object(),
            attach_defaults: empty_object(),
            transport: TransportMode::Stdio,
            accepts_directory_program: false,
        }
    }
}

impl AdapterConfig {
    pub fn test(command: &str, file_types: Vec<&str>) -> Self {
        Self {
            command: command.to_owned(),
            file_types: file_types.into_iter().map(str::to_owned).collect(),
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedAdapter {
    pub name: String,
    pub config: AdapterConfig,
    pub resolved_command: PathBuf,
    pub cwd: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct AdapterRegistry {
    adapters: BTreeMap<String, AdapterConfig>,
}

impl AdapterRegistry {
    pub fn builtins() -> BTreeMap<String, AdapterConfig> {
        let mut result = BTreeMap::new();
        result.insert(
            "gdb".into(),
            AdapterConfig {
                command: "gdb".into(),
                args: vec!["-i".into(), "dap".into()],
                languages: vec!["c".into(), "cpp".into(), "rust".into()],
                file_types: vec![".c".into(), ".cpp".into(), ".rs".into()],
                root_markers: vec![
                    "Makefile".into(),
                    "CMakeLists.txt".into(),
                    "Cargo.toml".into(),
                ],
                launch_defaults: serde_json::json!({"request":"launch", "stopOnEntry":true}),
                attach_defaults: serde_json::json!({"request":"attach"}),
                ..AdapterConfig::default()
            },
        );
        result.insert(
            "lldb-dap".into(),
            AdapterConfig {
                command: "lldb-dap".into(),
                languages: vec!["c".into(), "cpp".into(), "rust".into(), "swift".into()],
                file_types: vec![".c".into(), ".cpp".into(), ".rs".into(), ".swift".into()],
                root_markers: vec!["Cargo.toml".into(), "Package.swift".into()],
                launch_defaults: serde_json::json!({"request":"launch", "stopOnEntry":true}),
                attach_defaults: serde_json::json!({"request":"attach"}),
                ..AdapterConfig::default()
            },
        );
        result.insert(
            "debugpy".into(),
            AdapterConfig {
                command: "python".into(),
                args: vec!["-m".into(), "debugpy.adapter".into()],
                languages: vec!["python".into()],
                file_types: vec![".py".into()],
                root_markers: vec!["pyproject.toml".into(), "requirements.txt".into()],
                launch_defaults: serde_json::json!({"request":"launch", "justMyCode":false}),
                attach_defaults: serde_json::json!({"request":"attach", "justMyCode":false}),
                ..AdapterConfig::default()
            },
        );
        result.insert(
            "dlv".into(),
            AdapterConfig {
                command: "dlv".into(),
                args: vec!["dap".into(), "--listen=127.0.0.1:${port}".into()],
                transport: TransportMode::TcpListen,
                languages: vec!["go".into()],
                file_types: vec![".go".into()],
                root_markers: vec!["go.mod".into(), "go.work".into()],
                accepts_directory_program: true,
                ..AdapterConfig::default()
            },
        );
        result.insert(
            "js-debug-adapter".into(),
            AdapterConfig {
                command: "js-debug-adapter".into(),
                args: vec!["${port}".into(), "127.0.0.1".into()],
                languages: vec!["javascript".into(), "typescript".into()],
                file_types: vec![".js".into(), ".ts".into(), ".tsx".into()],
                root_markers: vec!["package.json".into(), "tsconfig.json".into()],
                transport: TransportMode::TcpListen,
                ..AdapterConfig::default()
            },
        );
        result
    }

    pub fn from_toml_layers(
        mut base: BTreeMap<String, AdapterConfig>,
        layers: &[&str],
    ) -> Result<Self> {
        for layer in layers {
            let value: toml::Value =
                toml::from_str(layer).map_err(|error| DapError::Config(error.to_string()))?;
            let value =
                serde_json::to_value(value).map_err(|error| DapError::Config(error.to_string()))?;
            let Some(adapters) = value.get("adapters") else {
                continue;
            };
            let Some(entries) = adapters.as_object() else {
                return Err(DapError::Config("adapters must be a table".into()));
            };
            for (name, override_value) in entries {
                let current = serde_json::to_value(base.get(name).cloned().unwrap_or_default())?;
                let merged = merge_values(current, override_value.clone());
                let config: AdapterConfig = serde_json::from_value(merged)?;
                base.insert(name.clone(), config);
            }
        }
        Ok(Self { adapters: base })
    }

    pub fn load(cwd: &Path) -> Result<Self> {
        let layers = Self::load_config_layers(cwd)?;
        let refs: Vec<&str> = layers.iter().map(String::as_str).collect();
        Self::from_toml_layers(Self::builtins(), &refs)
    }

    pub(crate) fn load_config_layers(cwd: &Path) -> Result<Vec<String>> {
        let (user, project) = Self::load_scoped_config_layers(cwd)?;
        let mut layers = Vec::new();
        if let Some(user) = user {
            layers.push(user);
        }
        layers.extend(project);
        Ok(layers)
    }

    pub(crate) fn load_scoped_config_layers(cwd: &Path) -> Result<(Option<String>, Vec<String>)> {
        let home = std::env::var_os("JCODE_HOME")
            .map(PathBuf::from)
            .or_else(dirs_home);
        let user = home
            .filter(|home| trusted_config_dir(home))
            .map(|home| read_config_file(&home.join("dap.toml")))
            .transpose()?
            .and_then(|layers| layers.into_iter().next());
        let mut current = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
        let boundary = nearest_vcs_root(&current)
            .or_else(|| dirs_home().filter(|home| current.starts_with(home)))
            .unwrap_or_else(|| current.clone());
        let mut dirs = Vec::new();
        loop {
            if trusted_config_dir(&current) {
                dirs.push(current.clone());
            }
            if current == boundary || !current.pop() {
                break;
            }
        }
        dirs.reverse();
        let mut project = Vec::new();
        for dir in dirs {
            project.extend(read_config_file(&dir.join("dap.toml"))?);
        }
        Ok((user, project))
    }

    pub fn get(&self, name: &str) -> Option<&AdapterConfig> {
        self.adapters.get(name)
    }
    pub fn adapters(&self) -> &BTreeMap<String, AdapterConfig> {
        &self.adapters
    }

    pub fn resolve(&self, name: &str, cwd: &Path) -> Result<ResolvedAdapter> {
        let config = self
            .adapters
            .get(name)
            .ok_or_else(|| DapError::Config(format!("adapter '{name}' is not configured")))?;
        if !config.enabled {
            return Err(DapError::Config(format!("adapter '{name}' is disabled")));
        }
        let command = resolve_command(&config.command, cwd).ok_or_else(|| {
            DapError::Config(format!(
                "adapter '{name}' command '{}' is unavailable",
                config.command
            ))
        })?;
        Ok(ResolvedAdapter {
            name: name.to_owned(),
            config: config.clone(),
            resolved_command: command,
            cwd: cwd.to_path_buf(),
        })
    }

    pub fn select_launch(
        &self,
        program: &Path,
        cwd: &Path,
        explicit: Option<&str>,
    ) -> Result<ResolvedAdapter> {
        if let Some(name) = explicit {
            return self.resolve(name, cwd);
        }
        let extension = program
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| format!(".{ext}").to_lowercase());
        let mut matches = Vec::new();
        for (name, config) in &self.adapters {
            if !config.enabled {
                continue;
            }
            let matches_type = match extension.as_deref() {
                Some(ext) => config
                    .file_types
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(ext)),
                None => config.accepts_directory_program,
            };
            if !matches_type {
                continue;
            }
            let specificity = root_marker_depth(program, cwd, &config.root_markers);
            if let Ok(adapter) = self.resolve(name, cwd) {
                matches.push((specificity, name.clone(), adapter));
            }
        }
        matches.sort_by(|left, right| {
            let specificity = match (left.0, right.0) {
                (Some(left), Some(right)) => left.cmp(&right),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            };
            specificity.then_with(|| left.1.cmp(&right.1))
        });
        matches
            .into_iter()
            .next()
            .map(|(_, _, adapter)| adapter)
            .ok_or_else(|| {
                DapError::Config(format!(
                    "no available DAP adapter matches {}",
                    program.display()
                ))
            })
    }
}

fn merge_values(base: Value, overlay: Value) -> Value {
    match (base, overlay) {
        (Value::Object(mut base), Value::Object(overlay)) => {
            for (key, value) in overlay {
                let merged = base
                    .remove(&key)
                    .map(|old| merge_values(old, value.clone()))
                    .unwrap_or(value);
                base.insert(key, merged);
            }
            Value::Object(base)
        }
        (_, overlay) => overlay,
    }
}

fn read_config_file(path: &Path) -> Result<Vec<String>> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(vec![content]),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error.into()),
    }
}

fn resolve_command(command: &str, cwd: &Path) -> Option<PathBuf> {
    let path = Path::new(command);
    if path.is_absolute() || command.contains('/') || command.contains('\\') {
        let candidate = if path.is_absolute() {
            path.to_path_buf()
        } else {
            cwd.join(path)
        };
        return is_executable(&candidate).then_some(candidate);
    }
    let mut candidates = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
        .flat_map(|dir| executable_candidates(&dir.join(command)));
    candidates.find(|candidate| is_executable(candidate))
}

fn executable_candidates(path: &Path) -> Vec<PathBuf> {
    #[cfg(windows)]
    let mut candidates = vec![path.to_path_buf()];
    #[cfg(not(windows))]
    let candidates = vec![path.to_path_buf()];
    #[cfg(windows)]
    if path.extension().is_none() {
        if let Some(exts) = std::env::var_os("PATHEXT") {
            candidates.extend(
                exts.to_string_lossy()
                    .split(';')
                    .filter(|extension| !extension.is_empty())
                    .map(|extension| path.with_extension(extension.trim_start_matches('.'))),
            );
        }
    }
    candidates
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

fn nearest_vcs_root(cwd: &Path) -> Option<PathBuf> {
    let mut current = cwd.to_path_buf();
    loop {
        if current.join(".git").exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

#[cfg(unix)]
fn trusted_config_dir(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }
    path.metadata()
        .map(|metadata| metadata.uid() == unsafe { libc::geteuid() })
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn trusted_config_dir(path: &Path) -> bool {
    path.is_dir()
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

fn root_marker_depth(program: &Path, cwd: &Path, markers: &[String]) -> Option<usize> {
    if markers.is_empty() {
        return None;
    }
    let mut current = if program.is_absolute() {
        program.parent().unwrap_or(cwd).to_path_buf()
    } else {
        cwd.join(program).parent().unwrap_or(cwd).to_path_buf()
    };
    let mut depth = 0;
    loop {
        if markers.iter().any(|marker| current.join(marker).exists()) {
            return Some(depth);
        }
        if !current.pop() {
            return None;
        }
        depth += 1;
    }
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .map(|home| home.join(".jcode"))
}
