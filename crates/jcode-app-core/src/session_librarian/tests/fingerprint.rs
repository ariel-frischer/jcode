use super::admission::admit_session;
use super::fingerprint::build_source_fingerprint;
use jcode_base::{
    config::{LibrarianAdmissionCaps, LibrarianBudgets},
    message::{ContentBlock, Role},
    session::Session,
};
use jcode_session_types::{
    LibrarianBudgetIdentity, LibrarianConfigurationIdentity, RouteIdentity, SourceFingerprint,
};
use serde_json::json;

const SAFE_CONTEXT: &str = "Decision: fingerprint only compact admitted session content.";
const ALGORITHM_VERSION: &str = "session-librarian-fingerprint.v1";

fn budgets() -> LibrarianBudgets {
    LibrarianBudgets {
        max_input_tokens: 12_000,
        max_output_tokens: 2_500,
        max_requests: 1,
        max_cost_micros: 500_000,
        deadline_seconds: 120,
    }
}

fn caps() -> LibrarianAdmissionCaps {
    LibrarianAdmissionCaps {
        max_receipt_bytes: 1_024,
        max_item_tokens: 768,
        max_normalized_file_tokens: 1_200,
        max_tool_category_tokens: 2_000,
    }
}

fn configuration() -> LibrarianConfigurationIdentity {
    LibrarianConfigurationIdentity {
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
        renderer_version: "session-librarian-markdown.v1".into(),
        route: RouteIdentity {
            provider: "openai".into(),
            api_method: "openai-oauth".into(),
            model: "gpt-5.6-luna".into(),
            reasoning_effort: "xhigh".into(),
        },
        schema_version: "session-summary.v1".into(),
    }
}

fn session_with_excluded_noise(secret: &str, raw_payload_bytes: usize) -> Session {
    let mut session = Session::create_with_id("fingerprint-session".into(), None, None);
    session.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: format!("{SAFE_CONTEXT}\nOPENAI_API_KEY={secret}"),
            cache_control: None,
        }],
    );
    session.add_message(
        Role::Assistant,
        vec![
            ContentBlock::Reasoning {
                text: format!("excluded reasoning {}", "r".repeat(raw_payload_bytes)),
            },
            ContentBlock::Image {
                media_type: "image/png".into(),
                data: format!("data:image/png;base64,{}", "a".repeat(raw_payload_bytes)),
            },
            ContentBlock::ToolUse {
                id: "unmatched-edit".into(),
                name: "apply_patch".into(),
                input: json!({
                    "patch_text": format!(
                        "*** Begin Patch\n+{}\n*** End Patch",
                        "p".repeat(raw_payload_bytes)
                    )
                }),
                thought_signature: None,
            },
        ],
    );
    session
}

fn admitted(session: &Session) -> super::AdmittedSessionContent {
    admit_session(session, &budgets(), &caps()).expect("fixture should retain safe compact content")
}

fn fingerprint(
    admitted: &super::AdmittedSessionContent,
    configuration: &LibrarianConfigurationIdentity,
) -> SourceFingerprint {
    build_source_fingerprint(admitted, configuration)
        .expect("valid admitted content and configuration should fingerprint")
}

fn assert_lowercase_sha256(digest: &str) {
    assert_eq!(digest.len(), 64, "SHA-256 must contain 64 hex digits");
    assert!(
        digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "digest must use lowercase hexadecimal"
    );
}

#[test]
fn equivalent_compact_redacted_content_has_one_stable_directory_safe_fingerprint() {
    let small = admitted(&session_with_excluded_noise(
        "sk-or-v1-0123456789abcdefghijklmnopqrstuv",
        1_024,
    ));
    let huge = admitted(&session_with_excluded_noise(
        "sk-or-v1-zyxwvutsrqponmlkjihgfedcba987654",
        8 * 1024 * 1024,
    ));

    assert_eq!(
        small.canonical_payload, huge.canonical_payload,
        "excluded raw reasoning, image, edit, patch, base64, and credential bytes must not change admitted content"
    );

    let first = fingerprint(&small, &configuration());
    let second = fingerprint(&huge, &configuration());

    assert_eq!(first, second);
    assert_eq!(first.algorithm_version, ALGORITHM_VERSION);
    assert_lowercase_sha256(&first.digest);
}

