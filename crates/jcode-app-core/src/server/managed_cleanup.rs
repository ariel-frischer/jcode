//! Narrow cleanup for Jcode-managed server processes left behind by upgrades.
//!
//! The server registry is the ownership boundary. We never scan the process
//! table looking for arbitrary `jcode` names, and we never act on a registry
//! that cannot be parsed. A candidate must also be a detached `serve` process
//! whose executable is inside the managed immutable build store.

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const GRACEFUL_WAIT: Duration = Duration::from_secs(1);
const ESCALATION_WAIT: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Serialize, Default)]
pub struct ManagedServerCleanupReport {
    pub socket: String,
    pub scanned: usize,
    pub cleaned: usize,
    pub skipped: usize,
    pub metadata_issue: Option<String>,
    pub entries: Vec<ManagedServerCleanupEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManagedServerCleanupEntry {
    pub name: String,
    pub pid: u32,
    pub socket: String,
    pub version: String,
    pub git_hash: String,
    pub decision: String,
    pub outcome: String,
}

/// Retire stale Jcode-managed daemons without disturbing the active server.
///
/// This is intentionally synchronous so release installers, hot rebuilds, and
/// the CLI command can all use exactly the same implementation. The bounded
/// waits are short and only run after all ownership checks pass.
pub fn cleanup_stale_managed_servers() -> ManagedServerCleanupReport {
    let canonical_socket = super::socket_path();
    let mut report = ManagedServerCleanupReport {
        socket: canonical_socket.display().to_string(),
        ..Default::default()
    };

    // An explicit socket is an isolated/custom service boundary. Install and
    // rebuild cleanup must never reinterpret it as the shared daemon.
    if std::env::var_os("JCODE_SOCKET").is_some() {
        report.metadata_issue = Some(
            "explicit JCODE_SOCKET is configured; leaving isolated server processes untouched"
                .to_string(),
        );
        return report;
    }

    let registry_path = match crate::registry::registry_path() {
        Ok(path) => path,
        Err(error) => {
            report.metadata_issue = Some(format!("server registry path unavailable: {error}"));
            return report;
        }
    };
    let registry_content = match std::fs::read_to_string(&registry_path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return report,
        Err(error) => {
            report.metadata_issue = Some(format!(
                "could not read server registry {}: {error}",
                registry_path.display()
            ));
            return report;
        }
    };
    let registry: crate::registry::ServerRegistry = match serde_json::from_str(&registry_content) {
        Ok(registry) => registry,
        Err(error) => {
            report.metadata_issue = Some(format!(
                "server registry is malformed; no processes were inspected: {error}"
            ));
            return report;
        }
    };

    let versions_dir = match crate::build::builds_dir() {
        Ok(path) => path.join("versions"),
        Err(error) => {
            report.metadata_issue = Some(format!("managed build path unavailable: {error}"));
            return report;
        }
    };
    let current_paths = current_managed_binary_paths();
    let listener_active = socket_has_live_listener(&canonical_socket);
    let active_listener_pid = registry
        .servers
        .values()
        .filter(|info| info.socket == canonical_socket && process_running(info.pid))
        .max_by(|left, right| left.started_at.cmp(&right.started_at))
        .map(|info| info.pid)
        .filter(|_| listener_active);
    let mut valid_pid_counts = HashMap::new();
    for info in registry.servers.values() {
        if info.socket == canonical_socket
            && !info.version.trim().is_empty()
            && !info.git_hash.trim().is_empty()
        {
            *valid_pid_counts.entry(info.pid).or_insert(0usize) += 1;
        }
    }
    let duplicate_valid_pids: HashSet<u32> = valid_pid_counts
        .into_iter()
        .filter_map(|(pid, count)| (count > 1).then_some(pid))
        .collect();
    let mut removed_servers = Vec::new();

    for info in registry.servers.values() {
        report.scanned += 1;

        let mut entry = ManagedServerCleanupEntry {
            name: info.name.clone(),
            pid: info.pid,
            socket: info.socket.display().to_string(),
            version: info.version.clone(),
            git_hash: info.git_hash.clone(),
            decision: "preserve".to_string(),
            outcome: String::new(),
        };

        if info.socket != canonical_socket {
            entry.outcome = "alternate-socket".to_string();
            report.skipped += 1;
            report.entries.push(entry);
            continue;
        }
        if info.version.trim().is_empty() || info.git_hash.trim().is_empty() {
            entry.outcome = "missing-version-metadata".to_string();
            report.skipped += 1;
            report.entries.push(entry);
            continue;
        }
        if duplicate_valid_pids.contains(&info.pid) {
            entry.outcome = "duplicate-pid".to_string();
            report.skipped += 1;
            report.entries.push(entry);
            continue;
        }
        if !process_running(info.pid) {
            entry.outcome = "already-exited".to_string();
            report.skipped += 1;
            report.entries.push(entry);
            continue;
        }
        if active_listener_pid == Some(info.pid) {
            entry.outcome = "current-shared-daemon".to_string();
            report.skipped += 1;
            report.entries.push(entry);
            continue;
        }
        if info.pid == std::process::id() {
            entry.outcome = "current-process".to_string();
            report.skipped += 1;
            report.entries.push(entry);
            continue;
        }
        let Some(executable) = managed_server_executable(info.pid, &versions_dir) else {
            entry.outcome = "ownership-unproven".to_string();
            report.skipped += 1;
            report.entries.push(entry);
            continue;
        };
        if !process_commandline_is_serve(info.pid) {
            entry.outcome = "not-a-serve-daemon".to_string();
            report.skipped += 1;
            report.entries.push(entry);
            continue;
        }
        if current_paths.iter().any(|path| path == &executable) {
            entry.outcome = "current-build".to_string();
            report.skipped += 1;
            report.entries.push(entry);
            continue;
        }

        entry.decision = "retire".to_string();
        let outcome = retire_owned_server(info.pid);
        if matches!(
            outcome.as_str(),
            "graceful-exit" | "escalated-exit" | "already-exited"
        ) {
            report.cleaned += 1;
            removed_servers.push(info.clone());
        } else {
            report.skipped += 1;
        }
        entry.outcome = outcome;
        report.entries.push(entry);
    }

    // Keep the registry from repeatedly presenting processes that cleanup has
    // conclusively retired. This is best effort and never changes the safety
    // decision above.
    if !removed_servers.is_empty()
        && let Err(error) = crate::registry::ServerRegistry::remove_matching_sync(&removed_servers)
    {
        report.metadata_issue = Some(format!(
            "cleaned processes but could not update server registry: {error}"
        ));
    }

    report
}

fn process_running(pid: u32) -> bool {
    if pid == 0 || !crate::platform::is_process_running(pid) {
        return false;
    }

    #[cfg(target_os = "linux")]
    {
        // kill(pid, 0) still succeeds for a zombie until its parent reaps it.
        // Cleanup cares whether the daemon is executable, not whether a dead
        // process table entry is awaiting reap.
        if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat"))
            && let Some(state) = stat
                .rsplit_once(')')
                .and_then(|(_, rest)| rest.split_whitespace().next())
            && state == "Z"
        {
            return false;
        }
    }

    true
}

fn current_managed_binary_paths() -> Vec<PathBuf> {
    [
        crate::build::shared_server_binary_path(),
        crate::build::current_binary_path(),
    ]
    .into_iter()
    .filter_map(Result::ok)
    .filter_map(canonicalize_path)
    .collect()
}

#[expect(
    clippy::manual_ok_err,
    reason = "The production swallowed-error guardrail forbids fallible-to-option conversion for this ownership probe"
)]
fn canonicalize_path(path: PathBuf) -> Option<PathBuf> {
    match std::fs::canonicalize(path) {
        Ok(path) => Some(path),
        Err(_) => None,
    }
}

