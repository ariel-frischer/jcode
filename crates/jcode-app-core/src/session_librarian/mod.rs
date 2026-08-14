//! Manually invoked session librarian orchestration.
//!
//! This module owns the complete one-shot workflow. Callers provide either the
//! canonical current [`Session`] or an explicit persisted session identifier and
//! receive exactly one terminal [`LibrarianResult`]. The implementation remains
//! responsible for resolving persisted sessions read-only, admitting and
//! fingerprinting content, using an independent provider route, validating and
//! rendering the response, and atomically publishing the artifact pair.

use async_trait::async_trait;
use jcode_base::{
    config::{
        Config, LibrarianConfigError, LibrarianInvocationOverrides, LibrarianRouteIdentity,
        LibrarianRouteValidation, ResolvedLibrarianConfig, resolve_librarian_config,
    },
    session::Session,
};
use jcode_session_types::{
    BoundedUsage, LibrarianBudgetIdentity, LibrarianConfigurationIdentity, LibrarianRelevantFiles,
    RouteIdentity, SessionSummary, SourceFingerprint, StructuredSummarySections,
};
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

#[allow(dead_code)]
mod admission;
#[allow(dead_code)]
mod fingerprint;
#[allow(dead_code)]
mod generation;
#[allow(dead_code)]
mod handoff;
#[allow(dead_code)]
mod publication;

const SUMMARY_FORMAT_VERSION: &str = "session-summary.v1";
const FILTER_VERSION: &str = "session-librarian-filter.v1";
const PROMPT_VERSION: &str = "session-librarian-prompt.v1";
const RECEIPT_VERSION: &str = "session-librarian-receipt.v1";

/// Canonical source selected for one explicit librarian invocation.
///
/// `Current` borrows the server-owned session directly. `Persisted` is only an
/// identifier: the workflow must resolve it through the existing persistence
/// path without switching the active session or creating another transcript.
#[derive(Debug)]
pub enum LibrarianSessionTarget<'a> {
    Current(&'a Session),
    Persisted { session_id: &'a str },
}

impl LibrarianSessionTarget<'_> {
    pub fn requested_session_id(&self) -> &str {
        match self {
            Self::Current(session) => &session.id,
            Self::Persisted { session_id } => session_id,
        }
    }
}

/// One manually authorized, independently configured librarian attempt.
#[derive(Debug)]
pub struct LibrarianInvocation<'a> {
    pub target: LibrarianSessionTarget<'a>,
    pub overrides: LibrarianInvocationOverrides,
}

impl<'a> LibrarianInvocation<'a> {
    pub fn current(session: &'a Session, overrides: LibrarianInvocationOverrides) -> Self {
        Self {
            target: LibrarianSessionTarget::Current(session),
            overrides,
        }
    }

    pub fn persisted(session_id: &'a str, overrides: LibrarianInvocationOverrides) -> Self {
        Self {
            target: LibrarianSessionTarget::Persisted { session_id },
            overrides,
        }
    }
}

/// Deterministic, redacted provider input produced by local admission.
///
/// The payload is intentionally opaque outside this module so callers cannot
/// bypass filtering, token accounting, or fingerprint construction.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) struct AdmittedSessionContent {
    pub(crate) session_id: String,
    pub(crate) canonical_payload: Vec<u8>,
    pub(crate) input_tokens: u32,
}

/// One bounded provider response before schema validation and rendering.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) struct LibrarianGeneration {
    pub(crate) response_json: String,
    pub(crate) usage: BoundedUsage,
}

/// Matching renderings created from one validated
/// [`SessionSummary`](jcode_session_types::SessionSummary).
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) struct RenderedArtifactPair {
    pub(crate) markdown: Vec<u8>,
    pub(crate) json: Vec<u8>,
}

/// Immutable location of a published Markdown/JSON artifact pair.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibrarianArtifactPaths {
    directory: PathBuf,
    markdown: PathBuf,
    json: PathBuf,
}

impl LibrarianArtifactPaths {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            markdown: directory.join("summary.md"),
            json: directory.join("summary.json"),
            directory,
        }
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn markdown(&self) -> &Path {
        &self.markdown
    }

    pub fn json(&self) -> &Path {
        &self.json
    }
}

