use super::publication::{PublicationClaim, PublicationStore};
use super::{LibrarianFailureStage, LibrarianGeneration};
use jcode_session_types::{
    BoundedUsage, LibrarianBudgetIdentity, LibrarianConfigurationIdentity, RouteIdentity,
    SessionSummary, SourceFingerprint,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{fs, path::Path};

const SESSION_ID: &str = "handoff-session";
const DIGEST: &str = "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
const FORMAT_VERSION: &str = "session-summary.v1";

fn route() -> RouteIdentity {
    RouteIdentity {
        provider: "openai".into(),
        api_method: "openai-oauth".into(),
        model: "gpt-5.6-luna".into(),
        reasoning_effort: "xhigh".into(),
    }
}

fn usage() -> BoundedUsage {
    BoundedUsage {
        input_tokens: 640,
        output_tokens: 420,
        request_count: 1,
        elapsed_ms: 1_250,
        cost_micros_usd: 24_000,
    }
}

fn fingerprint() -> SourceFingerprint {
    SourceFingerprint {
        algorithm_version: "session-librarian-fingerprint.v1".into(),
        digest: DIGEST.into(),
        configuration_identity: LibrarianConfigurationIdentity {
            budgets: LibrarianBudgetIdentity {
                deadline_seconds: 120,
                max_cost_micros_usd: 500_000,
                max_input_tokens: 12_000,
                max_output_tokens: 2_500,
                max_requests: 1,
            },
            filter_version: "session-librarian-filter.v1".into(),
            prompt_version: "session-librarian-prompt.v1".into(),
            receipt_version: "session-librarian-receipt.v1".into(),
            route: route(),
            schema_version: FORMAT_VERSION.into(),
        },
    }
}

fn summary_value(handoff_brief: &str, relevant_files: Value) -> Value {
    json!({
        "format_version": FORMAT_VERSION,
        "session_id": SESSION_ID,
        "source_fingerprint": fingerprint(),
        "generated_at": "2026-08-14T03:30:00Z",
        "effective_route": route(),
        "usage": usage(),
        "summary": {
            "goal": "Finish the session librarian handoff projection.",
            "outcomes": ["Safe summary artifacts are published atomically."],
            "decisions": ["Reuse the existing session_transition handoff contract."],
            "unresolved_work": ["Implement handoff normalization."],
            "risks": ["Provider-supplied paths must not bypass local validation."],
            "next_steps": ["Normalize and project continuation context."]
        },
        "handoff_brief": handoff_brief,
        "relevant_files": relevant_files
    })
}

fn publish(
    value: Value,
) -> Result<(tempfile::TempDir, super::LibrarianArtifactPaths), super::LibrarianFailure> {
    let temp = tempfile::tempdir().expect("handoff tempdir");
    let store = PublicationStore::new(temp.path().to_path_buf());
    let lease = match store
        .claim(SESSION_ID, &fingerprint())
        .expect("new fingerprint should acquire publication lease")
    {
        PublicationClaim::Generate(lease) => lease,
        PublicationClaim::Reused(_) => panic!("fresh fixture unexpectedly reused a generation"),
    };
    let paths = lease.publish_generation(LibrarianGeneration {
        response_json: serde_json::to_string(&value).expect("serialize handoff fixture"),
        usage: usage(),
    })?;

    debug_assert!(paths.directory().starts_with(temp.path()));
    Ok((temp, paths))
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct SessionTransitionProjection {
    prompt: String,
    relevant_files: Vec<String>,
}

#[test]
fn continuation_summary_projects_a_concise_normalized_handoff() {
    let (_temp, paths) = publish(summary_value(
        "  Continue by implementing the bounded handoff projection.\n",
        json!([
            "./crates/jcode-app-core/src/session_librarian/mod.rs",
            "crates/jcode-app-core/src/session_librarian/./mod.rs",
            "crates/jcode-app-core/src/session_librarian/tests/../tests/handoff.rs"
        ]),
    ))
    .expect("safe continuation context should publish");

    let json = fs::read_to_string(paths.json()).expect("read summary.json");
    let summary: SessionSummary = serde_json::from_str(&json).expect("summary JSON contract");
    assert_eq!(
        summary.handoff_brief,
        "Continue by implementing the bounded handoff projection."
    );
    assert_eq!(
        summary.relevant_files.as_slice(),
        [
            Path::new("crates/jcode-app-core/src/session_librarian/mod.rs"),
            Path::new("crates/jcode-app-core/src/session_librarian/tests/handoff.rs"),
        ]
    );
    assert!(summary.relevant_files.as_slice().len() <= 32);

    let transition: SessionTransitionProjection = serde_json::from_value(json!({
        "prompt": summary.handoff_brief,
        "relevant_files": summary
            .relevant_files
            .as_slice()
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    }))
    .expect("summary fields should populate session_transition without transcript parsing");
    assert!(!transition.prompt.is_empty());
    assert_eq!(transition.relevant_files.len(), 2);

    let markdown = fs::read_to_string(paths.markdown()).expect("read summary.md");
    assert!(markdown.contains("## Handoff brief"));
    assert!(markdown.contains(&transition.prompt));
    for path in &transition.relevant_files {
        assert!(markdown.contains(&format!("`{path}`")));
    }
}

#[test]
fn unsafe_or_unbounded_relevant_paths_are_removed_or_rejected_locally() {
    let oversized = format!("crates/{}", "a".repeat(2_048));
    let cases = [
        ("empty", String::new()),
        ("parent traversal", "../outside-the-workspace.rs".into()),
        ("secret file", ".env".into()),
        (
            "secret value",
            "OPENAI_API_KEY=sk-or-v1-0123456789abcdefghijklmnopqrstuv".into(),
        ),
        ("unbounded", oversized),
    ];

    for (name, unsafe_path) in cases {
        let value = summary_value(
            "Continue from the safe file only.",
            json!([
                "crates/jcode-app-core/src/session_librarian/mod.rs",
                unsafe_path
            ]),
        );

        match publish(value) {
            Ok((_temp, paths)) => {
                let json = fs::read_to_string(paths.json()).expect("read normalized summary.json");
                let summary: SessionSummary =
                    serde_json::from_str(&json).expect("normalized summary JSON contract");
                assert_eq!(
                    summary.relevant_files.as_slice(),
                    [Path::new(
                        "crates/jcode-app-core/src/session_librarian/mod.rs"
                    )],
                    "unsafe {name} path should be removed"
                );
            }
            Err(error) => assert_eq!(
                error.stage,
                LibrarianFailureStage::Validation,
                "unsafe {name} path should fail at local validation"
            ),
        }
    }
}

#[test]
fn empty_handoff_brief_is_rejected_when_continuation_context_exists() {
    let error = publish(summary_value(
        "  \n",
        json!(["crates/jcode-app-core/src/session_librarian/mod.rs"]),
    ))
    .expect_err("continuation files require a non-empty handoff brief");

    assert_eq!(error.stage, LibrarianFailureStage::Validation);
}
