//! Runs the newly built CLI, never PATH's jcode or the shared daemon.
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

#[path = "memory_usage_cli/network_guard.rs"]
mod network_guard;
#[path = "memory_usage_cli/runtime.rs"]
mod runtime;

fn invoke(home: &Path, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_jcode"));
    command
        .env_clear()
        .env("HOME", home)
        .env("JCODE_HOME", home)
        .env("XDG_CONFIG_HOME", home)
        .env("XDG_DATA_HOME", home)
        .env("XDG_RUNTIME_DIR", home)
        .env("JCODE_NO_TELEMETRY", "1")
        .env("DO_NOT_TRACK", "1")
        .env("HTTP_PROXY", "http://127.0.0.1:9")
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .current_dir(home)
        .args(["--no-update", "--no-selfdev"])
        .args(args);
    network_guard::deny_network(&mut command);
    command.output().expect("execute worktree-built jcode")
}
fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
fn json(home: &Path, args: &[&str]) -> Value {
    serde_json::from_str(&success(invoke(home, args))).expect("standalone deterministic JSON")
}
fn snapshot(path: &Path) -> BTreeMap<PathBuf, (std::time::SystemTime, Option<Vec<u8>>)> {
    let mut files = BTreeMap::new();
    files.insert(
        path.to_path_buf(),
        (fs::metadata(path).unwrap().modified().unwrap(), None),
    );
    for item in fs::read_dir(path).unwrap() {
        let item = item.unwrap();
        if item.file_type().unwrap().is_dir() {
            files.extend(snapshot(&item.path()));
        } else {
            files.insert(
                item.path(),
                (
                    item.metadata().unwrap().modified().unwrap(),
                    Some(fs::read(item.path()).unwrap()),
                ),
            );
        }
    }
    files
}
fn seed(home: &Path) {
    let calls: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/memory_usage/calls.json")).unwrap();
    let now = chrono::Utc::now().to_rfc3339();
    let mut lines = String::new();
    for mut call in calls {
        call["recorded_at"] = now.clone().into();
        lines.push_str(&serde_json::to_string(&call).unwrap());
        lines.push('\n');
    }
    lines.push_str(include_str!("fixtures/memory_usage/corrupt.jsonl"));
    let dir = home.join("memory-usage");
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("writer.lock"), "").unwrap();
    fs::write(dir.join("requests.v1.jsonl"), lines).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        for name in ["writer.lock", "requests.v1.jsonl"] {
            fs::set_permissions(dir.join(name), fs::Permissions::from_mode(0o600)).unwrap();
        }
    }
}

#[test]
fn help_discovers_usage_and_options_without_startup_writes() {
    let home = tempfile::tempdir().unwrap();
    let help = success(invoke(home.path(), &["memory", "--help"]));
    assert!(help.contains("usage"));
    let help = success(invoke(home.path(), &["memory", "usage", "--help"]));
    for flag in ["--session", "--calls", "--json"] {
        assert!(help.contains(flag));
    }
    assert_eq!(
        snapshot(home.path()).len(),
        1,
        "read-only help starts no logging/config/telemetry"
    );
}

#[test]
fn empty_history_is_unavailable_not_lifetime_zero_and_creates_no_files() {
    let home = tempfile::tempdir().unwrap();
    let report = json(home.path(), &["memory", "usage", "--json"]);
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["sessions"], serde_json::json!([]));
    assert_eq!(report["coverage"], "unavailable");
    assert!(report.get("calls").is_none());
    assert!(
        report["storage_warnings"]
            .as_array()
            .unwrap()
            .contains(&"loss_history_unavailable".into())
    );
    let text = success(invoke(home.path(), &["memory", "usage"]));
    assert!(text.contains("No retained observations"));
    assert!(text.contains("not lifetime"));
    assert_eq!(snapshot(home.path()).len(), 1);
}

#[test]
fn json_calls_sessions_unknowns_and_zero_reconcile_deterministically() {
    let home = tempfile::tempdir().unwrap();
    seed(home.path());
    let before = snapshot(home.path());
    let args = ["memory", "usage", "--calls", "--json"];
    let first = success(invoke(home.path(), &args));
    assert_eq!(first, success(invoke(home.path(), &args)));
    assert!(!first.contains("PRIVATE_"));
    let report: Value = serde_json::from_str(&first).unwrap();
    assert_eq!(report["calls"].as_array().unwrap().len(), 6);
    let ids: Vec<_> = report["calls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["request_id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        [
            "r-known",
            "r-luna",
            "r-ownerless",
            "r-partial",
            "r-pro",
            "r-zero"
        ]
    );
    assert_eq!(report["calls"][0]["pricing"]["estimate_nano_usd"], 355_000);
    assert!(report["calls"][0]["usage"]["cache_creation_tokens"].is_null());
    assert!(report["calls"][1]["pricing"]["estimate_nano_usd"].is_null());
    assert_eq!(report["calls"][2]["attempt_coverage"], "provider_call_only");
    assert!(report["calls"][3]["usage"]["output_tokens"].is_null());
    assert_eq!(report["calls"][5]["pricing"]["estimate_nano_usd"], 0);
    assert_eq!(report["sessions"][0]["session_id"], Value::Null);
    assert_eq!(report["sessions"][1]["session_id"], "session-a");
    assert_eq!(report["sessions"][1]["calls"], 3);
    assert_eq!(
        report["sessions"][1]["tokens"]["input_tokens"]["known_subtotal"],
        300
    );
    assert_eq!(
        report["sessions"][1]["known_cost_subtotal_nano_usd"],
        4_555_000
    );
    assert_eq!(report["sessions"][1]["unknown_cost_calls"], 2);
    assert!(
        report["pricing_policy"]
            .as_str()
            .unwrap()
            .contains("not actual billed")
    );
    assert_eq!(
        snapshot(home.path()),
        before,
        "no telemetry/config/log/retention mutation"
    );
}

