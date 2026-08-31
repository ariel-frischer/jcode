use super::SearchResult;
use crate::config::{ResolvedWebSearchPolicy, WebSearchEngine};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::collections::VecDeque;
use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

/// Normalized outcome for one engine. `Exhausted` intentionally does not
/// exist here because it is meaningful only for the aggregate search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PerEngineOutcomeKind {
    Success,
    Empty,
    Challenge,
    Timeout,
    Transient,
    Permanent,
    Disabled,
    Unavailable,
    HealthSuppressed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SearchTerminalOutcome {
    Success,
    NoEligibleEngine,
    Exhausted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct EngineAttempt {
    pub engine: WebSearchEngine,
    pub attempts: u8,
    pub retry_count: u8,
    pub classification: PerEngineOutcomeKind,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SearchExecution {
    pub results: Option<Vec<SearchResult>>,
    pub considered: Vec<EngineAttempt>,
    pub selected_engine: Option<WebSearchEngine>,
    pub physical_attempt_count: u8,
    pub retry_count: u8,
    pub skip_count: u8,
    pub elapsed_ms: u64,
    pub terminal: SearchTerminalOutcome,
}

impl SearchExecution {
    #[cfg(test)]
    pub(crate) fn considered_engines(&self) -> Vec<WebSearchEngine> {
        self.considered.iter().map(|entry| entry.engine).collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct DiagnosticPolicySummary {
    pub enabled: bool,
    pub duckduckgo_enabled: bool,
    pub bing_enabled: bool,
    pub searxng_enabled: bool,
    pub fallback_order: Vec<String>,
    pub fallback_enabled: bool,
    pub attempt_timeout_ms: u64,
    pub retries_enabled: bool,
    pub max_retries: u8,
    pub health_suppression_enabled: bool,
    pub health_failure_threshold: u8,
    pub health_cooldown_ms: u64,
    pub diagnostics_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SearchDiagnosticSummary {
    pub schema_version: String,
    pub effective_policy: DiagnosticPolicySummary,
    pub considered: Vec<EngineAttempt>,
    pub physical_attempt_count: u8,
    pub retry_count: u8,
    pub skip_count: u8,
    pub elapsed_ms: u64,
    pub selected_engine: Option<String>,
    pub per_engine_outcomes: BTreeMap<String, PerEngineOutcomeKind>,
    pub search_terminal: SearchTerminalOutcome,
}

impl SearchDiagnosticSummary {
    pub(crate) fn from_execution(
        policy: &ResolvedWebSearchPolicy,
        execution: &SearchExecution,
    ) -> Self {
        let per_engine_outcomes = execution
            .considered
            .iter()
            .map(|entry| (entry.engine.as_str().to_string(), entry.classification))
            .collect();
        Self {
            schema_version: "jcode.websearch.diagnostics.v1".to_string(),
            effective_policy: DiagnosticPolicySummary {
                enabled: policy.enabled,
                duckduckgo_enabled: policy.duckduckgo_enabled,
                bing_enabled: policy.bing_enabled,
                searxng_enabled: policy.searxng_enabled,
                fallback_order: policy
                    .fallback_order
                    .iter()
                    .map(|engine| engine.as_str().to_string())
                    .collect(),
                fallback_enabled: policy.fallback_enabled,
                attempt_timeout_ms: policy.attempt_timeout_ms,
                retries_enabled: policy.retries_enabled,
                max_retries: policy.max_retries,
                health_suppression_enabled: policy.health_suppression_enabled,
                health_failure_threshold: policy.health_failure_threshold,
                health_cooldown_ms: policy.health_cooldown_ms,
                diagnostics_enabled: policy.diagnostics_enabled,
            },
            considered: execution.considered.clone(),
            physical_attempt_count: execution.physical_attempt_count,
            retry_count: execution.retry_count,
            skip_count: execution.skip_count,
            elapsed_ms: execution.elapsed_ms,
            selected_engine: execution
                .selected_engine
                .map(|engine| engine.as_str().to_string()),
            per_engine_outcomes,
            search_terminal: execution.terminal,
        }
    }

    pub(crate) fn metadata(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_else(|_| {
            serde_json::json!({
                "schema_version": "jcode.websearch.diagnostics.v1",
                "search_terminal": "exhausted"
            })
        })
    }
}

pub(crate) fn is_clean_first_success(execution: &SearchExecution) -> bool {
    execution.terminal == SearchTerminalOutcome::Success
        && execution.considered.len() == 1
        && execution.physical_attempt_count == 1
        && execution.retry_count == 0
}

pub(crate) fn presentation_title(execution: &SearchExecution) -> String {
    let title = match execution.terminal {
        SearchTerminalOutcome::Success => {
            let engine = execution
                .selected_engine
                .map(WebSearchEngine::as_str)
                .unwrap_or("unknown");
            if is_clean_first_success(execution) {
                engine.to_string()
            } else {
                format!(
                    "{engine} (attempts {}, retries {})",
                    execution.physical_attempt_count, execution.retry_count
                )
            }
        }
        SearchTerminalOutcome::NoEligibleEngine => format!(
            "websearch: no eligible engine (skipped {})",
            execution.skip_count
        ),
        SearchTerminalOutcome::Exhausted => format!(
            "websearch: exhausted (attempts {}, retries {}, skipped {})",
            execution.physical_attempt_count, execution.retry_count, execution.skip_count
        ),
    };
    truncate_display(&title, 96)
}

pub(crate) fn presentation_summary(execution: &SearchExecution) -> Option<String> {
    if is_clean_first_success(execution) {
        return None;
    }
    let text = match execution.terminal {
        SearchTerminalOutcome::Success => format!(
            "Search fallback selected {} after {} attempt(s), {} retry/retries.",
            execution
                .selected_engine
                .map(WebSearchEngine::as_str)
                .unwrap_or("no engine"),
            execution.physical_attempt_count,
            execution.retry_count
        ),
        SearchTerminalOutcome::NoEligibleEngine => format!(
            "No eligible search engine was contacted ({} skipped).",
            execution.skip_count
        ),
        SearchTerminalOutcome::Exhausted => format!(
            "Websearch exhausted {} attempt(s) across eligible engines.",
            execution.physical_attempt_count
        ),
    };
    Some(truncate_display(&text, 96))
}

fn truncate_display(text: &str, max_width: usize) -> String {
    if text.chars().count() <= max_width {
        return text.to_string();
    }
    if max_width <= 1 {
        return "…".to_string();
    }
    let mut result = String::new();
    for character in text.chars() {
        if result.chars().count() + 1 >= max_width {
            break;
        }
        result.push(character);
    }
    result.push('…');
    result
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct EngineHealthState {
    pub consecutive_failures: u8,
    pub suppressed_until: Option<Instant>,
}

pub(crate) type EngineHealthMap = HashMap<WebSearchEngine, EngineHealthState>;

impl EngineHealthState {
    pub(crate) fn is_suppressed(&mut self, now: Instant) -> bool {
        match self.suppressed_until {
            Some(until) if now < until => true,
            Some(_) => {
                self.suppressed_until = None;
                self.consecutive_failures = 0;
                false
            }
            None => false,
        }
    }

    pub(crate) fn record_terminal_failure(
        &mut self,
        classification: PerEngineOutcomeKind,
        now: Instant,
        policy: &ResolvedWebSearchPolicy,
    ) {
        if !policy.health_suppression_enabled
            || !matches!(
                classification,
                PerEngineOutcomeKind::Challenge
                    | PerEngineOutcomeKind::Transient
                    | PerEngineOutcomeKind::Timeout
            )
        {
            return;
        }
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        if self.consecutive_failures >= policy.health_failure_threshold {
            self.suppressed_until = Some(now + Duration::from_millis(policy.health_cooldown_ms));
            self.consecutive_failures = 0;
        }
    }

    pub(crate) fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.suppressed_until = None;
    }
}

/// Adapter-independent result returned by the existing engine adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BackendOutcome {
    Results(Vec<SearchResult>),
    Empty,
    Challenge,
    Timeout,
    Transient,
    Permanent,
}

#[async_trait]
pub(crate) trait SearchBackend {
    async fn search(&mut self, engine: WebSearchEngine, attempt: u8) -> BackendOutcome;
}

/// Small deterministic backend seam for orchestration tests and future local
/// fixture checks. It does not construct an HTTP client or perform I/O.
#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct ScriptedBackend {
    outcomes: VecDeque<BackendOutcome>,
    calls: Vec<(WebSearchEngine, u8)>,
}

#[cfg(test)]
impl ScriptedBackend {
    pub(crate) fn new(outcomes: impl IntoIterator<Item = BackendOutcome>) -> Self {
        Self {
            outcomes: outcomes.into_iter().collect(),
            calls: Vec::new(),
        }
    }

    pub(crate) fn calls(&self) -> &[(WebSearchEngine, u8)] {
        &self.calls
    }
}

#[cfg(test)]
#[async_trait]
impl SearchBackend for ScriptedBackend {
    async fn search(&mut self, engine: WebSearchEngine, attempt: u8) -> BackendOutcome {
        self.calls.push((engine, attempt));
        self.outcomes
            .pop_front()
            .unwrap_or(BackendOutcome::Permanent)
    }
}

/// Run the bounded resilient decision loop using an adapter-independent
/// backend. Every engine is considered once, while physical attempts are
/// bounded by `E * (1 + max_retries)` and only transient/timeout outcomes
/// retry.
pub(crate) async fn run_search<B: SearchBackend>(
    policy: &ResolvedWebSearchPolicy,
    preferred: WebSearchEngine,
    health: &mut EngineHealthMap,
    now: Instant,
    backend: &mut B,
) -> anyhow::Result<SearchExecution> {
    let search_started = Instant::now();
    let considered_order = merged_engine_order(policy, preferred);
    let mut considered = Vec::with_capacity(considered_order.len());
    let mut physical_attempt_count = 0_u8;
    let mut retry_count = 0_u8;
    let mut skip_count = 0_u8;
    let mut selected_engine = None;
    let mut selected_results = None;

    for engine in considered_order {
        let engine_started = Instant::now();
        let engine_now = now + search_started.elapsed();
        let enabled = engine_enabled(policy, engine);
        if !enabled {
            considered.push(EngineAttempt {
                engine,
                attempts: 0,
                retry_count: 0,
                classification: PerEngineOutcomeKind::Disabled,
                elapsed_ms: 0,
            });
            skip_count = skip_count.saturating_add(1);
            continue;
        }
        if !engine_available(policy, engine) {
            considered.push(EngineAttempt {
                engine,
                attempts: 0,
                retry_count: 0,
                classification: PerEngineOutcomeKind::Unavailable,
                elapsed_ms: 0,
            });
            skip_count = skip_count.saturating_add(1);
            continue;
        }
        let state = health.entry(engine).or_default();
        if state.is_suppressed(engine_now) {
            considered.push(EngineAttempt {
                engine,
                attempts: 0,
                retry_count: 0,
                classification: PerEngineOutcomeKind::HealthSuppressed,
                elapsed_ms: 0,
            });
            skip_count = skip_count.saturating_add(1);
            continue;
        }

        let max_attempts = if policy.retries_enabled {
            policy.max_retries.saturating_add(1)
        } else {
            1
        };
        let mut attempts = 0_u8;
        let mut engine_retry_count = 0_u8;
        let mut terminal = PerEngineOutcomeKind::Permanent;
        let mut filtered_usable_results = None;

        while attempts < max_attempts {
            attempts += 1;
            physical_attempt_count = physical_attempt_count.saturating_add(1);
            let outcome = tokio::time::timeout(
                Duration::from_millis(policy.attempt_timeout_ms),
                backend.search(engine, attempts),
            )
            .await
            .unwrap_or(BackendOutcome::Timeout);
            match outcome {
                BackendOutcome::Results(results) => {
                    let results = retain_usable_results(results);
                    if results.is_empty() {
                        terminal = PerEngineOutcomeKind::Empty;
                    } else {
                        terminal = PerEngineOutcomeKind::Success;
                        filtered_usable_results = Some(results);
                    }
                    break;
                }
                BackendOutcome::Empty => {
                    terminal = PerEngineOutcomeKind::Empty;
                    break;
                }
                BackendOutcome::Challenge => {
                    terminal = PerEngineOutcomeKind::Challenge;
                    break;
                }
                BackendOutcome::Permanent => {
                    terminal = PerEngineOutcomeKind::Permanent;
                    break;
                }
                transient @ (BackendOutcome::Timeout | BackendOutcome::Transient) => {
                    terminal = match transient {
                        BackendOutcome::Timeout => PerEngineOutcomeKind::Timeout,
                        BackendOutcome::Transient => PerEngineOutcomeKind::Transient,
                        _ => unreachable!(),
                    };
                    if attempts < max_attempts {
                        engine_retry_count = engine_retry_count.saturating_add(1);
                        retry_count = retry_count.saturating_add(1);
                        tokio::time::sleep(Duration::from_millis(
                            jcode_config_types::WEBSEARCH_RETRY_DELAY_MS,
                        ))
                        .await;
                    } else {
                        break;
                    }
                }
            }
        }

        let state = health.entry(engine).or_default();
        if terminal == PerEngineOutcomeKind::Success {
            state.record_success();
            selected_engine = Some(engine);
            selected_results = filtered_usable_results;
        } else if matches!(
            terminal,
            PerEngineOutcomeKind::Challenge
                | PerEngineOutcomeKind::Transient
                | PerEngineOutcomeKind::Timeout
        ) {
            state.record_terminal_failure(terminal, now + search_started.elapsed(), policy);
        }
        considered.push(EngineAttempt {
            engine,
            attempts,
            retry_count: engine_retry_count,
            classification: terminal,
            elapsed_ms: engine_started.elapsed().as_millis() as u64,
        });
        if selected_engine.is_some() {
            break;
        }
    }

    let terminal = if selected_engine.is_some() {
        SearchTerminalOutcome::Success
    } else if physical_attempt_count == 0 {
        SearchTerminalOutcome::NoEligibleEngine
    } else {
        SearchTerminalOutcome::Exhausted
    };
    Ok(SearchExecution {
        results: selected_results,
        considered,
        selected_engine,
        physical_attempt_count,
        retry_count,
        skip_count,
        elapsed_ms: search_started.elapsed().as_millis() as u64,
        terminal,
    })
}

/// Run a search without holding shared health state across network I/O or retry
/// delays. A snapshot drives selection, then terminal health transitions are
/// merged back under a short-lived lock.
pub(crate) async fn run_search_with_shared_health<B: SearchBackend>(
    policy: &ResolvedWebSearchPolicy,
    preferred: WebSearchEngine,
    health: &tokio::sync::Mutex<EngineHealthMap>,
    now: Instant,
    backend: &mut B,
) -> anyhow::Result<SearchExecution> {
    let mut snapshot = health.lock().await.clone();
    let execution = run_search(policy, preferred, &mut snapshot, now, backend).await?;
    let completed_at = now + Duration::from_millis(execution.elapsed_ms);

    let mut shared = health.lock().await;
    for attempt in &execution.considered {
        let state = shared.entry(attempt.engine).or_default();
        match attempt.classification {
            PerEngineOutcomeKind::Success => state.record_success(),
            PerEngineOutcomeKind::Challenge
            | PerEngineOutcomeKind::Transient
            | PerEngineOutcomeKind::Timeout => {
                state.record_terminal_failure(attempt.classification, completed_at, policy);
            }
            PerEngineOutcomeKind::Empty
            | PerEngineOutcomeKind::Permanent
            | PerEngineOutcomeKind::Disabled
            | PerEngineOutcomeKind::Unavailable
            | PerEngineOutcomeKind::HealthSuppressed => {}
        }
    }

    Ok(execution)
}

pub(crate) fn merged_engine_order(
    policy: &ResolvedWebSearchPolicy,
    preferred: WebSearchEngine,
) -> Vec<WebSearchEngine> {
    let mut order = Vec::with_capacity(1 + policy.fallback_order.len());
    order.push(preferred);
    if policy.fallback_enabled {
        order.extend(policy.fallback_order.iter().copied());
    }
    let mut deduplicated = Vec::with_capacity(order.len());
    for engine in order {
        if !deduplicated.contains(&engine) {
            deduplicated.push(engine);
        }
        if deduplicated.len() == jcode_config_types::WEBSEARCH_MAX_ENGINES {
            break;
        }
    }
    deduplicated
}

fn engine_enabled(policy: &ResolvedWebSearchPolicy, engine: WebSearchEngine) -> bool {
    match engine {
        WebSearchEngine::Duckduckgo => policy.duckduckgo_enabled,
        WebSearchEngine::Bing => policy.bing_enabled,
        WebSearchEngine::Searxng => policy.searxng_enabled,
    }
}

fn engine_available(policy: &ResolvedWebSearchPolicy, engine: WebSearchEngine) -> bool {
    match engine {
        WebSearchEngine::Searxng => policy.trusted_searxng_url.is_some(),
        WebSearchEngine::Duckduckgo | WebSearchEngine::Bing => true,
    }
}

fn retain_usable_results(results: Vec<SearchResult>) -> Vec<SearchResult> {
    results
        .into_iter()
        .filter(|result| {
            if result.title.trim().is_empty() {
                return false;
            }
            let Ok(url) = url::Url::parse(result.url.trim()) else {
                return false;
            };
            matches!(url.scheme(), "http" | "https")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn policy() -> ResolvedWebSearchPolicy {
        ResolvedWebSearchPolicy {
            enabled: true,
            duckduckgo_enabled: true,
            bing_enabled: true,
            searxng_enabled: true,
            fallback_order: vec![
                WebSearchEngine::Bing,
                WebSearchEngine::Duckduckgo,
                WebSearchEngine::Searxng,
            ],
            fallback_enabled: true,
            attempt_timeout_ms: 100,
            retries_enabled: true,
            max_retries: 1,
            health_suppression_enabled: true,
            health_failure_threshold: 2,
            health_cooldown_ms: 1_000,
            diagnostics_enabled: true,
            trusted_searxng_url: Some("http://127.0.0.1:8080".to_string()),
        }
    }

    fn result(title: &str, url: &str) -> SearchResult {
        SearchResult {
            title: title.to_string(),
            url: url.to_string(),
            snippet: "snippet".to_string(),
        }
    }

    #[tokio::test]
    async fn resolves_preferred_order_deduplicates_and_stops_on_first_usable_result() {
        let now = Instant::now();
        let mut health = EngineHealthMap::default();
        let mut backend = ScriptedBackend::new([
            BackendOutcome::Challenge,
            BackendOutcome::Results(vec![result("Bing", "https://bing.example/result")]),
            BackendOutcome::Results(vec![result("late", "https://late.example/result")]),
        ]);

        let execution = run_search(
            &policy(),
            WebSearchEngine::Duckduckgo,
            &mut health,
            now,
            &mut backend,
        )
        .await
        .unwrap();

        assert_eq!(
            execution.considered_engines(),
            &[WebSearchEngine::Duckduckgo, WebSearchEngine::Bing]
        );
        assert_eq!(execution.selected_engine, Some(WebSearchEngine::Bing));
        assert_eq!(execution.physical_attempt_count, 2);
        assert_eq!(
            backend.calls(),
            &[(WebSearchEngine::Duckduckgo, 1), (WebSearchEngine::Bing, 1),]
        );
        assert_eq!(execution.terminal, SearchTerminalOutcome::Success);
    }

    #[tokio::test]
    async fn disabled_unavailable_and_suppressed_engines_are_skipped_without_calls() {
        let now = Instant::now();
        let mut configured = policy();
        configured.duckduckgo_enabled = false;
        configured.trusted_searxng_url = None;
        let mut health = EngineHealthMap::default();
        health.insert(
            WebSearchEngine::Bing,
            EngineHealthState {
                consecutive_failures: 2,
                suppressed_until: Some(now + Duration::from_secs(1)),
            },
        );
        let mut backend = ScriptedBackend::new([BackendOutcome::Empty]);

        let execution = run_search(
            &configured,
            WebSearchEngine::Duckduckgo,
            &mut health,
            now,
            &mut backend,
        )
        .await
        .unwrap();

        assert_eq!(backend.calls(), &[]);
        assert_eq!(execution.terminal, SearchTerminalOutcome::NoEligibleEngine);
        assert_eq!(execution.skip_count, 3);
        assert_eq!(
            execution
                .considered
                .iter()
                .map(|entry| entry.classification)
                .collect::<Vec<_>>(),
            vec![
                PerEngineOutcomeKind::Disabled,
                PerEngineOutcomeKind::HealthSuppressed,
                PerEngineOutcomeKind::Unavailable,
            ]
        );
    }

    #[tokio::test]
    async fn fallback_disabled_considers_only_the_preferred_engine() {
        let now = Instant::now();
        let mut configured = policy();
        configured.fallback_enabled = false;
        let mut health = EngineHealthMap::default();
        let mut backend = ScriptedBackend::new([BackendOutcome::Empty]);

        let execution = run_search(
            &configured,
            WebSearchEngine::Searxng,
            &mut health,
            now,
            &mut backend,
        )
        .await
        .unwrap();

        assert_eq!(execution.considered_engines(), &[WebSearchEngine::Searxng]);
        assert_eq!(execution.terminal, SearchTerminalOutcome::Exhausted);
        assert_eq!(backend.calls(), &[(WebSearchEngine::Searxng, 1)]);
    }

    #[tokio::test]
    async fn transient_retries_once_then_increments_health_once_and_recovers_on_success() {
        let now = Instant::now();
        let mut configured = policy();
        configured.fallback_order = vec![WebSearchEngine::Duckduckgo];
        let mut health = EngineHealthMap::default();
        let mut backend =
            ScriptedBackend::new([BackendOutcome::Transient, BackendOutcome::Transient]);

        let first = run_search(
            &configured,
            WebSearchEngine::Duckduckgo,
            &mut health,
            now,
            &mut backend,
        )
        .await
        .unwrap();
        assert_eq!(first.retry_count, 1);
        assert_eq!(health[&WebSearchEngine::Duckduckgo].consecutive_failures, 1);
        assert_eq!(backend.calls().len(), 2);

        let mut successful_backend = ScriptedBackend::new([BackendOutcome::Results(vec![result(
            "ok",
            "https://ok.test",
        )])]);
        let second = run_search(
            &configured,
            WebSearchEngine::Duckduckgo,
            &mut health,
            now + Duration::from_millis(1),
            &mut successful_backend,
        )
        .await
        .unwrap();
        assert_eq!(second.selected_engine, Some(WebSearchEngine::Duckduckgo));
        assert_eq!(health[&WebSearchEngine::Duckduckgo].consecutive_failures, 0);
        assert_eq!(health[&WebSearchEngine::Duckduckgo].suppressed_until, None);
    }

    #[tokio::test]
    async fn only_transient_and_timeout_outcomes_retry_within_configured_bounds() {
        let now = Instant::now();
        let mut configured = policy();
        configured.fallback_order = vec![WebSearchEngine::Duckduckgo];
        configured.max_retries = 2;
        let mut health = EngineHealthMap::default();
        let mut backend = ScriptedBackend::new([
            BackendOutcome::Timeout,
            BackendOutcome::Transient,
            BackendOutcome::Permanent,
            BackendOutcome::Results(vec![result("never", "https://never.test")]),
        ]);

        let execution = run_search(
            &configured,
            WebSearchEngine::Duckduckgo,
            &mut health,
            now,
            &mut backend,
        )
        .await
        .unwrap();

        assert_eq!(execution.physical_attempt_count, 3);
        assert_eq!(execution.retry_count, 2);
        assert_eq!(
            execution.considered[0].classification,
            PerEngineOutcomeKind::Permanent
        );
        assert_eq!(backend.calls().len(), 3);
    }

    #[tokio::test]
    async fn retries_can_be_disabled_and_exhaustion_retains_each_terminal_reason() {
        let now = Instant::now();
        let mut configured = policy();
        configured.retries_enabled = false;
        configured.fallback_order = vec![WebSearchEngine::Bing, WebSearchEngine::Searxng];
        let mut health = EngineHealthMap::default();
        let mut backend = ScriptedBackend::new([
            BackendOutcome::Transient,
            BackendOutcome::Challenge,
            BackendOutcome::Empty,
        ]);

        let execution = run_search(
            &configured,
            WebSearchEngine::Duckduckgo,
            &mut health,
            now,
            &mut backend,
        )
        .await
        .unwrap();

        assert_eq!(execution.terminal, SearchTerminalOutcome::Exhausted);
        assert_eq!(execution.physical_attempt_count, 3);
        assert_eq!(execution.retry_count, 0);
        assert_eq!(
            execution
                .considered
                .iter()
                .map(|entry| entry.classification)
                .collect::<Vec<_>>(),
            vec![
                PerEngineOutcomeKind::Transient,
                PerEngineOutcomeKind::Challenge,
                PerEngineOutcomeKind::Empty,
            ]
        );
    }

    #[tokio::test]
    async fn partial_results_are_unusable_and_do_not_prevent_a_later_fallback() {
        let now = Instant::now();
        let mut configured = policy();
        configured.fallback_order = vec![WebSearchEngine::Bing];
        let mut health = EngineHealthMap::default();
        let mut backend = ScriptedBackend::new([
            BackendOutcome::Results(vec![result("", "https://partial.test")]),
            BackendOutcome::Results(vec![result("usable", "https://usable.test")]),
        ]);

        let execution = run_search(
            &configured,
            WebSearchEngine::Duckduckgo,
            &mut health,
            now,
            &mut backend,
        )
        .await
        .unwrap();

        assert_eq!(execution.selected_engine, Some(WebSearchEngine::Bing));
        assert_eq!(
            execution.considered[0].classification,
            PerEngineOutcomeKind::Empty
        );
        assert_eq!(backend.calls().len(), 2);
    }

    #[tokio::test]
    async fn usable_result_sets_drop_invalid_entries_before_rendering() {
        let now = Instant::now();
        let mut configured = policy();
        configured.fallback_order = vec![WebSearchEngine::Duckduckgo];
        let mut health = EngineHealthMap::default();
        let mut backend = ScriptedBackend::new([BackendOutcome::Results(vec![
            result("usable", "https://usable.test"),
            result("", "https://missing-title.test"),
            result("unsupported", "ftp://unsupported.test"),
        ])]);

        let execution = run_search(
            &configured,
            WebSearchEngine::Duckduckgo,
            &mut health,
            now,
            &mut backend,
        )
        .await
        .unwrap();

        assert_eq!(execution.terminal, SearchTerminalOutcome::Success);
        assert_eq!(
            execution.results,
            Some(vec![result("usable", "https://usable.test")])
        );
    }

    struct DelayedSequenceBackend {
        outcomes: VecDeque<(Duration, BackendOutcome)>,
        calls: Vec<WebSearchEngine>,
    }

    #[async_trait]
    impl SearchBackend for DelayedSequenceBackend {
        async fn search(&mut self, engine: WebSearchEngine, _attempt: u8) -> BackendOutcome {
            self.calls.push(engine);
            let (delay, outcome) = self.outcomes.pop_front().unwrap();
            tokio::time::sleep(delay).await;
            outcome
        }
    }

    #[tokio::test]
    async fn suppression_expiry_is_checked_when_each_engine_is_reached() {
        let now = Instant::now();
        let mut configured = policy();
        configured.retries_enabled = false;
        configured.fallback_order = vec![WebSearchEngine::Bing];
        let mut health = EngineHealthMap::from([(
            WebSearchEngine::Bing,
            EngineHealthState {
                consecutive_failures: 0,
                suppressed_until: Some(now + Duration::from_millis(5)),
            },
        )]);
        let mut backend = DelayedSequenceBackend {
            outcomes: VecDeque::from([
                (Duration::from_millis(15), BackendOutcome::Empty),
                (
                    Duration::ZERO,
                    BackendOutcome::Results(vec![result("usable", "https://usable.test")]),
                ),
            ]),
            calls: Vec::new(),
        };

        let execution = run_search(
            &configured,
            WebSearchEngine::Duckduckgo,
            &mut health,
            now,
            &mut backend,
        )
        .await
        .unwrap();

        assert_eq!(execution.selected_engine, Some(WebSearchEngine::Bing));
        assert_eq!(
            backend.calls,
            vec![WebSearchEngine::Duckduckgo, WebSearchEngine::Bing]
        );
    }

    #[tokio::test]
    async fn shared_health_cooldown_starts_after_the_failed_attempt() {
        let now = Instant::now();
        let mut configured = policy();
        configured.retries_enabled = false;
        configured.health_failure_threshold = 1;
        configured.health_cooldown_ms = 1_000;
        configured.fallback_order = vec![WebSearchEngine::Duckduckgo];
        let health = tokio::sync::Mutex::new(EngineHealthMap::default());
        let mut backend = DelayedSequenceBackend {
            outcomes: VecDeque::from([(Duration::from_millis(15), BackendOutcome::Transient)]),
            calls: Vec::new(),
        };

        run_search_with_shared_health(
            &configured,
            WebSearchEngine::Duckduckgo,
            &health,
            now,
            &mut backend,
        )
        .await
        .unwrap();

        let suppressed_until = health.lock().await[&WebSearchEngine::Duckduckgo]
            .suppressed_until
            .unwrap();
        assert!(suppressed_until >= now + Duration::from_millis(1_010));
    }

    #[test]
    fn health_threshold_cooldown_is_inclusive_and_engine_local() {
        let start = Instant::now();
        let mut state = EngineHealthState::default();
        let configured = policy();

        state.record_terminal_failure(PerEngineOutcomeKind::Transient, start, &configured);
        assert!(!state.is_suppressed(start));
        state.record_terminal_failure(
            PerEngineOutcomeKind::Transient,
            start + Duration::from_millis(1),
            &configured,
        );
        let suppressed_until = state.suppressed_until.unwrap();
        assert!(state.is_suppressed(suppressed_until - Duration::from_nanos(1)));
        assert!(!state.is_suppressed(suppressed_until));
        assert_eq!(state.consecutive_failures, 0);
    }

    #[test]
    fn repeated_challenges_open_health_suppression() {
        let start = Instant::now();
        let mut state = EngineHealthState::default();
        let configured = policy();

        state.record_terminal_failure(PerEngineOutcomeKind::Challenge, start, &configured);
        assert!(!state.is_suppressed(start));
        state.record_terminal_failure(
            PerEngineOutcomeKind::Challenge,
            start + Duration::from_millis(1),
            &configured,
        );
        assert!(state.is_suppressed(start + Duration::from_millis(2)));
    }

    struct DelayedBackend;

    #[async_trait]
    impl SearchBackend for DelayedBackend {
        async fn search(&mut self, _engine: WebSearchEngine, _attempt: u8) -> BackendOutcome {
            tokio::time::sleep(Duration::from_millis(10)).await;
            BackendOutcome::Results(vec![result("delayed", "https://delayed.test")])
        }
    }

    #[tokio::test]
    async fn execution_records_real_elapsed_time() {
        let mut health = EngineHealthMap::default();
        let mut backend = DelayedBackend;
        let execution = run_search(
            &policy(),
            WebSearchEngine::Duckduckgo,
            &mut health,
            Instant::now(),
            &mut backend,
        )
        .await
        .unwrap();

        assert!(execution.elapsed_ms >= 5, "execution={execution:?}");
        assert!(
            execution.considered[0].elapsed_ms >= 5,
            "execution={execution:?}"
        );
    }

    struct BlockingBackend {
        started: Option<tokio::sync::oneshot::Sender<()>>,
        release: Option<tokio::sync::oneshot::Receiver<()>>,
    }

    #[async_trait]
    impl SearchBackend for BlockingBackend {
        async fn search(&mut self, _engine: WebSearchEngine, _attempt: u8) -> BackendOutcome {
            if let Some(started) = self.started.take() {
                let _ = started.send(());
            }
            if let Some(release) = self.release.take() {
                let _ = release.await;
            }
            BackendOutcome::Results(vec![result("released", "https://released.test")])
        }
    }

    #[tokio::test]
    async fn shared_health_lock_is_not_held_during_network_wait() {
        let health = std::sync::Arc::new(tokio::sync::Mutex::new(EngineHealthMap::default()));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let task_health = health.clone();
        let task = tokio::spawn(async move {
            let mut backend = BlockingBackend {
                started: Some(started_tx),
                release: Some(release_rx),
            };
            run_search_with_shared_health(
                &policy(),
                WebSearchEngine::Duckduckgo,
                task_health.as_ref(),
                Instant::now(),
                &mut backend,
            )
            .await
        });

        started_rx.await.unwrap();
        let guard = tokio::time::timeout(Duration::from_millis(50), health.lock())
            .await
            .expect("health lock must remain available while an engine waits");
        drop(guard);
        release_tx.send(()).unwrap();
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn scripted_decision_is_stable_across_one_hundred_runs() {
        let now = Instant::now();
        let configured = policy();
        let mut expected = None;
        for _ in 0..100 {
            let mut health = EngineHealthMap::default();
            let mut backend = ScriptedBackend::new([
                BackendOutcome::Empty,
                BackendOutcome::Results(vec![result("stable", "https://stable.test")]),
            ]);
            let execution = run_search(
                &configured,
                WebSearchEngine::Duckduckgo,
                &mut health,
                now,
                &mut backend,
            )
            .await
            .unwrap();
            let decision = (
                execution.considered.clone(),
                execution.physical_attempt_count,
                execution.retry_count,
                execution.terminal,
                backend.calls().to_vec(),
            );
            if let Some(expected) = &expected {
                assert_eq!(&decision, expected);
            } else {
                expected = Some(decision);
            }
        }
    }

    #[test]
    fn diagnostic_metadata_is_bounded_and_excludes_sensitive_inputs() {
        let configured = policy();
        let execution = SearchExecution {
            results: None,
            considered: vec![EngineAttempt {
                engine: WebSearchEngine::Searxng,
                attempts: 0,
                retry_count: 0,
                classification: PerEngineOutcomeKind::Unavailable,
                elapsed_ms: 4,
            }],
            selected_engine: None,
            physical_attempt_count: 0,
            retry_count: 0,
            skip_count: 1,
            elapsed_ms: 4,
            terminal: SearchTerminalOutcome::NoEligibleEngine,
        };
        let summary = SearchDiagnosticSummary::from_execution(&configured, &execution);
        let encoded = summary.metadata().to_string();
        assert!(encoded.contains("jcode.websearch.diagnostics.v1"));
        assert!(encoded.contains("searxng"));
        for secret in [
            "fixture query",
            "bing-secret-key",
            "response body",
            "https://private.example.test",
        ] {
            assert!(!encoded.contains(secret));
        }
    }

    #[test]
    fn diagnostic_trigger_presentation_is_one_line_and_silent_for_clean_success() {
        let clean = SearchExecution {
            results: Some(vec![result("ok", "https://ok.test")]),
            considered: vec![EngineAttempt {
                engine: WebSearchEngine::Bing,
                attempts: 1,
                retry_count: 0,
                classification: PerEngineOutcomeKind::Success,
                elapsed_ms: 1,
            }],
            selected_engine: Some(WebSearchEngine::Bing),
            physical_attempt_count: 1,
            retry_count: 0,
            skip_count: 0,
            elapsed_ms: 1,
            terminal: SearchTerminalOutcome::Success,
        };
        assert_eq!(presentation_title(&clean), "bing");
        assert_eq!(presentation_summary(&clean), None);

        let noisy = SearchExecution {
            physical_attempt_count: 6,
            retry_count: 3,
            skip_count: 1,
            terminal: SearchTerminalOutcome::Exhausted,
            ..clean
        };
        let title = presentation_title(&noisy);
        let line = presentation_summary(&noisy).unwrap();
        assert!(title.chars().count() <= 96);
        assert!(line.chars().count() <= 96);
        assert!(!line.contains("fixture query"));
    }

    #[tokio::test]
    #[ignore = "benchmark-style acceptance test; run explicitly for performance evidence"]
    async fn orchestration_bookkeeping_stays_within_latency_budget() {
        let configured = policy();
        let start = Instant::now();
        for _ in 0..100 {
            let health = tokio::sync::Mutex::new(EngineHealthMap::default());
            let mut backend = ScriptedBackend::new([BackendOutcome::Results(vec![result(
                "warmup",
                "https://warmup.test",
            )])]);
            let _ = run_search_with_shared_health(
                &configured,
                WebSearchEngine::Duckduckgo,
                &health,
                start,
                &mut backend,
            )
            .await
            .unwrap();
        }

        let mut samples = Vec::with_capacity(1_000);
        for _ in 0..1_000 {
            let health = tokio::sync::Mutex::new(EngineHealthMap::default());
            let mut backend = ScriptedBackend::new([BackendOutcome::Results(vec![result(
                "sample",
                "https://sample.test",
            )])]);
            let started = Instant::now();
            let _ = run_search_with_shared_health(
                &configured,
                WebSearchEngine::Duckduckgo,
                &health,
                started,
                &mut backend,
            )
            .await
            .unwrap();
            samples.push(started.elapsed());
        }
        samples.sort_unstable();
        let median = samples[samples.len() / 2];
        let p95 = samples[samples.len() * 95 / 100];
        eprintln!(
            "websearch orchestration benchmark: samples={} median_us={} p95_us={}",
            samples.len(),
            median.as_micros(),
            p95.as_micros()
        );
        assert!(median < Duration::from_millis(1), "median={median:?}");
        assert!(p95 < Duration::from_millis(2), "p95={p95:?}");
    }
}
