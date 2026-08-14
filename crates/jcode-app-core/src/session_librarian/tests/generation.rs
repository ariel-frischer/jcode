use super::generation::{GenerationProviderFactory, GenerationRouteFacts, generate_summary};
use super::{AdmittedSessionContent, LibrarianFailureStage};
use crate::message::{Message, StreamEvent, ToolDefinition};
use crate::provider::{EventStream, Provider};
use async_trait::async_trait;
use futures::stream;
use jcode_base::config::{
    Config, LibrarianInvocationOverrides, LibrarianRouteIdentity, LibrarianRouteValidation,
    ResolvedLibrarianConfig, resolve_librarian_config,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

const SAFE_PAYLOAD: &str = r#"{"format_version":"session-librarian-admission.v1","items":[{"kind":"text","text":"safe decision"}]}"#;
const VALID_RESPONSE: &str = r#"{
  "summary": {
    "goal": "Finish bounded librarian generation.",
    "outcomes": ["The provider response stayed within hard limits."],
    "decisions": ["Use an independent native OAuth route."],
    "unresolved_work": ["Integrate generation into orchestration."],
    "risks": ["Malformed output must fail closed."],
    "next_steps": ["Validate and publish the structured response."]
  },
  "handoff_brief": "Continue with bounded generation integration.",
  "relevant_files": ["crates/jcode-app-core/src/session_librarian/generation.rs"]
}"#;

#[derive(Clone, Debug)]
enum FakeBehavior {
    Response {
        body: String,
        input_tokens: u64,
        output_tokens: u64,
    },
    ProviderFailure(String),
    Delayed(Duration),
}

#[derive(Clone)]
struct FakeProvider {
    route: LibrarianRouteIdentity,
    behavior: FakeBehavior,
    calls: Arc<AtomicUsize>,
    transmitted: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl Provider for FakeProvider {
    async fn complete(
        &self,
        messages: &[Message],
        _tools: &[ToolDefinition],
        system: &str,
        _resume_session_id: Option<&str>,
    ) -> anyhow::Result<EventStream> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.transmitted
            .lock()
            .expect("transmission lock")
            .push(format!("{system}\n{messages:?}"));

        match &self.behavior {
            FakeBehavior::Response {
                body,
                input_tokens,
                output_tokens,
            } => {
                let events = vec![
                    Ok(StreamEvent::TextDelta(body.clone())),
                    Ok(StreamEvent::TokenUsage {
                        input_tokens: Some(*input_tokens),
                        output_tokens: Some(*output_tokens),
                        cache_read_input_tokens: None,
                        cache_creation_input_tokens: None,
                    }),
                    Ok(StreamEvent::MessageEnd { stop_reason: None }),
                ];
                Ok(Box::pin(stream::iter(events)))
            }
            FakeBehavior::ProviderFailure(message) => Err(anyhow::anyhow!(message.clone())),
            FakeBehavior::Delayed(delay) => {
                tokio::time::sleep(*delay).await;
                Ok(Box::pin(stream::iter(vec![
                    Ok(StreamEvent::TextDelta(VALID_RESPONSE.into())),
                    Ok(StreamEvent::MessageEnd { stop_reason: None }),
                ])))
            }
        }
    }

    fn name(&self) -> &str {
        &self.route.provider
    }

    fn model(&self) -> String {
        self.route.model.clone()
    }

    fn reasoning_effort(&self) -> Option<String> {
        Some(self.route.reasoning_effort.clone())
    }

    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(self.clone())
    }
}

#[derive(Clone)]
struct FakeFactory {
    facts: GenerationRouteFacts,
    behavior: FakeBehavior,
    builds: Arc<AtomicUsize>,
    calls: Arc<AtomicUsize>,
    transmitted: Arc<Mutex<Vec<String>>>,
}

