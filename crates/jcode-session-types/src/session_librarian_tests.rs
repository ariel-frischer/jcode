use super::{
    BoundedUsage, ResumeTarget, RouteIdentity, SessionSummary, SourceFingerprint,
    StructuredSummarySections,
};
use serde_json::{Value, json};

const FORMAT_VERSION: &str = "session-summary.v1";
const FINGERPRINT_VERSION: &str = "session-librarian-fingerprint.v1";
const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn summary_json(relevant_files: Vec<Value>) -> Value {
    json!({
        "format_version": FORMAT_VERSION,
        "session_id": "session-123",
        "source_fingerprint": {
            "algorithm_version": FINGERPRINT_VERSION,
            "digest": DIGEST,
            "configuration_identity": {
                "budgets": {
                    "deadline_seconds": 120,
                    "max_cost_micros_usd": 500_000,
                    "max_input_tokens": 12_000,
                    "max_output_tokens": 2_500,
                    "max_requests": 1
                },
                "filter_version": "session-librarian-filter.v1",
                "prompt_version": "session-librarian-prompt.v1",
                "receipt_version": "session-librarian-receipt.v1",
                "renderer_version": "session-librarian-markdown.v1",
                "route": {
                    "provider": "openai",
                    "api_method": "openai-oauth",
                    "model": "gpt-5.6-luna",
                    "reasoning_effort": "xhigh"
                },
                "schema_version": FORMAT_VERSION
            }
        },
        "generated_at": "2026-08-14T01:23:27Z",
        "effective_route": {
            "provider": "openai",
            "api_method": "openai-oauth",
            "model": "gpt-5.6-luna",
            "reasoning_effort": "xhigh"
        },
        "usage": {
            "input_tokens": 4_096,
            "output_tokens": 768,
            "request_count": 1,
            "elapsed_ms": 1_234,
            "cost_micros_usd": 125_000
        },
        "summary": {
            "goal": "Add a manually invoked bounded session librarian.",
            "outcomes": ["The artifact contract was specified."],
            "decisions": ["Keep stable contracts in jcode-session-types."],
            "unresolved_work": ["Implement the orchestration layer."],
            "risks": ["Provider output must be validated before publication."],
            "next_steps": ["Implement the dependency-light types."]
        },
        "handoff_brief": "Continue by implementing the versioned contracts and their validation.",
        "relevant_files": relevant_files
    })
}

#[test]
fn librarian_summary_json_shape_roundtrips_with_explicit_versions() {
    let expected = summary_json(vec![
        json!("crates/jcode-session-types/src/lib.rs"),
        json!("specs/007-add-session-librarian/plan.yaml"),
    ]);

    let summary: SessionSummary = serde_json::from_value(expected.clone())
        .expect("the versioned librarian summary fixture should deserialize");
    let encoded =
        serde_json::to_value(&summary).expect("the versioned librarian summary should serialize");

    assert_eq!(encoded, expected);
    assert_eq!(encoded["format_version"], FORMAT_VERSION);
    assert_eq!(
        encoded["source_fingerprint"]["algorithm_version"],
        FINGERPRINT_VERSION
    );
    assert_eq!(encoded["source_fingerprint"]["digest"], DIGEST);
    assert_eq!(
        encoded["summary"],
        json!({
            "goal": "Add a manually invoked bounded session librarian.",
            "outcomes": ["The artifact contract was specified."],
            "decisions": ["Keep stable contracts in jcode-session-types."],
            "unresolved_work": ["Implement the orchestration layer."],
            "risks": ["Provider output must be validated before publication."],
            "next_steps": ["Implement the dependency-light types."]
        })
    );
    assert_eq!(
        encoded["handoff_brief"],
        "Continue by implementing the versioned contracts and their validation."
    );
    assert_eq!(
        encoded["relevant_files"],
        json!([
            "crates/jcode-session-types/src/lib.rs",
            "specs/007-add-session-librarian/plan.yaml"
        ])
    );
}