#[test]
fn every_summary_affecting_content_or_configuration_change_changes_the_digest() {
    let base_admitted = admitted(&session_with_excluded_noise(
        "sk-or-v1-0123456789abcdefghijklmnopqrstuv",
        1_024,
    ));
    let base_configuration = configuration();
    let baseline = fingerprint(&base_admitted, &base_configuration);

    let mut changed_content = base_admitted.clone();
    changed_content.canonical_payload = br#"{"version":1,"session_id":"fingerprint-session","items":[{"kind":"text","role":"user","text":"different admitted decision","estimated_tokens":10}]}"#.to_vec();
    assert_ne!(
        baseline.digest,
        fingerprint(&changed_content, &base_configuration).digest
    );

    let mut variants = Vec::new();

    let mut changed = base_configuration.clone();
    changed.filter_version = "session-librarian-filter.v2".into();
    variants.push(("filter", changed));

    let mut changed = base_configuration.clone();
    changed.prompt_version = "session-librarian-prompt.v2".into();
    variants.push(("prompt", changed));

    let mut changed = base_configuration.clone();
    changed.schema_version = "session-summary.v2".into();
    variants.push(("schema", changed));

    let mut changed = base_configuration.clone();
    changed.renderer_version = "session-librarian-markdown.v2".into();
    variants.push(("renderer", changed));

    let mut changed = base_configuration.clone();
    changed.route.model = "gpt-5.6-sol".into();
    variants.push(("route", changed));

    let mut changed = base_configuration.clone();
    changed.route.reasoning_effort = "medium".into();
    variants.push(("effort", changed));

    type BudgetMutation = (&'static str, fn(&mut LibrarianBudgetIdentity));
    let budget_mutations: &[BudgetMutation] = &[
        ("deadline", |budget: &mut LibrarianBudgetIdentity| {
            budget.deadline_seconds += 1
        }),
        ("cost", |budget: &mut LibrarianBudgetIdentity| {
            budget.max_cost_micros_usd += 1
        }),
        ("input", |budget: &mut LibrarianBudgetIdentity| {
            budget.max_input_tokens += 1
        }),
        ("output", |budget: &mut LibrarianBudgetIdentity| {
            budget.max_output_tokens += 1
        }),
        ("requests", |budget: &mut LibrarianBudgetIdentity| {
            budget.max_requests += 1
        }),
    ];
    for &(name, mutate) in budget_mutations {
        let mut changed = base_configuration.clone();
        mutate(&mut changed.budgets);
        variants.push((name, changed));
    }

    for (name, changed) in variants {
        assert_ne!(
            baseline.digest,
            fingerprint(&base_admitted, &changed).digest,
            "{name} changes must invalidate idempotent reuse"
        );
    }
}

#[test]
fn unstable_object_key_order_does_not_change_the_digest() {
    let ordered = super::AdmittedSessionContent {
        session_id: "fingerprint-session".into(),
        canonical_payload: br#"{"version":1,"session_id":"fingerprint-session","items":[{"kind":"receipt","operation":"edit","status":"success","counts":{"bytes":42,"lines":3},"estimated_tokens":20}]}"#.to_vec(),
        input_tokens: 20,
    };
    let reordered = super::AdmittedSessionContent {
        session_id: "fingerprint-session".into(),
        canonical_payload: br#"{"items":[{"estimated_tokens":20,"counts":{"lines":3,"bytes":42},"status":"success","operation":"edit","kind":"receipt"}],"session_id":"fingerprint-session","version":1}"#.to_vec(),
        input_tokens: 20,
    };

    assert_eq!(
        fingerprint(&ordered, &configuration()).digest,
        fingerprint(&reordered, &configuration()).digest,
        "canonical key ordering must not affect the digest"
    );
}

#[test]
fn receipt_format_version_is_part_of_the_idempotency_contract() {
    let admitted = admitted(&session_with_excluded_noise(
        "sk-or-v1-0123456789abcdefghijklmnopqrstuv",
        4_096,
    ));
    let baseline = fingerprint(&admitted, &configuration());
    let mut changed = configuration();
    changed.receipt_version = "session-librarian-receipt.v2".into();

    assert_ne!(baseline.digest, fingerprint(&admitted, &changed).digest);
    assert_eq!(
        baseline.configuration_identity.receipt_version,
        "session-librarian-receipt.v1"
    );
}