/// Stage that rejected an invocation before a successful publication.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LibrarianFailureStage {
    Resolution,
    Configuration,
    Admission,
    Fingerprinting,
    Generation,
    Validation,
    Rendering,
    Locking,
    Publication,
}

/// Actionable, credential-free terminal failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibrarianFailure {
    pub stage: LibrarianFailureStage,
    pub code: &'static str,
    pub message: String,
    pub usage: Option<BoundedUsage>,
}

impl std::fmt::Display for LibrarianFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for LibrarianFailure {}

/// Successful or reused publication details shared by both terminal outcomes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibrarianCompletion {
    pub session_id: String,
    pub source_fingerprint: SourceFingerprint,
    pub artifacts: LibrarianArtifactPaths,
    /// Usage incurred by this invocation. Reuse therefore reports zero requests.
    pub usage: BoundedUsage,
}

/// Exhaustive terminal state for one librarian invocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LibrarianResult {
    Reused(LibrarianCompletion),
    Succeeded(LibrarianCompletion),
    Failed(LibrarianFailure),
}

impl LibrarianResult {
    pub fn usage(&self) -> Option<&BoundedUsage> {
        match self {
            Self::Reused(completion) | Self::Succeeded(completion) => Some(&completion.usage),
            Self::Failed(failure) => failure.usage.as_ref(),
        }
    }
}

/// The single server-side entry point for the complete librarian workflow.
///
/// Implementations own source resolution, admission, fingerprinting, provider
/// generation, response validation, rendering, locking, and publication. This
/// focused contract is not a general plugin ABI and must not mutate the active
/// session or its provider route.
#[async_trait]
pub trait SessionLibrarian: Send + Sync {
    async fn invoke(&self, invocation: LibrarianInvocation<'_>) -> LibrarianResult;
}

/// Production one-shot librarian assembled from the existing config, session,
/// provider-runtime, and storage owners.
pub struct DefaultSessionLibrarian {
    provider_factory: Arc<dyn generation::GenerationProviderFactory>,
    config_override: Option<Config>,
    publication_root_override: Option<PathBuf>,
}

impl Default for DefaultSessionLibrarian {
    fn default() -> Self {
        Self {
            provider_factory: Arc::new(generation::NativeGenerationProviderFactory),
            config_override: None,
            publication_root_override: None,
        }
    }
}

impl DefaultSessionLibrarian {
    #[cfg(test)]
    fn with_components(
        config: Config,
        provider_factory: Arc<dyn generation::GenerationProviderFactory>,
        publication_root: PathBuf,
    ) -> Self {
        Self {
            provider_factory,
            config_override: Some(config),
            publication_root_override: Some(publication_root),
        }
    }

    fn config(&self) -> &Config {
        self.config_override
            .as_ref()
            .unwrap_or_else(|| jcode_base::config::config())
    }

    fn publication_root(&self) -> Result<PathBuf, LibrarianFailure> {
        if let Some(root) = &self.publication_root_override {
            return Ok(root.clone());
        }
        jcode_base::storage::jcode_dir()
            .map(|root| root.join("feedback").join("sessions"))
            .map_err(|_| LibrarianFailure {
                stage: LibrarianFailureStage::Publication,
                code: "librarian_feedback_directory_unavailable",
                message: "The session librarian feedback directory could not be resolved.".into(),
                usage: None,
            })
    }