#[test]
fn librarian_nested_contracts_keep_stable_json_shapes() {
    let summary: SessionSummary = serde_json::from_value(summary_json(Vec::new()))
        .expect("the librarian summary fixture should deserialize");
    let encoded = serde_json::to_value(summary).expect("summary should serialize");

    let fingerprint: SourceFingerprint =
        serde_json::from_value(encoded["source_fingerprint"].clone())
            .expect("source fingerprint should deserialize independently");
    assert_eq!(
        serde_json::to_value(fingerprint).expect("fingerprint should serialize"),
        encoded["source_fingerprint"]
    );

    let route: RouteIdentity = serde_json::from_value(encoded["effective_route"].clone())
        .expect("route identity should deserialize independently");
    assert_eq!(
        serde_json::to_value(route).expect("route identity should serialize"),
        json!({
            "provider": "openai",
            "api_method": "openai-oauth",
            "model": "gpt-5.6-luna",
            "reasoning_effort": "xhigh"
        })
    );

    let usage: BoundedUsage = serde_json::from_value(encoded["usage"].clone())
        .expect("bounded usage should deserialize independently");
    assert_eq!(
        serde_json::to_value(usage).expect("bounded usage should serialize"),
        json!({
            "input_tokens": 4_096,
            "output_tokens": 768,
            "request_count": 1,
            "elapsed_ms": 1_234,
            "cost_micros_usd": 125_000
        })
    );

    let sections: StructuredSummarySections = serde_json::from_value(encoded["summary"].clone())
        .expect("structured summary sections should deserialize independently");
    assert_eq!(
        serde_json::to_value(sections).expect("structured summary sections should serialize"),
        encoded["summary"]
    );
}

#[test]
fn librarian_route_serialization_cannot_contain_credentials() {
    fn assert_no_credential_keys(value: &Value) {
        match value {
            Value::Object(fields) => {
                for (key, nested) in fields {
                    assert!(
                        !matches!(
                            key.as_str(),
                            "api_key"
                                | "access_token"
                                | "oauth_token"
                                | "refresh_token"
                                | "credentials"
                                | "authorization"
                        ),
                        "serialized route contract exposed credential-bearing field {key}"
                    );
                    assert_no_credential_keys(nested);
                }
            }
            Value::Array(items) => {
                for item in items {
                    assert_no_credential_keys(item);
                }
            }
            _ => {}
        }
    }

    let summary: SessionSummary = serde_json::from_value(summary_json(Vec::new()))
        .expect("the librarian summary fixture should deserialize");
    let encoded = serde_json::to_value(summary).expect("summary should serialize");

    assert_no_credential_keys(&encoded["effective_route"]);
    assert_no_credential_keys(&encoded["source_fingerprint"]["configuration_identity"]["route"]);
}

#[test]
fn librarian_relevant_files_enforce_the_handoff_limit() {
    let accepted = (0..32)
        .map(|index| json!(format!("src/file-{index}.rs")))
        .collect();
    serde_json::from_value::<SessionSummary>(summary_json(accepted))
        .expect("the established handoff limit of 32 relevant files should be accepted");

    let rejected = (0..33)
        .map(|index| json!(format!("src/file-{index}.rs")))
        .collect();
    let error = serde_json::from_value::<SessionSummary>(summary_json(rejected))
        .expect_err("more than 32 relevant files must be rejected");

    assert!(
        error.to_string().contains("32"),
        "the bound failure should identify the 32-path contract: {error}"
    );
}

#[test]
fn existing_session_type_json_shape_is_unchanged() {
    let target = ResumeTarget::JcodeSession {
        session_id: "legacy-session".to_string(),
    };

    assert_eq!(
        serde_json::to_value(target).expect("legacy session type should still serialize"),
        json!({"JcodeSession": {"session_id": "legacy-session"}})
    );
}