#[test]
fn session_filter_text_metadata_and_safe_invalid_selectors() {
    let home = tempfile::tempdir().unwrap();
    seed(home.path());
    let report = json(
        home.path(),
        &[
            "memory",
            "usage",
            "--session",
            "session-b",
            "--calls",
            "--json",
        ],
    );
    assert_eq!(report["sessions"].as_array().unwrap().len(), 1);
    assert_eq!(report["calls"].as_array().unwrap().len(), 2);
    let text = success(invoke(home.path(), &["memory", "usage", "--calls"]));
    for label in [
        "API-equivalent",
        "known subtotal",
        "unknown",
        "unattributed",
        "retained",
        "Controls",
        "gpt-5.6-luna",
        "xhigh",
        "provider_call_only",
        "r-known",
        "operation-fixture",
    ] {
        assert!(text.contains(label), "missing text label {label}");
    }
    assert!(!text.contains("PRIVATE_"));
    let missing = json(
        home.path(),
        &["memory", "usage", "--session", "absent", "--json"],
    );
    assert_eq!(missing["coverage"], "unavailable");
    for bad in [
        "../PRIVATE_SELECTOR",
        "bad/PRIVATE_SELECTOR",
        "",
        "bad PRIVATE_SELECTOR",
    ] {
        let result = invoke(
            home.path(),
            &["memory", "usage", "--session", bad, "--json"],
        );
        assert!(!result.status.success());
        let stderr = String::from_utf8(result.stderr).unwrap();
        assert!(stderr.contains("invalid session selector"));
        assert!(!stderr.contains("PRIVATE_SELECTOR"));
    }
    for args in [
        vec!["memory", "usage", "--session"],
        vec!["memory", "usage", "--unknown"],
        vec!["memory", "usage", "--calls", "true"],
    ] {
        assert!(!invoke(home.path(), &args).status.success());
    }
}

#[test]
fn effective_controls_and_malformed_config_do_not_leak_or_migrate() {
    let home = tempfile::tempdir().unwrap();
    seed(home.path());
    let config = home.path().join("config.toml");
    for enabled in [false, true] {
        for persist in [false, true] {
            for logs in [false, true] {
                fs::write(&config, format!("[lifecycle_observability]\nenabled = {enabled}\npersist_session_events = {persist}\nemit_structured_logs = {logs}\n")).unwrap();
                let before = snapshot(home.path());
                let report = json(home.path(), &["memory", "usage", "--json"]);
                assert_eq!(report["controls"]["enabled"], enabled);
                assert_eq!(
                    report["controls"]["persist_session_events"],
                    enabled && persist
                );
                assert_eq!(report["controls"]["emit_structured_logs"], enabled && logs);
                assert_eq!(
                    report["sessions"].as_array().unwrap().len(),
                    3,
                    "disabled now does not erase retained observations"
                );
                assert_eq!(snapshot(home.path()), before);
            }
        }
    }
    fs::write(&config, "PRIVATE_CONFIG_SECRET = [ malformed").unwrap();
    let before = snapshot(home.path());
    let output = invoke(home.path(), &["memory", "usage", "--json"]);
    assert!(!output.status.success());
    assert!(
        !String::from_utf8(output.stderr)
            .unwrap()
            .contains("PRIVATE_CONFIG_SECRET")
    );
    assert_eq!(snapshot(home.path()), before);
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn network_guard_denies_socket_creation() {
    use std::os::unix::process::ExitStatusExt;
    let mut child = Command::new(std::env::current_exe().unwrap());
    child
        .env_clear()
        .env("JCODE_USAGE_SOCKET_PROBE", "1")
        .args(["--exact", "network_guard_socket_probe", "--nocapture"]);
    network_guard::deny_network(&mut child);
    assert_eq!(child.output().unwrap().status.signal(), Some(libc::SIGSYS));
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn network_guard_socket_probe() {
    if std::env::var_os("JCODE_USAGE_SOCKET_PROBE").is_some() {
        // No send or connection. The test guard must kill before socket creation.
        let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
        if fd >= 0 {
            unsafe {
                libc::close(fd);
            }
        }
    }
}
