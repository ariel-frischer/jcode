use super::sidecar_calls::{name_cluster_with_client, run_final_extraction_with_sidecar};
use super::*;
use crate::message::{Message, StreamEvent, ToolDefinition};
use crate::provider::{EventStream, Provider};
use crate::sidecar::{MemoryOperationKind as Kind, Sidecar};
use jcode_session_types::memory_usage::MemoryRequestObservation;
use std::sync::Arc;

#[derive(Clone)]
struct MemoryFixture;

#[async_trait::async_trait]
impl Provider for MemoryFixture {
    async fn complete(
        &self,
        _: &[Message],
        _: &[ToolDefinition],
        system: &str,
        _: Option<&str>,
    ) -> Result<EventStream> {
        let text = if system.contains("relevance checker") {
            "RELEVANT: yes\nREASON: fixture"
        } else if system.contains("contradiction detector") {
            "YES"
        } else if system.contains("name memory clusters") {
            "fixture cluster"
        } else {
            "[]"
        };
        Ok(Box::pin(futures::stream::iter(vec![
            Ok(StreamEvent::TokenUsage {
                input_tokens: Some(8),
                output_tokens: Some(2),
                cache_read_input_tokens: Some(0),
                cache_creation_input_tokens: None,
                reported_cost_usd: None,
            }),
            Ok(StreamEvent::TextDelta(text.into())),
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

fn sidecar(tx: tokio::sync::mpsc::Sender<MemoryRequestObservation>) -> Sidecar {
    Sidecar::test_provider(Arc::new(MemoryFixture)).with_observation_sender(tx)
}

#[tokio::test]
async fn operation_attribution_interleaves_sessions_votes_and_detached_final() {
    let _lock = crate::storage::lock_test_env();
    let home = tempfile::tempdir().unwrap();
    let prior = std::env::var_os("JCODE_HOME");
    crate::env::set_var("JCODE_HOME", home.path());
    let (tx, mut rx) = tokio::sync::mpsc::channel(32);
    let run = |session: &'static str| {
        let client = sidecar(tx.clone()).with_memory_operation(Some(session), Kind::Unattributed);
        let final_client = sidecar(tx.clone());
        let working_dir = home.path().to_string_lossy().into_owned();
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
                crate::memory::MemoryEntry::new(
                    crate::memory::MemoryCategory::Fact,
                    "PRIVATE_CANDIDATE",
                ),
                1.0,
            )];
            let (_, outcome) = crate::memory_rerank::rerank_candidates_consensus_attributed(
                &client,
                "PRIVATE_QUERY",
                candidates,
                2,
                2,
            )
            .await;
            assert_eq!(outcome, crate::memory_rerank::RerankOutcome::Judged);
            assert_eq!(
                name_cluster_with_client(&["PRIVATE_MEMBER".into()], Some(session), &client)
                    .await
                    .unwrap(),
                "fixture cluster"
            );
            // Owned session identity moves into the actual detached final helper.
            let task = tokio::spawn(run_final_extraction_with_sidecar(
                "PRIVATE_TRANSCRIPT".into(),
                session.into(),
                Some(working_dir),
                final_client,
            ));
            drop(client);
            task.await.unwrap();
        }
    };
    tokio::join!(run("session-a"), run("session-b"));
    sidecar(tx.clone())
        .check_relevance("PRIVATE_MEMORY", "PRIVATE_CONTEXT")
        .await
        .unwrap();
    let records: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    if let Some(prior) = prior {
        crate::env::set_var("JCODE_HOME", prior);
    } else {
        crate::env::remove_var("JCODE_HOME");
    }
    assert_eq!(records.len(), 15);
    let mut ids = std::collections::HashSet::new();
    for session in ["session-a", "session-b"] {
        let owned: Vec<_> = records
            .iter()
            .filter(|r| r.context.session_id.as_deref() == Some(session))
            .collect();
        assert_eq!(owned.len(), 7, "all actual operations retain their owner");
        for kind in [
            Kind::Relevance,
            Kind::IncrementalExtraction,
            Kind::ContradictionCheck,
            Kind::ClusterNaming,
            Kind::FinalExtraction,
        ] {
            assert_eq!(
                owned
                    .iter()
                    .filter(|r| r.context.operation_kind == kind)
                    .count(),
                1,
                "{kind:?}"
            );
        }
        let votes: Vec<_> = owned
            .iter()
            .filter(|r| r.context.operation_kind == Kind::Rerank)
            .collect();
        assert_eq!(votes.len(), 2);
        assert_eq!(votes[0].context.operation_id, votes[1].context.operation_id);
        assert!(ids.insert(votes[0].context.operation_id.clone()));
    }
    let ownerless: Vec<_> = records
        .iter()
        .filter(|r| r.context.session_id.is_none())
        .collect();
    assert_eq!(ownerless.len(), 1);
    assert_eq!(ownerless[0].context.operation_kind, Kind::Relevance);
    let requests: std::collections::HashSet<_> = records.iter().map(|r| &r.request_id).collect();
    assert_eq!(requests.len(), records.len());
    assert_eq!(
        records
            .iter()
            .map(|r| r.usage.total_tokens().unwrap().unwrap())
            .sum::<u64>(),
        150
    );
    assert!(
        !serde_json::to_string(&records)
            .unwrap()
            .contains("PRIVATE_")
    );
}

#[tokio::test]
async fn skipped_empty_consensus_has_no_observation() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let client = sidecar(tx);
    crate::memory_rerank::rerank_candidates_consensus_attributed(&client, "private", vec![], 2, 2)
        .await;
    assert!(rx.try_recv().is_err());
}
