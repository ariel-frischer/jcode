//! Manually invoked session librarian orchestration.
//!
//! This module owns the complete one-shot workflow. Callers provide either the
//! canonical current [`Session`] or an explicit persisted session identifier and
//! receive exactly one terminal [`LibrarianResult`]. The implementation remains
//! responsible for resolving persisted sessions read-only, admitting and
//! fingerprinting content, using an independent provider route, validating and
//! rendering the response, and atomically publishing the artifact pair.

use async_trait::async_trait;
use jcode_base::{config::LibrarianInvocationOverrides, session::Session};
use jcode_session_types::{BoundedUsage, SourceFingerprint};
use std::path::{Path, PathBuf};

#[allow(dead_code)]
mod admission;
#[allow(dead_code)]
mod fingerprint;

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

#[cfg(test)]
#[path = "tests/admission.rs"]
mod admission_tests;

#[cfg(test)]
#[path = "tests/fingerprint.rs"]
mod fingerprint_tests;

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
