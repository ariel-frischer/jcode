use super::generation::{GenerationProviderFactory, GenerationRouteFacts};
use super::{DefaultSessionLibrarian, LibrarianInvocation, LibrarianResult, SessionLibrarian};
use crate::message::{ContentBlock, Message, Role, StreamEvent, ToolDefinition};
use crate::provider::{EventStream, Provider};
use async_trait::async_trait;
use futures::stream;
use jcode_base::config::{Config, LibrarianRouteIdentity};
use jcode_base::session::Session;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

const RESPONSE: &str = r#"{
  "summary": {
    "goal": "Complete one bounded session summary.",
    "outcomes": ["The one-shot orchestrator completed."],
    "decisions": ["Reuse immutable artifacts for equivalent content."],
    "unresolved_work": [],
    "risks": ["Never publish partial output."],
    "next_steps": ["Continue from the handoff brief."]
  },
  "handoff_brief": "Continue with the published bounded summary.",
  "relevant_files": ["crates/jcode-app-core/src/session_librarian/mod.rs"]
}"#;

#[derive(Clone)]
struct FakeProvider {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl Provider for FakeProvider {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[ToolDefinition],
        _system: &str,
        _resume_session_id: Option<&str>,
    ) -> anyhow::Result<EventStream> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(Box::pin(stream::iter(vec![
            Ok(StreamEvent::TextDelta(RESPONSE.into())),
            Ok(StreamEvent::TokenUsage {
                input_tokens: Some(40),
                output_tokens: Some(20),
                cache_read_input_tokens: None,
                cache_creation_input_tokens: None,
            }),
            Ok(StreamEvent::MessageEnd { stop_reason: None }),
        ])))
    }

    fn name(&self) -> &str {
        "openai-oauth"
    }

    fn model(&self) -> String {
        "gpt-5.6-luna".into()
    }

    fn reasoning_effort(&self) -> Option<String> {
        Some("xhigh".into())
    }

    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(self.clone())
    }
}

struct FakeFactory {
    calls: Arc<AtomicUsize>,
}

impl GenerationProviderFactory for FakeFactory {
    fn inspect(&self, _route: &LibrarianRouteIdentity) -> GenerationRouteFacts {
        GenerationRouteFacts {
            supported: true,
            authentication_available: true,
            runtime_registered: true,
            input_cost_micros_per_million_tokens: Some(1_000_000),
            output_cost_micros_per_million_tokens: Some(1_000_000),
        }
    }

    fn build(&self, _route: &LibrarianRouteIdentity) -> Option<Arc<dyn Provider>> {
        Some(Arc::new(FakeProvider {
            calls: Arc::clone(&self.calls),
        }))
    }
}

fn session() -> Session {
    let mut session = Session::create_with_id("orchestration-session".into(), None, None);
    session.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "Summarize the completed librarian integration.".into(),
            cache_control: None,
        }],
    );
    session
}

#[tokio::test]
async fn equivalent_invocations_reuse_one_generation_and_changed_content_publishes_another() {
    let temp = tempfile::tempdir().expect("orchestration tempdir");
    let calls = Arc::new(AtomicUsize::new(0));
    let librarian = DefaultSessionLibrarian::with_components(
        Config::default(),
        Arc::new(FakeFactory {
            calls: Arc::clone(&calls),
        }),
        temp.path().to_path_buf(),
    );
    let mut source = session();

    let first = librarian
        .invoke(LibrarianInvocation::current(&source, Default::default()))
        .await;
    let second = librarian
        .invoke(LibrarianInvocation::current(&source, Default::default()))
        .await;

    let (first, second) = match (first, second) {
        (LibrarianResult::Succeeded(first), LibrarianResult::Reused(second)) => (first, second),
        outcomes => panic!("expected succeeded then reused, got {outcomes:?}"),
    };
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(first.source_fingerprint, second.source_fingerprint);
    assert_eq!(second.usage.request_count, 0);
    assert!(first.artifacts.markdown().is_file());
    assert!(first.artifacts.json().is_file());

    source.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "New admitted decision changes the immutable generation.".into(),
            cache_control: None,
        }],
    );
    let changed = librarian
        .invoke(LibrarianInvocation::current(&source, Default::default()))
        .await;
    let LibrarianResult::Succeeded(changed) = changed else {
        panic!("changed admitted content should generate a new artifact");
    };

    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_ne!(first.source_fingerprint, changed.source_fingerprint);
    assert_ne!(first.artifacts.directory(), changed.artifacts.directory());
    assert!(changed.artifacts.markdown().is_file());
    assert!(changed.artifacts.json().is_file());
}
