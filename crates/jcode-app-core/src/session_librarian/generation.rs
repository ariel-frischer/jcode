use super::{AdmittedSessionContent, LibrarianFailure, LibrarianFailureStage, LibrarianGeneration};
use crate::message::{Message, StreamEvent};
use crate::provider::Provider;
use futures::StreamExt;
use jcode_base::config::{LibrarianRouteIdentity, ResolvedLibrarianConfig};
use jcode_session_types::BoundedUsage;
use std::sync::Arc;
use std::time::{Duration, Instant};

const GENERATION_SYSTEM_PROMPT: &str = "Summarize the admitted Jcode session content. Return only the required session librarian JSON object.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GenerationRouteFacts {
    pub(crate) supported: bool,
    pub(crate) authentication_available: bool,
    pub(crate) runtime_registered: bool,
    pub(crate) input_cost_micros_per_million_tokens: Option<u64>,
    pub(crate) output_cost_micros_per_million_tokens: Option<u64>,
}

impl GenerationRouteFacts {
    pub(crate) fn worst_case_cost_micros(
        &self,
        input_tokens: u32,
        output_tokens: u32,
    ) -> Option<u64> {
        let input = token_cost_micros(self.input_cost_micros_per_million_tokens?, input_tokens);
        let output = token_cost_micros(self.output_cost_micros_per_million_tokens?, output_tokens);
        Some(input.saturating_add(output))
    }
}

pub(crate) trait GenerationProviderFactory: Send + Sync {
    fn inspect(&self, route: &LibrarianRouteIdentity) -> GenerationRouteFacts;
    fn build(&self, route: &LibrarianRouteIdentity) -> Option<Arc<dyn Provider>>;
}

/// Production factory for the independently configured native OpenAI OAuth route.
///
/// The concrete runtime remains registered by the binary composition root. This
/// module only inspects non-secret route facts and asks the existing base registry
/// for a fresh provider instance, so it never touches the active session provider
/// or its runtime state.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct NativeGenerationProviderFactory;

impl GenerationProviderFactory for NativeGenerationProviderFactory {
    fn inspect(&self, route: &LibrarianRouteIdentity) -> GenerationRouteFacts {
        let provider_supported = route.provider == "openai-oauth";
        let model_supported = jcode_base::provider::ALL_OPENAI_MODELS
            .iter()
            .any(|model| *model == route.model);
        let effort_supported =
            jcode_provider_core::inferred_reasoning_efforts(Some("openai"), Some(&route.model))
                .contains(&route.reasoning_effort.as_str());
        let pricing = (provider_supported && model_supported)
            .then(|| {
                jcode_base::provider::pricing::metered_pricing_for_source(
                    "openai:api-key",
                    &route.model,
                )
            })
            .flatten();

        GenerationRouteFacts {
            supported: provider_supported && model_supported && effort_supported,
            authentication_available: provider_supported
                && jcode_base::auth::codex::load_oauth_credentials().is_ok(),
            runtime_registered: provider_supported
                && jcode_base::provider::external::external_provider_registered(
                    jcode_base::provider::external::OPENAI_RUNTIME,
                ),
            input_cost_micros_per_million_tokens: pricing
                .as_ref()
                .and_then(|estimate| estimate.input_price_per_mtok_micros),
            output_cost_micros_per_million_tokens: pricing
                .as_ref()
                .and_then(|estimate| estimate.output_price_per_mtok_micros),
        }
    }

    fn build(&self, route: &LibrarianRouteIdentity) -> Option<Arc<dyn Provider>> {
        if !self.inspect(route).supported {
            return None;
        }
        let provider = jcode_base::provider::external::instantiate_external_provider(
            jcode_base::provider::external::OPENAI_RUNTIME,
        )?;
        provider.set_model(&route.model).ok()?;
        provider
            .set_reasoning_effort(&route.reasoning_effort)
            .ok()?;
        Some(provider)
    }
}

