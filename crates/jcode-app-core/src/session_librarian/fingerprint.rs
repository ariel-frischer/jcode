use super::{AdmittedSessionContent, LibrarianFailure, LibrarianFailureStage};
use jcode_session_types::{LibrarianConfigurationIdentity, SourceFingerprint};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

const ALGORITHM_VERSION: &str = "session-librarian-fingerprint.v1";

#[derive(Serialize)]
struct FingerprintInput<'a> {
    algorithm_version: &'static str,
    configuration: &'a LibrarianConfigurationIdentity,
    content: Value,
}

pub(crate) fn build_source_fingerprint(
    admitted: &AdmittedSessionContent,
    configuration: &LibrarianConfigurationIdentity,
) -> Result<SourceFingerprint, LibrarianFailure> {
    let content = serde_json::from_slice(&admitted.canonical_payload).map_err(|_| {
        fingerprint_failure(
            "librarian_invalid_admitted_content",
            "Session librarian could not fingerprint malformed admitted content.",
        )
    })?;
    let input = FingerprintInput {
        algorithm_version: ALGORITHM_VERSION,
        configuration,
        content,
    };
    let value = serde_json::to_value(input).map_err(|_| {
        fingerprint_failure(
            "librarian_fingerprint_serialization_failed",
            "Session librarian could not serialize the source fingerprint input.",
        )
    })?;
    let canonical = serde_json::to_vec(&canonicalize(value)).map_err(|_| {
        fingerprint_failure(
            "librarian_fingerprint_serialization_failed",
            "Session librarian could not serialize the source fingerprint input.",
        )
    })?;

    Ok(SourceFingerprint {
        algorithm_version: ALGORITHM_VERSION.to_string(),
        digest: format!("{:x}", Sha256::digest(canonical)),
        configuration_identity: configuration.clone(),
    })
}

fn canonicalize(value: Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.into_iter().map(canonicalize).collect()),
        Value::Object(values) => {
            let mut entries = values.into_iter().collect::<Vec<_>>();
            entries.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, canonicalize(value)))
                    .collect(),
            )
        }
        scalar => scalar,
    }
}

fn fingerprint_failure(code: &'static str, message: &'static str) -> LibrarianFailure {
    LibrarianFailure {
        stage: LibrarianFailureStage::Fingerprinting,
        code,
        message: message.to_string(),
        usage: None,
    }
}