fn managed_server_executable(pid: u32, versions_dir: &Path) -> Option<PathBuf> {
    let executable = process_executable(pid)?;
    let executable = match std::fs::canonicalize(executable) {
        Ok(path) => path,
        Err(_) => return None,
    };
    let version_dir = executable.parent()?;
    let managed_root = match std::fs::canonicalize(versions_dir) {
        Ok(path) => path,
        Err(_) => return None,
    };
    if version_dir.parent()? != managed_root {
        return None;
    }
    Some(executable)
}

fn retire_owned_server(pid: u32) -> String {
    if !process_running(pid) {
        return "already-exited".to_string();
    }

    if crate::platform::signal_detached_process_group(pid, termination_signal()).is_err() {
        return "graceful-signal-failed".to_string();
    }
    if wait_for_exit(pid, GRACEFUL_WAIT) {
        return "graceful-exit".to_string();
    }

    if crate::platform::signal_detached_process_group(pid, kill_signal()).is_err() {
        return "escalation-signal-failed".to_string();
    }
    if wait_for_exit(pid, ESCALATION_WAIT) {
        "escalated-exit".to_string()
    } else {
        "still-running".to_string()
    }
}

fn wait_for_exit(pid: u32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while process_running(pid) {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    true
}

#[cfg(unix)]
fn termination_signal() -> i32 {
    libc::SIGTERM
}

#[cfg(not(unix))]
fn termination_signal() -> i32 {
    0
}

#[cfg(unix)]
fn kill_signal() -> i32 {
    libc::SIGKILL
}

#[cfg(not(unix))]
fn kill_signal() -> i32 {
    0
}

#[cfg(target_os = "linux")]
fn process_executable(pid: u32) -> Option<PathBuf> {
    let path = match std::fs::read_link(format!("/proc/{pid}/exe")) {
        Ok(path) => path,
        Err(_) => return None,
    };
    let path = path.to_string_lossy();
    Some(PathBuf::from(
        path.strip_suffix(" (deleted)").unwrap_or(&path),
    ))
}

#[cfg(not(target_os = "linux"))]
fn process_executable(_pid: u32) -> Option<PathBuf> {
    None
}

#[cfg(target_os = "linux")]
fn process_commandline_is_serve(pid: u32) -> bool {
    match std::fs::read(format!("/proc/{pid}/cmdline")) {
        Ok(cmdline) => cmdline.split(|byte| *byte == 0).any(|arg| arg == b"serve"),
        Err(_) => false,
    }
}

#[cfg(not(target_os = "linux"))]
fn process_commandline_is_serve(_pid: u32) -> bool {
    false
}

#[cfg(unix)]
fn socket_has_live_listener(path: &Path) -> bool {
    use std::os::unix::net::UnixStream;

    UnixStream::connect(path).is_ok()
}

#[cfg(not(unix))]
fn socket_has_live_listener(_path: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{ServerInfo, ServerRegistry};
    use std::ffi::OsString;
    use std::process::{Command, Stdio};
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    struct EnvGuard {
        home: Option<OsString>,
        runtime: Option<OsString>,
        socket: Option<OsString>,
    }

    impl EnvGuard {
        fn new(home: &Path, runtime: &Path) -> Self {
            let guard = Self {
                home: std::env::var_os("JCODE_HOME"),
                runtime: std::env::var_os("JCODE_RUNTIME_DIR"),
                socket: std::env::var_os("JCODE_SOCKET"),
            };
            crate::env::set_var("JCODE_HOME", home);
            crate::env::set_var("JCODE_RUNTIME_DIR", runtime);
            crate::env::remove_var("JCODE_SOCKET");
            guard
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            restore_env("JCODE_HOME", self.home.take());
            restore_env("JCODE_RUNTIME_DIR", self.runtime.take());
            restore_env("JCODE_SOCKET", self.socket.take());
        }
    }

    fn restore_env(name: &str, value: Option<OsString>) {
        match value {
            Some(value) => crate::env::set_var(name, value),
            None => crate::env::remove_var(name),
        }
    }

    fn registry_info(pid: u32, socket: PathBuf) -> ServerInfo {
        ServerInfo {
            id: "server_old".to_string(),
            name: "old".to_string(),
            icon: "🪦".to_string(),
            socket,
            debug_socket: PathBuf::from("/tmp/jcode-debug.sock"),
            git_hash: "oldhash".to_string(),
            version: "old-version".to_string(),
            pid,
            started_at: "2026-01-01T00:00:00Z".to_string(),
            sessions: Vec::new(),
        }
    }

    fn write_registry(registry: &ServerRegistry) {
        let path = crate::registry::registry_path().expect("registry path");
        std::fs::create_dir_all(path.parent().unwrap()).expect("registry dir");
        std::fs::write(path, serde_json::to_vec(registry).expect("registry json"))
            .expect("registry write");
    }

    #[test]
    fn malformed_registry_is_a_safe_noop() {
        let _lock = env_lock();
        let home = tempfile::tempdir().expect("home");
        let runtime = tempfile::tempdir().expect("runtime");
        let _env = EnvGuard::new(home.path(), runtime.path());
        let path = crate::registry::registry_path().expect("registry path");
        std::fs::create_dir_all(path.parent().unwrap()).expect("registry dir");
        std::fs::write(&path, b"not json").expect("malformed registry");

        let report = cleanup_stale_managed_servers();
        assert_eq!(report.cleaned, 0);
        assert!(report.metadata_issue.unwrap().contains("malformed"));
    }

    #[test]
    fn alternate_socket_is_preserved_without_process_inspection() {
        let _lock = env_lock();
        let home = tempfile::tempdir().expect("home");
        let runtime = tempfile::tempdir().expect("runtime");
        let _env = EnvGuard::new(home.path(), runtime.path());
        let alternate = runtime.path().join("named.sock");
        let mut registry = ServerRegistry::default();
        registry.register(registry_info(std::process::id(), alternate));
        write_registry(&registry);

        let report = cleanup_stale_managed_servers();
        assert_eq!(report.cleaned, 0);
        assert_eq!(report.entries[0].outcome, "alternate-socket");
    }

    #[test]
    fn missing_registry_is_a_noop_for_partial_install() {
        let _lock = env_lock();
        let home = tempfile::tempdir().expect("home");
        let runtime = tempfile::tempdir().expect("runtime");
        let _env = EnvGuard::new(home.path(), runtime.path());

        let report = cleanup_stale_managed_servers();
        assert_eq!(report.scanned, 0);
        assert_eq!(report.cleaned, 0);
        assert!(report.metadata_issue.is_none());
    }

    #[test]
    fn explicit_socket_is_preserved_without_registry_inspection() {
        let _lock = env_lock();
        let home = tempfile::tempdir().expect("home");
        let runtime = tempfile::tempdir().expect("runtime");
        let _env = EnvGuard::new(home.path(), runtime.path());
        crate::env::set_var("JCODE_SOCKET", runtime.path().join("private.sock"));

        let report = cleanup_stale_managed_servers();
        assert_eq!(report.cleaned, 0);
        assert!(
            report
                .metadata_issue
                .unwrap()
                .contains("explicit JCODE_SOCKET")
        );
    }

    #[cfg(unix)]
    #[test]
    fn current_shared_daemon_is_preserved() {
        let _lock = env_lock();
        let home = tempfile::tempdir().expect("home");
        let runtime = tempfile::tempdir().expect("runtime");
        let _env = EnvGuard::new(home.path(), runtime.path());
        let socket = super::super::socket_path();
        let _listener = std::os::unix::net::UnixListener::bind(&socket).expect("live socket");
        let mut registry = ServerRegistry::default();
        registry.register(registry_info(std::process::id(), socket));
        write_registry(&registry);

        let report = cleanup_stale_managed_servers();
        assert_eq!(report.cleaned, 0);
        assert_eq!(report.entries[0].outcome, "current-shared-daemon");
    }

    #[test]
    fn duplicate_and_missing_metadata_are_not_kill_candidates() {
        let _lock = env_lock();
        let home = tempfile::tempdir().expect("home");
        let runtime = tempfile::tempdir().expect("runtime");
        let _env = EnvGuard::new(home.path(), runtime.path());
        let socket = super::super::socket_path();
        let mut first = registry_info(std::process::id(), socket.clone());
        first.name = "missing-version".to_string();
        first.version.clear();
        let mut second = registry_info(std::process::id(), socket);
        second.name = "duplicate".to_string();
        let mut third = second.clone();
        third.name = "duplicate-again".to_string();
        let mut registry = ServerRegistry::default();
        registry.register(first);
        registry.register(second);
        registry.register(third);
        write_registry(&registry);

        let report = cleanup_stale_managed_servers();
        assert_eq!(report.cleaned, 0);
        assert!(
            report
                .entries
                .iter()
                .any(|entry| entry.outcome == "missing-version-metadata")
        );
        assert!(
            report
                .entries
                .iter()
                .any(|entry| entry.outcome == "duplicate-pid")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn managed_older_server_is_gracefully_terminated_and_removed() {
        let _lock = env_lock();
        let home = tempfile::tempdir().expect("home");
        let runtime = tempfile::tempdir().expect("runtime");
        let _env = EnvGuard::new(home.path(), runtime.path());
        let versions = home.path().join("builds/versions/old-version");
        std::fs::create_dir_all(&versions).expect("version dir");
        let binary = versions.join("jcode");

        let ready = runtime.path().join("graceful-ready");
        let (mut child, pid) = spawn_managed_child(&binary, &ready, false);
        let mut registry = ServerRegistry::default();
        registry.register(registry_info(pid, super::super::socket_path()));
        write_registry(&registry);

        let report = cleanup_stale_managed_servers();
        assert_eq!(report.cleaned, 1, "report={report:?}");
        assert!(matches!(
            report.entries[0].outcome.as_str(),
            "graceful-exit" | "escalated-exit"
        ));
        child
            .wait()
            .expect("managed cleanup fixture should be waitable");
        assert!(!crate::platform::is_process_running(pid));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn managed_server_ignoring_term_is_escalated_after_bounded_wait() {
        let _lock = env_lock();
        let home = tempfile::tempdir().expect("home");
        let runtime = tempfile::tempdir().expect("runtime");
        let _env = EnvGuard::new(home.path(), runtime.path());
        let versions = home.path().join("builds/versions/old-version");
        std::fs::create_dir_all(&versions).expect("version dir");
        let binary = versions.join("jcode");

        let ready = runtime.path().join("escalation-ready");
        let (mut child, pid) = spawn_managed_child(&binary, &ready, true);
        let mut registry = ServerRegistry::default();
        registry.register(registry_info(pid, super::super::socket_path()));
        write_registry(&registry);

        let started = Instant::now();
        let report = cleanup_stale_managed_servers();
        assert!(started.elapsed() < Duration::from_secs(4));
        assert_eq!(report.cleaned, 1, "report={report:?}");
        assert_eq!(report.entries[0].outcome, "escalated-exit");
        child
            .wait()
            .expect("managed cleanup fixture should be waitable");
        assert!(!crate::platform::is_process_running(pid));

        let repeated = cleanup_stale_managed_servers();
        assert_eq!(repeated.scanned, 0, "report={repeated:?}");
        assert_eq!(repeated.cleaned, 0);
    }

    #[cfg(target_os = "linux")]
    fn spawn_managed_child(
        binary: &Path,
        ready: &Path,
        ignore_term: bool,
    ) -> (std::process::Child, u32) {
        std::fs::copy("/bin/sh", binary).expect("copy shell fixture");
        let script = if ignore_term {
            "trap '' TERM; : > \"$JCODE_MANAGED_CLEANUP_READY\"; while :; do sleep 60; done"
        } else {
            ": > \"$JCODE_MANAGED_CLEANUP_READY\"; while :; do sleep 60; done"
        };
        let mut command = Command::new(binary);
        command
            .arg("-c")
            .arg(script)
            .arg("serve")
            .env("JCODE_MANAGED_CLEANUP_READY", ready)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if ignore_term {
            command.env("JCODE_MANAGED_CLEANUP_IGNORE_TERM", "1");
        }
        use std::os::unix::process::CommandExt;
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn().expect("spawn managed server fixture");
        let pid = child.id();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !ready.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(ready.exists(), "managed cleanup fixture did not start");
        (child, pid)
    }
}