pub(crate) async fn generate_summary(
    factory: &dyn GenerationProviderFactory,
    config: &ResolvedLibrarianConfig,
    admitted: &AdmittedSessionContent,
) -> Result<LibrarianGeneration, LibrarianFailure> {
    let facts = factory.inspect(&config.route);
    preflight(&facts, config, admitted)?;
    let provider = factory.build(&config.route).ok_or_else(|| {
        failure(
            "librarian_provider_unavailable",
            "The independently configured librarian provider runtime could not be constructed.",
        )
    })?;

    let payload = std::str::from_utf8(&admitted.canonical_payload).map_err(|_| {
        failure(
            "librarian_admitted_payload_invalid",
            "The locally admitted librarian payload was not valid UTF-8.",
        )
    })?;
    let system_prompt = format!(
        "{GENERATION_SYSTEM_PROMPT} Do not exceed {} output tokens.",
        config.budgets.max_output_tokens
    );
    let started = Instant::now();
    let mut response_json = String::new();
    let mut input_tokens = admitted.input_tokens;
    let mut output_tokens = 0_u32;
    let mut input_usage_reported = false;
    let mut output_usage_reported = false;
    let deadline = Duration::from_secs(config.budgets.deadline_seconds);
    let request = tokio::time::timeout(deadline, async {
        let mut stream = provider
            .complete(&[Message::user(payload)], &[], &system_prompt, None)
            .await
            .map_err(|_| provider_failure())?;

        while let Some(event) = stream.next().await {
            match event.map_err(|_| provider_failure())? {
                StreamEvent::TextDelta(text) => {
                    let candidate_tokens = conservative_token_count(
                        response_json.len().saturating_add(text.len()),
                    );
                    if candidate_tokens > config.budgets.max_output_tokens {
                        output_tokens = config.budgets.max_output_tokens;
                        return Err(failure(
                            "librarian_output_budget_exceeded",
                            "The librarian provider response reached the configured output-token budget.",
                        ));
                    }
                    response_json.push_str(&text);
                }
                StreamEvent::TokenUsage {
                    input_tokens: reported_input,
                    output_tokens: reported_output,
                    ..
                } => {
                    if let Some(value) = reported_input {
                        input_tokens = u32::try_from(value).unwrap_or(u32::MAX);
                        input_usage_reported = true;
                    }
                    if let Some(value) = reported_output {
                        output_tokens = u32::try_from(value).unwrap_or(u32::MAX);
                        output_usage_reported = true;
                    }
                    enforce_usage_limits(
                        &facts,
                        config,
                        input_tokens,
                        output_tokens,
                        started.elapsed(),
                    )?;
                }
                _ => {}
            }
        }
        Ok(())
    })
    .await;

    match request {
        Err(_) => {
            return Err(failure_with_usage(
                "librarian_deadline_exceeded",
                "The librarian provider request exceeded the configured wall-clock deadline.",
                bounded_usage(&facts, config, input_tokens, output_tokens, deadline),
            ));
        }
        Ok(Err(mut error)) => {
            error.usage = Some(bounded_usage(
                &facts,
                config,
                input_tokens,
                output_tokens,
                started.elapsed(),
            ));
            return Err(error);
        }
        Ok(Ok(())) => {}
    }

    enforce_usage_limits(
        &facts,
        config,
        input_tokens,
        output_tokens,
        started.elapsed(),
    )?;
    if !input_usage_reported || !output_usage_reported {
        return Err(failure_with_usage(
            "librarian_usage_accounting_missing",
            "The librarian provider did not report complete input and output token usage.",
            bounded_usage(
                &facts,
                config,
                input_tokens,
                output_tokens,
                started.elapsed(),
            ),
        ));
    }
    serde_json::from_str::<serde_json::Value>(&response_json).map_err(|_| {
        failure_with_usage(
            "librarian_response_malformed",
            "The librarian provider returned malformed JSON.",
            bounded_usage(
                &facts,
                config,
                input_tokens,
                output_tokens,
                started.elapsed(),
            ),
        )
    })?;

    let cost_micros_usd = facts
        .worst_case_cost_micros(input_tokens, output_tokens)
        .unwrap_or(config.budgets.max_cost_micros);
    Ok(LibrarianGeneration {
        response_json,
        usage: BoundedUsage {
            input_tokens,
            output_tokens,
            request_count: 1,
            elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            cost_micros_usd,
        },
    })
}

