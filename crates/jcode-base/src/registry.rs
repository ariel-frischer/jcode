#![cfg_attr(test, allow(clippy::await_holding_lock))]

//! Server registry for multi-server architecture
//!
//! Tracks running servers in `~/.jcode/servers.json` for discovery by clients.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use tokio::fs;

use crate::storage::jcode_dir;

/// Information about a running server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    /// Full server ID (e.g., "server_blazing_1705012345678")
    pub id: String,
    /// Short name (e.g., "blazing")
    pub name: String,
    /// Icon for display (e.g., "🔥")
    pub icon: String,
    /// Socket path
    pub socket: PathBuf,
    /// Debug socket path
    pub debug_socket: PathBuf,
    /// Git hash of the binary
    pub git_hash: String,
    /// Version string (e.g., "v0.1.123")
    pub version: String,
    /// Process ID
    pub pid: u32,
    /// When the server started (ISO 8601)
    pub started_at: String,
    /// Session names currently on this server
    #[serde(default)]
    pub sessions: Vec<String>,
}

impl ServerInfo {
    /// Display name with icon (e.g., "🔥 blazing")
    pub fn display_name(&self) -> String {
        format!("{} {}", self.icon, self.name)
    }
}

/// The server registry file
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServerRegistry {
    /// Map from server name to server info
    #[serde(flatten)]
    pub servers: HashMap<String, ServerInfo>,
}

impl ServerRegistry {
    /// Load the registry from disk
    pub async fn load() -> Result<Self> {
        let path = registry_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(&path).await?;
        let registry: Self = serde_json::from_str(&content)?;
        Ok(registry)
    }

    /// Save the registry to disk
    pub async fn save(&self) -> Result<()> {
        self.save_sync()
    }

    /// Synchronously save the registry while holding the same inter-process
    /// lock used by asynchronous registry writers.
    pub fn save_sync(&self) -> Result<()> {
        let path = registry_path()?;

        // Ensure parent directory exists
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("server registry has no parent directory"))?;
        std::fs::create_dir_all(parent)?;
        let _lock = RegistryFileLock::acquire(&path)?;

        crate::storage::write_json_fast(&path, self)?;
        if let Err(e) = crate::platform::set_directory_permissions_owner_only(parent) {
            crate::logging::info(&format!(
                "Registry save: failed to harden directory permissions for {}: {}",
                parent.display(),
                e
            ));
        }
        if let Err(e) = crate::platform::set_permissions_owner_only(&path) {
            crate::logging::info(&format!(
                "Registry save: failed to harden file permissions for {}: {}",
                path.display(),
                e
            ));
        }
        Ok(())
    }

    /// Remove exact server snapshots while holding the registry lock.
    ///
    /// Cleanup runs from a snapshot because process inspection and bounded
    /// termination can take time. Reloading under the write lock and matching
    /// identity fields prevents that snapshot from clobbering a concurrent
    /// registration or replacement.
    pub fn remove_matching_sync(entries: &[ServerInfo]) -> Result<usize> {
        let path = registry_path()?;
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("server registry has no parent directory"))?;
        std::fs::create_dir_all(parent)?;
        let _lock = RegistryFileLock::acquire(&path)?;

        if !path.exists() {
            return Ok(0);
        }
        let content = std::fs::read_to_string(&path)?;
        let mut registry: Self = serde_json::from_str(&content)?;
        let mut removed = 0;
        for expected in entries {
            let matches = registry.servers.get(&expected.name).is_some_and(|actual| {
                actual.id == expected.id
                    && actual.pid == expected.pid
                    && actual.socket == expected.socket
                    && actual.git_hash == expected.git_hash
                    && actual.version == expected.version
                    && actual.started_at == expected.started_at
            });
            if matches {
                registry.servers.remove(&expected.name);
                removed += 1;
            }
        }

        if removed == 0 {
            return Ok(0);
        }
        crate::storage::write_json_fast(&path, &registry)?;
        if let Err(e) = crate::platform::set_directory_permissions_owner_only(parent) {
            crate::logging::info(&format!(
                "Registry save: failed to harden directory permissions for {}: {}",
                parent.display(),
                e
            ));
        }
        if let Err(e) = crate::platform::set_permissions_owner_only(&path) {
            crate::logging::info(&format!(
                "Registry save: failed to harden file permissions for {}: {}",
                path.display(),
                e
            ));
        }
        Ok(removed)
    }

    /// Register a server
    pub fn register(&mut self, info: ServerInfo) {
        self.servers.insert(info.name.clone(), info);
    }

    /// Unregister a server by name
    pub fn unregister(&mut self, name: &str) {
        self.servers.remove(name);
    }

    /// Find a server by name
    pub fn find_by_name(&self, name: &str) -> Option<&ServerInfo> {
        self.servers.get(name)
    }

    /// Get all servers sorted by started_at (newest first)
    pub fn servers_by_time(&self) -> Vec<&ServerInfo> {
        let mut servers: Vec<_> = self.servers.values().collect();
        servers.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        servers
    }

    /// Clean up stale entries (servers that are no longer running or have been superseded).
    ///
    /// Socket path ownership is managed by the server process itself. Registry
    /// cleanup must not unlink those paths because a new live server can reuse
    /// the same published socket after a reboot or reload while an older
    /// registry entry still references it.
    pub async fn cleanup_stale(&mut self) -> Result<Vec<String>> {
        let mut removed = Vec::new();

        // First pass: remove entries whose process is dead
        let names: Vec<_> = self.servers.keys().cloned().collect();
        for name in &names {
            if let Some(info) = self.servers.get(name) {
                let pid = info.pid;
                if !is_process_running(pid) {
                    removed.push(name.clone());
                    self.servers.remove(name);
                }
            }
        }

        // Second pass: if multiple entries share the same socket path (happens
        // after server exec/reload), keep only the newest one.
        let remaining: Vec<_> = self.servers.keys().cloned().collect();
        let mut socket_to_newest: std::collections::HashMap<PathBuf, (String, String)> =
            std::collections::HashMap::new();
        for name in &remaining {
            if let Some(info) = self.servers.get(name) {
                let entry = socket_to_newest
                    .entry(info.socket.clone())
                    .or_insert_with(|| (name.clone(), info.started_at.clone()));
                if info.started_at > entry.1 {
                    *entry = (name.clone(), info.started_at.clone());
                }
            }
        }
        for name in &remaining {
            if let Some(info) = self.servers.get(name)
                && let Some((newest_name, _)) = socket_to_newest.get(&info.socket)
                && newest_name != name
            {
                removed.push(name.clone());
                self.servers.remove(name);
            }
        }

        if !removed.is_empty() {
            self.save().await?;
        }

        Ok(removed)
    }

    /// Add a session to a server
    pub fn add_session(&mut self, server_name: &str, session_name: &str) {
        if let Some(info) = self.servers.get_mut(server_name)
            && !info.sessions.contains(&session_name.to_string())
        {
            info.sessions.push(session_name.to_string());
        }
    }

    /// Remove a session from a server
    pub fn remove_session(&mut self, server_name: &str, session_name: &str) {
        if let Some(info) = self.servers.get_mut(server_name) {
            info.sessions.retain(|s| s != session_name);
        }
    }
}