    async fn invoke_session(
        &self,
        session: &Session,
        overrides: &LibrarianInvocationOverrides,
    ) -> Result<LibrarianResult, LibrarianFailure> {
        let active_route = active_route(session);
        let config = resolve_librarian_config(self.config(), overrides, &active_route, |route| {
            let facts = self.provider_factory.inspect(route);
            LibrarianRouteValidation {
                supported: facts.supported,
                authentication_available: facts.authentication_available,
                worst_case_cost_micros: facts.worst_case_cost_micros(12_000, 2_500),
            }
        })
        .map_err(configuration_failure)?;

        let admitted = admission::admit_session(session, &config.budgets, &config.admission_caps)?;
        let configuration_identity = configuration_identity(&config);
        let fingerprint =
            fingerprint::build_source_fingerprint(&admitted, &configuration_identity)?;
        let store = publication::PublicationStore::new(self.publication_root()?);

        match store.claim(&session.id, &fingerprint)? {
            publication::PublicationClaim::Reused(artifacts) => {
                Ok(LibrarianResult::Reused(LibrarianCompletion {
                    session_id: session.id.clone(),
                    source_fingerprint: fingerprint,
                    artifacts,
                    usage: zero_usage(),
                }))
            }
            publication::PublicationClaim::Generate(lease) => {
                let generation = generation::generate_summary(
                    self.provider_factory.as_ref(),
                    &config,
                    &admitted,
                )
                .await?;
                let generation = project_generation(
                    generation,
                    &session.id,
                    &fingerprint,
                    &configuration_identity.route,
                )?;
                let usage = generation.usage.clone();
                let artifacts = lease
                    .publish_generation(generation)
                    .map_err(|mut failure| {
                        failure.usage.get_or_insert_with(|| usage.clone());
                        failure
                    })?;
                Ok(LibrarianResult::Succeeded(LibrarianCompletion {
                    session_id: session.id.clone(),
                    source_fingerprint: fingerprint,
                    artifacts,
                    usage,
                }))
            }
        }
    }
}

#[async_trait]
impl SessionLibrarian for DefaultSessionLibrarian {
    async fn invoke(&self, invocation: LibrarianInvocation<'_>) -> LibrarianResult {
        let result = match invocation.target {
            LibrarianSessionTarget::Current(session) => {
                self.invoke_session(session, &invocation.overrides).await
            }
            LibrarianSessionTarget::Persisted { session_id } => match Session::load(session_id) {
                Ok(session) => self.invoke_session(&session, &invocation.overrides).await,
                Err(_) => Err(LibrarianFailure {
                    stage: LibrarianFailureStage::Resolution,
                    code: "source_session_not_found",
                    message: "No persisted session exists for the requested identifier.".into(),
                    usage: None,
                }),
            },
        };
        result.unwrap_or_else(LibrarianResult::Failed)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderSummary {
    summary: StructuredSummarySections,
    handoff_brief: String,
    relevant_files: LibrarianRelevantFiles,
}

fn project_generation(
    generation: LibrarianGeneration,
    session_id: &str,
    fingerprint: &SourceFingerprint,
    route: &RouteIdentity,
) -> Result<LibrarianGeneration, LibrarianFailure> {
    let provider: ProviderSummary =
        serde_json::from_str(&generation.response_json).map_err(|_| LibrarianFailure {
            stage: LibrarianFailureStage::Validation,
            code: "librarian_response_invalid",
            message:
                "The librarian provider response did not match the required summary content schema."
                    .into(),
            usage: Some(generation.usage.clone()),
        })?;
    let summary = SessionSummary {
        format_version: SUMMARY_FORMAT_VERSION.into(),
        session_id: session_id.into(),
        source_fingerprint: fingerprint.clone(),
        generated_at: chrono::Utc::now(),
        effective_route: route.clone(),
        usage: generation.usage.clone(),
        summary: provider.summary,
        handoff_brief: provider.handoff_brief,
        relevant_files: provider.relevant_files,
    };
    let response_json = serde_json::to_string(&summary).map_err(|_| LibrarianFailure {
        stage: LibrarianFailureStage::Validation,
        code: "librarian_response_projection_failed",
        message:
            "The validated librarian response could not be projected into the artifact schema."
                .into(),
        usage: Some(generation.usage.clone()),
    })?;
    Ok(LibrarianGeneration {
        response_json,
        usage: generation.usage,
    })
}

// Keep missing route metadata explicit. The repository's swallowed-error budget
// intentionally rejects `unwrap_or_default` in production code.
#[allow(clippy::manual_unwrap_or_default)]
fn active_route(session: &Session) -> LibrarianRouteIdentity {
    let provider = match session
        .route_api_method
        .clone()
        .or_else(|| session.provider_key.clone())
    {
        Some(provider) => provider,
        None => String::new(),
    };
    let model = match session.model.clone() {
        Some(model) => model,
        None => String::new(),
    };
    let reasoning_effort = match session.reasoning_effort.clone() {
        Some(reasoning_effort) => reasoning_effort,
        None => String::new(),
    };
    LibrarianRouteIdentity {
        provider,
        model,
        reasoning_effort,
    }
}

fn configuration_identity(config: &ResolvedLibrarianConfig) -> LibrarianConfigurationIdentity {
    LibrarianConfigurationIdentity {
        budgets: LibrarianBudgetIdentity {
            deadline_seconds: config.budgets.deadline_seconds,
            max_cost_micros_usd: config.budgets.max_cost_micros,
            max_input_tokens: config.budgets.max_input_tokens,
            max_output_tokens: config.budgets.max_output_tokens,
            max_requests: config.budgets.max_requests,
        },
        filter_version: FILTER_VERSION.into(),
        prompt_version: PROMPT_VERSION.into(),
        receipt_version: RECEIPT_VERSION.into(),
        route: RouteIdentity {
            provider: "openai".into(),
            api_method: config.route.provider.clone(),
            model: config.route.model.clone(),
            reasoning_effort: config.route.reasoning_effort.clone(),
        },
        schema_version: SUMMARY_FORMAT_VERSION.into(),
    }
}

fn zero_usage() -> BoundedUsage {
    BoundedUsage {
        input_tokens: 0,
        output_tokens: 0,
        request_count: 0,
        elapsed_ms: 0,
        cost_micros_usd: 0,
    }
}

fn configuration_failure(error: LibrarianConfigError) -> LibrarianFailure {
    let (code, message) = match error {
        LibrarianConfigError::InvalidRouteField { .. } => (
            "librarian_route_invalid",
            "The session librarian route contains an invalid provider, model, or effort value.",
        ),
        LibrarianConfigError::InvalidBudget { .. } => (
            "librarian_budget_invalid",
            "The session librarian requires positive finite hard-budget values.",
        ),
        LibrarianConfigError::UnsupportedRoute { .. } => (
            "librarian_route_unsupported",
            "The configured session librarian provider, model, or effort is unsupported.",
        ),
        LibrarianConfigError::MissingAuthentication { .. } => (
            "librarian_authentication_missing",
            "The configured session librarian route has no available authentication.",
        ),
        LibrarianConfigError::UnknownPricing { .. } => (
            "librarian_pricing_unknown",
            "The configured session librarian route has no verified pricing metadata.",
        ),
        LibrarianConfigError::UnsafeCost { .. } => (
            "librarian_cost_unapproved",
            "The session librarian worst-case cost exceeds the approved hard budget.",
        ),
    };
    LibrarianFailure {
        stage: LibrarianFailureStage::Configuration,
        code,
        message: message.into(),
        usage: None,
    }
}

#[cfg(test)]
#[path = "tests/admission.rs"]
mod admission_tests;

#[cfg(test)]
#[path = "tests/fingerprint.rs"]
mod fingerprint_tests;

#[cfg(test)]
#[path = "tests/generation.rs"]
mod generation_tests;

#[cfg(test)]
#[path = "tests/handoff.rs"]
mod handoff_tests;

#[cfg(test)]
#[path = "tests/publication.rs"]
mod publication_tests;

#[cfg(test)]
#[path = "tests/orchestration.rs"]
mod orchestration_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_invocation_keeps_only_the_requested_identifier() {
        let invocation =
            LibrarianInvocation::persisted("session-123", LibrarianInvocationOverrides::default());

        assert_eq!(invocation.target.requested_session_id(), "session-123");
        assert!(matches!(
            invocation.target,
            LibrarianSessionTarget::Persisted {
                session_id: "session-123"
            }
        ));
    }

    #[test]
    fn artifact_paths_always_name_the_matching_pair() {
        let paths = LibrarianArtifactPaths::new(PathBuf::from("generation"));

        assert_eq!(paths.directory(), Path::new("generation"));
        assert_eq!(paths.markdown(), Path::new("generation/summary.md"));
        assert_eq!(paths.json(), Path::new("generation/summary.json"));
    }
}