fn preflight(
    facts: &GenerationRouteFacts,
    config: &ResolvedLibrarianConfig,
    admitted: &AdmittedSessionContent,
) -> Result<(), LibrarianFailure> {
    if config.budgets.max_requests != 1 {
        return Err(failure(
            "librarian_request_budget_invalid",
            "The librarian max_requests budget must be exactly 1.",
        ));
    }
    if !facts.supported {
        return Err(failure(
            "librarian_route_unsupported",
            "The configured librarian provider, model, or reasoning effort is unsupported.",
        ));
    }
    if !facts.authentication_available {
        return Err(failure(
            "librarian_authentication_missing",
            "The configured librarian OAuth credential is unavailable. Run `jcode login --provider openai`.",
        ));
    }
    if !facts.runtime_registered {
        return Err(failure(
            "librarian_runtime_unavailable",
            "The configured librarian provider runtime is not registered in this Jcode build.",
        ));
    }
    if admitted.input_tokens > config.budgets.max_input_tokens {
        return Err(failure(
            "librarian_input_budget_exceeded",
            "The admitted librarian input exceeds the configured token budget.",
        ));
    }
    let exposure = facts
        .worst_case_cost_micros(admitted.input_tokens, config.budgets.max_output_tokens)
        .ok_or_else(|| {
            failure(
                "librarian_pricing_unknown",
                "The configured librarian route has no verifiable pricing evidence.",
            )
        })?;
    if exposure > config.budgets.max_cost_micros {
        return Err(failure(
            "librarian_cost_budget_exceeded",
            "The librarian request's worst-case cost exceeds the approved budget.",
        ));
    }
    Ok(())
}

fn token_cost_micros(price_per_million: u64, tokens: u32) -> u64 {
    let numerator = price_per_million.saturating_mul(u64::from(tokens));
    numerator.saturating_add(999_999) / 1_000_000
}

fn conservative_token_count(bytes: usize) -> u32 {
    u32::try_from(bytes).unwrap_or(u32::MAX)
}

fn enforce_usage_limits(
    facts: &GenerationRouteFacts,
    config: &ResolvedLibrarianConfig,
    input_tokens: u32,
    output_tokens: u32,
    elapsed: Duration,
) -> Result<(), LibrarianFailure> {
    let cost = facts
        .worst_case_cost_micros(input_tokens, output_tokens)
        .unwrap_or(u64::MAX);
    let (code, message) = if input_tokens > config.budgets.max_input_tokens {
        (
            "librarian_input_usage_exceeded",
            "The provider reported input usage above the configured token budget.",
        )
    } else if output_tokens > config.budgets.max_output_tokens {
        (
            "librarian_output_budget_exceeded",
            "The librarian provider response exceeded the configured output-token budget.",
        )
    } else if cost > config.budgets.max_cost_micros {
        (
            "librarian_cost_usage_exceeded",
            "The provider usage exceeded the approved librarian cost budget.",
        )
    } else if elapsed > Duration::from_secs(config.budgets.deadline_seconds) {
        (
            "librarian_deadline_exceeded",
            "The librarian provider request exceeded the configured wall-clock deadline.",
        )
    } else {
        return Ok(());
    };

    Err(failure_with_usage(
        code,
        message,
        bounded_usage(facts, config, input_tokens, output_tokens, elapsed),
    ))
}

fn bounded_usage(
    facts: &GenerationRouteFacts,
    config: &ResolvedLibrarianConfig,
    input_tokens: u32,
    output_tokens: u32,
    elapsed: Duration,
) -> BoundedUsage {
    let input_tokens = input_tokens.min(config.budgets.max_input_tokens);
    let output_tokens = output_tokens.min(config.budgets.max_output_tokens);
    BoundedUsage {
        input_tokens,
        output_tokens,
        request_count: 1.min(config.budgets.max_requests),
        elapsed_ms: u64::try_from(elapsed.as_millis())
            .unwrap_or(u64::MAX)
            .min(config.budgets.deadline_seconds.saturating_mul(1_000)),
        cost_micros_usd: facts
            .worst_case_cost_micros(input_tokens, output_tokens)
            .unwrap_or(config.budgets.max_cost_micros)
            .min(config.budgets.max_cost_micros),
    }
}

fn provider_failure() -> LibrarianFailure {
    failure(
        "librarian_provider_failed",
        "The librarian provider request failed before producing a valid response.",
    )
}

fn failure_with_usage(code: &'static str, message: &str, usage: BoundedUsage) -> LibrarianFailure {
    let mut failure = failure(code, message);
    failure.usage = Some(usage);
    failure
}

fn failure(code: &'static str, message: &str) -> LibrarianFailure {
    let bounded: String = message.chars().take(512).collect();
    LibrarianFailure {
        stage: LibrarianFailureStage::Generation,
        code,
        message: bounded,
        usage: None,
    }
}