struct RegistryFileLock {
    #[cfg(unix)]
    _file: std::fs::File,
}

impl RegistryFileLock {
    fn acquire(registry_path: &Path) -> Result<Self> {
        let lock_path = registry_path.with_extension("lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)?;

        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;

            let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
            if result != 0 {
                Err(std::io::Error::last_os_error().into())
            } else {
                Ok(Self { _file: file })
            }
        }

        #[cfg(not(unix))]
        {
            drop(file);
            Ok(Self {})
        }
    }
}

/// Get the path to the registry file
pub fn registry_path() -> Result<PathBuf> {
    Ok(jcode_dir()?.join("servers.json"))
}

/// Get the socket directory path
pub fn socket_dir() -> Result<PathBuf> {
    Ok(crate::storage::runtime_dir().join("jcode"))
}

/// Get the socket path for a named server
pub fn server_socket_path(name: &str) -> PathBuf {
    socket_dir()
        .map(|d| d.join(format!("{}.sock", name)))
        .unwrap_or_else(|_| std::env::temp_dir().join(format!("jcode-{}.sock", name)))
}

/// Get the debug socket path for a named server
pub fn server_debug_socket_path(name: &str) -> PathBuf {
    socket_dir()
        .map(|d| d.join(format!("{}-debug.sock", name)))
        .unwrap_or_else(|_| std::env::temp_dir().join(format!("jcode-{}-debug.sock", name)))
}

/// Check if a process is still running
fn is_process_running(pid: u32) -> bool {
    crate::platform::is_process_running(pid)
}

/// Unregister a server from the registry
pub async fn unregister_server(name: &str) -> Result<()> {
    let mut registry = ServerRegistry::load().await?;
    registry.unregister(name);
    registry.save().await?;
    Ok(())
}

/// List all running servers
pub async fn list_servers() -> Result<Vec<ServerInfo>> {
    let mut registry = ServerRegistry::load().await?;
    registry.cleanup_stale().await?;
    Ok(registry.servers_by_time().into_iter().cloned().collect())
}

/// Best-effort sync lookup for a server by socket path.
///
/// This is used by client-side window title code before the async runtime is fully
/// established or in synchronous spawn helpers.
pub fn find_server_by_socket_sync(socket: &std::path::Path) -> Option<ServerInfo> {
    let path = registry_path().ok()?;
    let content = std::fs::read_to_string(path).ok()?;
    let registry: ServerRegistry = serde_json::from_str(&content).ok()?;
    registry
        .servers
        .values()
        .find(|info| info.socket == socket)
        .cloned()
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod registry_tests;