impl FakeFactory {
    fn available(behavior: FakeBehavior) -> Self {
        Self {
            facts: GenerationRouteFacts {
                supported: true,
                authentication_available: true,
                runtime_registered: true,
                input_cost_micros_per_million_tokens: Some(2_000_000),
                output_cost_micros_per_million_tokens: Some(8_000_000),
            },
            behavior,
            builds: Arc::new(AtomicUsize::new(0)),
            calls: Arc::new(AtomicUsize::new(0)),
            transmitted: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn provider(&self, route: LibrarianRouteIdentity) -> FakeProvider {
        FakeProvider {
            route,
            behavior: self.behavior.clone(),
            calls: Arc::clone(&self.calls),
            transmitted: Arc::clone(&self.transmitted),
        }
    }
}

impl GenerationProviderFactory for FakeFactory {
    fn inspect(&self, _route: &LibrarianRouteIdentity) -> GenerationRouteFacts {
        self.facts.clone()
    }

    fn build(&self, route: &LibrarianRouteIdentity) -> Option<Arc<dyn Provider>> {
        self.builds.fetch_add(1, Ordering::SeqCst);
        self.facts
            .runtime_registered
            .then(|| Arc::new(self.provider(route.clone())) as Arc<dyn Provider>)
    }
}

fn active_route() -> LibrarianRouteIdentity {
    LibrarianRouteIdentity {
        provider: "anthropic-oauth".into(),
        model: "claude-opus-4-1".into(),
        reasoning_effort: "high".into(),
    }
}

fn admitted(input_tokens: u32) -> AdmittedSessionContent {
    AdmittedSessionContent {
        session_id: "generation-test-session".into(),
        canonical_payload: SAFE_PAYLOAD.as_bytes().to_vec(),
        input_tokens,
    }
}

fn resolve(
    overrides: LibrarianInvocationOverrides,
    factory: &FakeFactory,
) -> Result<ResolvedLibrarianConfig, jcode_base::config::LibrarianConfigError> {
    resolve_librarian_config(
        &Config::default(),
        &overrides,
        &active_route(),
        |route, budgets| {
            let facts = factory.inspect(route);
            LibrarianRouteValidation {
                supported: facts.supported,
                authentication_available: facts.authentication_available,
                worst_case_cost_micros: facts
                    .worst_case_cost_micros(budgets.max_input_tokens, budgets.max_output_tokens),
            }
        },
    )
}

fn valid_behavior() -> FakeBehavior {
    FakeBehavior::Response {
        body: VALID_RESPONSE.into(),
        input_tokens: 640,
        output_tokens: 420,
    }
}

#[tokio::test]
async fn default_and_override_routes_are_independent_from_the_active_agent_route() {
    let factory = FakeFactory::available(valid_behavior());
    let original_active_route = active_route();
    let defaults = resolve(LibrarianInvocationOverrides::default(), &factory)
        .expect("default librarian route should resolve");

    assert_eq!(defaults.route.provider, "openai-oauth");
    assert_eq!(defaults.route.model, "gpt-5.6-luna");
    assert_eq!(defaults.route.reasoning_effort, "xhigh");
    generate_summary(&factory, &defaults, &admitted(640))
        .await
        .expect("default fake generation should succeed");

    let overridden = resolve(
        LibrarianInvocationOverrides {
            provider: Some("fixture-oauth".into()),
            model: Some("fixture-model".into()),
            reasoning_effort: Some("medium".into()),
            ..LibrarianInvocationOverrides::default()
        },
        &factory,
    )
    .expect("explicit librarian override should resolve");
    generate_summary(&factory, &overridden, &admitted(640))
        .await
        .expect("overridden fake generation should succeed");

    assert_eq!(factory.builds.load(Ordering::SeqCst), 2);
    assert_eq!(factory.calls.load(Ordering::SeqCst), 2);
    assert_eq!(active_route(), original_active_route);
}

#[tokio::test]
async fn preflight_rejections_transmit_nothing_and_never_build_an_unusable_provider() {
    let cases = [
        (
            "missing authentication",
            LibrarianInvocationOverrides::default(),
            false,
            true,
            true,
            Some(2_000_000),
            Some(8_000_000),
        ),
        (
            "unsupported provider",
            LibrarianInvocationOverrides {
                provider: Some("unsupported-provider".into()),
                ..LibrarianInvocationOverrides::default()
            },
            true,
            false,
            true,
            Some(2_000_000),
            Some(8_000_000),
        ),
        (
            "unsupported model",
            LibrarianInvocationOverrides {
                model: Some("unsupported-model".into()),
                ..LibrarianInvocationOverrides::default()
            },
            true,
            false,
            true,
            Some(2_000_000),
            Some(8_000_000),
        ),
        (
            "unsupported effort",
            LibrarianInvocationOverrides {
                reasoning_effort: Some("unsupported-effort".into()),
                ..LibrarianInvocationOverrides::default()
            },
            true,
            false,
            true,
            Some(2_000_000),
            Some(8_000_000),
        ),
        (
            "unavailable runtime",
            LibrarianInvocationOverrides::default(),
            true,
            true,
            false,
            Some(2_000_000),
            Some(8_000_000),
        ),
        (
            "unknown pricing",
            LibrarianInvocationOverrides::default(),
            true,
            true,
            true,
            None,
            Some(8_000_000),
        ),
    ];

    for (
        name,
        overrides,
        authentication_available,
        supported,
        runtime_registered,
        input_price,
        output_price,
    ) in cases
    {
        let mut factory = FakeFactory::available(valid_behavior());
        factory.facts.authentication_available = authentication_available;
        factory.facts.supported = supported;
        factory.facts.runtime_registered = runtime_registered;
        factory.facts.input_cost_micros_per_million_tokens = input_price;
        factory.facts.output_cost_micros_per_million_tokens = output_price;

        let outcome = match resolve(overrides, &factory) {
            Ok(config) => generate_summary(&factory, &config, &admitted(640))
                .await
                .map_err(|error| error.to_string()),
            Err(error) => Err(error.to_string()),
        };

        assert!(outcome.is_err(), "{name} must fail closed");
        assert_eq!(factory.calls.load(Ordering::SeqCst), 0, "{name}");
        assert!(
            factory
                .transmitted
                .lock()
                .expect("transmission lock")
                .is_empty(),
            "{name}"
        );
    }
}

#[tokio::test]
async fn excessive_worst_case_price_is_rejected_before_content_transmission() {
    let mut factory = FakeFactory::available(valid_behavior());
    factory.facts.input_cost_micros_per_million_tokens = Some(40_000_000);
    factory.facts.output_cost_micros_per_million_tokens = Some(40_000_000);

    let error = resolve(LibrarianInvocationOverrides::default(), &factory)
        .expect_err("worst-case exposure above USD 0.50 must be rejected");

    assert!(error.to_string().contains("exceeds the approved"));
    assert_eq!(factory.calls.load(Ordering::SeqCst), 0);
    assert!(
        factory
            .transmitted
            .lock()
            .expect("transmission lock")
            .is_empty()
    );
}

#[tokio::test]
async fn provider_failure_and_malformed_response_are_actionable_and_single_request() {
    let cases = [
        (
            "provider failure",
            FakeBehavior::ProviderFailure("fixture provider unavailable".into()),
        ),
        (
            "malformed response",
            FakeBehavior::Response {
                body: "not-json".into(),
                input_tokens: 640,
                output_tokens: 12,
            },
        ),
    ];

    for (name, behavior) in cases {
        let factory = FakeFactory::available(behavior);
        let config = resolve(LibrarianInvocationOverrides::default(), &factory)
            .expect("preflight should pass for runtime failure fixtures");
        let error = generate_summary(&factory, &config, &admitted(640))
            .await
            .expect_err(name);

        assert!(matches!(
            error.stage,
            LibrarianFailureStage::Generation | LibrarianFailureStage::Validation
        ));
        assert_eq!(factory.calls.load(Ordering::SeqCst), 1, "{name}");
        assert_eq!(
            factory.transmitted.lock().expect("transmission lock").len(),
            1,
            "{name}"
        );
        assert!(error.message.len() <= 512, "failure must remain bounded");
    }
}

#[tokio::test]
async fn output_request_and_deadline_limits_fail_closed_without_a_second_request() {
    let cases = [
        (
            "output limit",
            LibrarianInvocationOverrides {
                max_output_tokens: Some("10".into()),
                ..LibrarianInvocationOverrides::default()
            },
            FakeBehavior::Response {
                body: VALID_RESPONSE.into(),
                input_tokens: 640,
                output_tokens: 11,
            },
        ),
        (
            "deadline",
            LibrarianInvocationOverrides {
                deadline_seconds: Some("1".into()),
                ..LibrarianInvocationOverrides::default()
            },
            FakeBehavior::Delayed(Duration::from_secs(2)),
        ),
    ];

    for (name, overrides, behavior) in cases {
        let factory = FakeFactory::available(behavior);
        let config = resolve(overrides, &factory).expect("boundary fixture preflight");
        let error = generate_summary(&factory, &config, &admitted(640))
            .await
            .expect_err(name);

        assert_eq!(error.stage, LibrarianFailureStage::Generation, "{name}");
        assert_eq!(factory.calls.load(Ordering::SeqCst), 1, "{name}");
        if let Some(usage) = error.usage {
            assert!(usage.request_count <= config.budgets.max_requests);
            assert!(usage.output_tokens <= config.budgets.max_output_tokens);
            assert!(usage.cost_micros_usd <= config.budgets.max_cost_micros);
        }
    }

    let factory = FakeFactory::available(valid_behavior());
    let error = resolve(
        LibrarianInvocationOverrides {
            max_requests: Some("2".into()),
            ..LibrarianInvocationOverrides::default()
        },
        &factory,
    )
    .expect_err("the initial implementation supports exactly one request");
    assert!(error.to_string().contains("max_requests"));
    assert_eq!(factory.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn successful_generation_records_bounded_usage_and_only_admitted_content() {
    let factory = FakeFactory::available(valid_behavior());
    let config = resolve(LibrarianInvocationOverrides::default(), &factory)
        .expect("valid generation config");
    let generation = generate_summary(&factory, &config, &admitted(640))
        .await
        .expect("valid fake provider response");

    assert_eq!(factory.calls.load(Ordering::SeqCst), 1);
    assert_eq!(generation.usage.request_count, 1);
    assert!(generation.usage.input_tokens <= config.budgets.max_input_tokens);
    assert!(generation.usage.output_tokens <= config.budgets.max_output_tokens);
    assert!(generation.usage.cost_micros_usd <= config.budgets.max_cost_micros);
    let transmitted = factory.transmitted.lock().expect("transmission lock");
    assert_eq!(transmitted.len(), 1);
    assert!(transmitted[0].contains("safe decision"));
    assert!(transmitted[0].contains("handoff_brief"));
    assert!(transmitted[0].contains("relevant_files"));
}
