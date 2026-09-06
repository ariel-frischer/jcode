//! Production (non-cfg(test)) sidecar and default recorder, in a private child.
use super::{json, network_guard, snapshot};
use jcode::{
    message::{Message, StreamEvent, ToolDefinition},
    provider::{EventStream, Provider},
    sidecar::{MemoryOperationKind, Sidecar},
};
use std::{
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};

#[derive(Clone)]
struct OfflineProvider(Arc<AtomicUsize>);
#[async_trait::async_trait]
impl Provider for OfflineProvider {
    async fn complete(
        &self,
        _: &[Message],
        _: &[ToolDefinition],
        system: &str,
        _: Option<&str>,
    ) -> anyhow::Result<EventStream> {
        self.0.fetch_add(1, Ordering::Relaxed);
        let answer = if system.contains("relevance checker") {
            "RELEVANT: yes\nREASON: fixture"
        } else if system.contains("contradiction detector") {
            "YES"
        } else {
            "[]"
        };
        let usage = || StreamEvent::TokenUsage {
            input_tokens: Some(8),
            output_tokens: Some(2),
            cache_read_input_tokens: Some(0),
            cache_creation_input_tokens: None,
            reported_cost_usd: None,
        };
        Ok(Box::pin(futures::stream::iter(vec![
            Ok(usage()),
            Ok(usage()),
            Ok(StreamEvent::TextDelta(answer.into())),
            Ok(StreamEvent::MessageEnd { stop_reason: None }),
        ])))
    }
    fn name(&self) -> &str {
        "fixture"
    }
    fn model(&self) -> String {
        "fixture-model".into()
    }
    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(self.clone())
    }
}

#[test]
fn production_recorder_child() {
    if std::env::var_os("JCODE_USAGE_FIXTURE_CHILD").is_none() {
        return;
    }
    let requests = Arc::new(AtomicUsize::new(0));
    jcode::provider::set_active_provider(Arc::new(OfflineProvider(requests.clone())));
    let start = Instant::now();
    let client = Sidecar::new(); // No injected recorder or observation channel.
    let cold_ns = start.elapsed().as_nanos();
    assert_eq!(client.backend_name(), "provider");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let run = |session| {
            let client = client
                .clone()
                .with_memory_operation(Some(session), MemoryOperationKind::Unattributed);
            async move {
                assert!(
                    client
                        .check_relevance("PRIVATE_MEMORY", "PRIVATE_CONTEXT")
                        .await
                        .unwrap()
                        .0
                );
                assert!(
                    client
                        .extract_memories("PRIVATE_TRANSCRIPT")
                        .await
                        .unwrap()
                        .is_empty()
                );
                assert!(
                    client
                        .check_contradiction("PRIVATE_NEW", "PRIVATE_OLD")
                        .await
                        .unwrap()
                );
                let candidates = vec![(
                    jcode::memory::MemoryEntry::new(
                        jcode::memory::MemoryCategory::Fact,
                        "PRIVATE_CANDIDATE",
                    ),
                    1.0,
                )];
                let (_, outcome) = jcode::memory_rerank::rerank_candidates_consensus_attributed(
                    &client,
                    "PRIVATE_QUERY",
                    candidates,
                    2,
                    2,
                )
                .await;
                assert_eq!(outcome, jcode::memory_rerank::RerankOutcome::Judged);
            }
        };
        tokio::join!(run("session-a"), run("session-b"));
    });
    assert_eq!(requests.load(Ordering::Relaxed), 10, "no added inference");
    assert!(jcode::memory_usage::flush_global(
        jcode::memory_usage::MAX_FLUSH_WAIT
    ));
    let state = jcode::memory_usage::global_snapshot().unwrap();
    assert_eq!(state.accepted, 10);
    assert_eq!(state.dropped, 0);
    eprintln!("production default recorder cold constructor ns={cold_ns}; physical mock calls=10");
}

#[test]
fn production_default_recorder_reconciles_real_operations_in_new_cli() {
    for enabled in [false, true] {
        for persist in [false, true] {
            for logs in [false, true] {
                let home = tempfile::tempdir().unwrap();
                let mut command = Command::new(std::env::current_exe().unwrap());
                command
                    .env_clear()
                    .env("HOME", home.path())
                    .env("JCODE_HOME", home.path())
                    .env("XDG_CONFIG_HOME", home.path())
                    .env("XDG_DATA_HOME", home.path())
                    .env("JCODE_USAGE_FIXTURE_CHILD", "1")
                    .env("JCODE_NO_TELEMETRY", "1")
                    .env("DO_NOT_TRACK", "1")
                    .env("JCODE_LIFECYCLE_OBSERVABILITY_ENABLED", enabled.to_string())
                    .env(
                        "JCODE_LIFECYCLE_OBSERVABILITY_PERSIST_SESSION_EVENTS",
                        persist.to_string(),
                    )
                    .env(
                        "JCODE_LIFECYCLE_OBSERVABILITY_EMIT_STRUCTURED_LOGS",
                        logs.to_string(),
                    )
                    .current_dir(home.path())
                    .args([
                        "--exact",
                        "runtime::production_recorder_child",
                        "--nocapture",
                    ]);
                network_guard::deny_network(&mut command);
                let output = command.output().unwrap();
                assert!(
                    output.status.success(),
                    "private fixture failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                eprintln!(
                    "controls={enabled}/{persist}/{logs}: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                );
                let before = snapshot(home.path());
                let report = json(home.path(), &["memory", "usage", "--calls", "--json"]);
                assert_eq!(snapshot(home.path()), before, "CLI must remain read-only");
                let calls = report["calls"].as_array().unwrap();
                assert_eq!(calls.len(), if enabled && persist { 10 } else { 0 });
                assert!(!report.to_string().contains("PRIVATE_"));
                if enabled && persist {
                    for session in report["sessions"].as_array().unwrap() {
                        assert_eq!(session["calls"], 5);
                        assert_eq!(session["tokens"]["input_tokens"]["known_subtotal"], 40);
                        assert_eq!(session["tokens"]["output_tokens"]["known_subtotal"], 10);
                        assert_eq!(session["unknown_cost_calls"], 5);
                    }
                    assert_eq!(report["sessions"].as_array().unwrap().len(), 2);
                }
            }
        }
    }
}
