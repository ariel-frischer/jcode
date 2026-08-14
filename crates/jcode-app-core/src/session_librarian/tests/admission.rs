use super::admission::admit_session;
use jcode_base::{
    config::{LibrarianAdmissionCaps, LibrarianBudgets},
    message::{ContentBlock, Role},
    session::Session,
};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::Path;

const SECRET: &str = "sk-or-v1-0123456789abcdefghijklmnopqrstuv";
const SAFE_DECISION: &str = "Decision: keep the session librarian manually invoked.";
const SAFE_CONTINUATION: &str = "Next step: implement deterministic local admission.";
const HIDDEN_REASONING: &str = "HIDDEN_REASONING_UNIQUE_MARKER";
const STARTUP_BODY: &str = "STARTUP_INSTRUCTIONS_UNIQUE_MARKER";
const AGENTS_BODY: &str = "COMPLETE_AGENTS_BODY_UNIQUE_MARKER";
const SKILL_BODY: &str = "COMPLETE_SKILL_BODY_UNIQUE_MARKER";
const BULK_OUTPUT: &str = "BULK_SUCCESS_OUTPUT_UNIQUE_MARKER";
const RAW_CHANGE: &str = "RAW_EDIT_PATCH_UNIQUE_MARKER";
const INLINE_IMAGE: &str = "INLINE_IMAGE_UNIQUE_MARKER";
const BINARY_BLOB: &str = "BINARY_BLOB_UNIQUE_MARKER";
const BASE64_BLOB: &str = "BASE64_BLOB_UNIQUE_MARKER";

fn budgets(max_input_tokens: u32) -> LibrarianBudgets {
    LibrarianBudgets {
        max_input_tokens,
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

fn session() -> Session {
    Session::create_with_id("session-admission-test".into(), None, None)
}

fn text(text: impl Into<String>) -> ContentBlock {
    ContentBlock::Text {
        text: text.into(),
        cache_control: None,
    }
}

fn add_text(session: &mut Session, role: Role, value: impl Into<String>) {
    session.add_message(role, vec![text(value)]);
}

fn add_tool_attempt(
    session: &mut Session,
    call_id: &str,
    operation: &str,
    input: Value,
    result: impl Into<String>,
    is_error: bool,
) {
    session.add_message(
        Role::Assistant,
        vec![ContentBlock::ToolUse {
            id: call_id.into(),
            name: operation.into(),
            input,
            thought_signature: None,
        }],
    );
    session.add_message(
        Role::User,
        vec![ContentBlock::ToolResult {
            tool_use_id: call_id.into(),
            content: result.into(),
            is_error: Some(is_error),
        }],
    );
}

fn admit(session: &Session, max_input_tokens: u32) -> super::AdmittedSessionContent {
    admit_session(session, &budgets(max_input_tokens), &caps())
        .expect("fixture should contain eligible bounded content")
}

fn payload(admitted: &super::AdmittedSessionContent) -> Value {
    serde_json::from_slice(&admitted.canonical_payload)
        .expect("admitted content must use canonical JSON")
}

fn payload_text(admitted: &super::AdmittedSessionContent) -> String {
    String::from_utf8(admitted.canonical_payload.clone())
        .expect("canonical admission must be UTF-8 JSON")
}

fn receipt_items(payload: &Value) -> Vec<&Value> {
    payload["items"]
        .as_array()
        .expect("canonical admission should contain ordered items")
        .iter()
        .filter(|item| item["kind"] == "receipt")
        .collect()
}

fn assert_sha256(value: &Value, field: &str) {
    let digest = value[field]
        .as_str()
        .unwrap_or_else(|| panic!("receipt is missing {field}"));
    assert_eq!(digest.len(), 64, "{field} must be a SHA-256 hex digest");
    assert!(
        digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "{field} must use lowercase hexadecimal"
    );
}

#[test]
fn excluded_structural_categories_never_enter_provider_content() {
    let mut fixture = session();
    add_text(
        &mut fixture,
        Role::User,
        format!("<system-reminder>\n{STARTUP_BODY}\n</system-reminder>"),
    );
    add_text(
        &mut fixture,
        Role::User,
        format!("<system-reminder>\n{STARTUP_BODY}\n</system-reminder>"),
    );
    add_text(
        &mut fixture,
        Role::User,
        format!("# AGENTS.md\n{AGENTS_BODY}\n## Binding instructions"),
    );
    add_text(
        &mut fixture,
        Role::User,
        format!("## Skill: fixture-skill\n{SKILL_BODY}\n**Base directory**: /fixture"),
    );
    fixture.add_message(
        Role::Assistant,
        vec![
            ContentBlock::Reasoning {
                text: HIDDEN_REASONING.into(),
            },
            ContentBlock::ReasoningTrace {
                text: format!("{HIDDEN_REASONING}-trace"),
            },
            ContentBlock::AnthropicThinking {
                thinking: format!("{HIDDEN_REASONING}-anthropic"),
                signature: "signature".into(),
            },
            ContentBlock::OpenAIReasoning {
                id: "reasoning-id".into(),
                summary: vec![format!("{HIDDEN_REASONING}-openai")],
                encrypted_content: Some(format!("{BINARY_BLOB}-encrypted")),
                status: Some("completed".into()),
            },
            text(SAFE_DECISION),
        ],
    );
    fixture.add_message(
        Role::User,
        vec![ContentBlock::Image {
            media_type: "image/png".into(),
            data: format!("data:image/png;base64,{INLINE_IMAGE}{BASE64_BLOB}"),
        }],
    );
    add_tool_attempt(
        &mut fixture,
        "edit-1",
        "apply_patch",
        json!({
            "patch_text": format!("*** Begin Patch\n+{RAW_CHANGE}\n*** End Patch"),
            "intent": "Apply the bounded admission change"
        }),
        format!("Done. {BULK_OUTPUT} {}", "x".repeat(2 * 1024 * 1024)),
        false,
    );
    add_text(
        &mut fixture,
        Role::User,
        format!("data:application/octet-stream;base64,{BASE64_BLOB}{BINARY_BLOB}"),
    );
    add_text(&mut fixture, Role::User, SAFE_CONTINUATION);

    let admitted = admit(&fixture, 12_000);
    let provider_content = payload_text(&admitted);

    for excluded in [
        HIDDEN_REASONING,
        STARTUP_BODY,
        AGENTS_BODY,
        SKILL_BODY,
        BULK_OUTPUT,
        RAW_CHANGE,
        INLINE_IMAGE,
        BINARY_BLOB,
        BASE64_BLOB,
        "*** Begin Patch",
    ] {
        assert!(
            !provider_content.contains(excluded),
            "excluded marker reached provider content: {excluded}"
        );
    }
    assert!(provider_content.contains(SAFE_DECISION));
    assert!(provider_content.contains(SAFE_CONTINUATION));
}

#[test]
fn recognized_secrets_are_redacted_before_canonicalization_and_safe_text_is_unchanged() {
    let mut fixture = session();
    add_text(
        &mut fixture,
        Role::User,
        format!("{SAFE_DECISION}\nOPENAI_API_KEY={SECRET}\nDirect token: {SECRET}"),
    );
    add_tool_attempt(
        &mut fixture,
        "failure-1",
        "read",
        json!({"file_path": "src/lib.rs", "intent": SAFE_CONTINUATION}),
        format!("permission denied for token {SECRET}"),
        true,
    );

    let first = admit(&fixture, 12_000);
    let second = admit(&fixture, 12_000);
    let canonical = payload_text(&first);

    assert_eq!(first.canonical_payload, second.canonical_payload);
    assert!(!canonical.contains(SECRET));
    assert!(canonical.contains("[REDACTED_SECRET]"));
    assert!(canonical.contains(SAFE_DECISION));
    assert!(canonical.contains(SAFE_CONTINUATION));
}

#[test]
fn absolute_tool_paths_are_project_relative_or_omitted_before_provider_admission() {
    let mut fixture = session();
    fixture.working_dir = Some("/home/private-user/project".into());
    add_text(&mut fixture, Role::User, SAFE_DECISION);
    add_tool_attempt(
        &mut fixture,
        "inside-project",
        "read",
        json!({
            "file_path": "/home/private-user/project/src/lib.rs",
            "intent": "Inspect the project source"
        }),
        "status=ok bytes=42",
        false,
    );
    add_tool_attempt(
        &mut fixture,
        "outside-project",
        "read",
        json!({
            "file_path": "/home/private-user/.config/secret.toml",
            "intent": "Inspect an unrelated path"
        }),
        "status=ok bytes=17",
        false,
    );

    let admitted = admit(&fixture, 12_000);
    let parsed = payload(&admitted);
    let receipts = receipt_items(&parsed);
    let paths = receipts
        .iter()
        .filter_map(|receipt| receipt["path"].as_str())
        .collect::<Vec<_>>();

    assert_eq!(paths, vec!["src/lib.rs"]);
    let provider_content = payload_text(&admitted);
    assert!(!provider_content.contains("/home/private-user"));
    assert!(!provider_content.contains("secret.toml"));
}

#[test]
fn huge_file_and_tool_payloads_become_deterministic_one_kib_receipts() {
    let operations = ["edit", "multiedit", "apply_patch", "read", "write", "bash"];
    let payload_sizes = [
        64 * 1024,
        128 * 1024,
        256 * 1024,
        512 * 1024,
        1024 * 1024,
        2 * 1024 * 1024,
    ];

    for (operation, payload_size) in operations.into_iter().zip(payload_sizes) {
        let mut fixture = session();
        add_text(&mut fixture, Role::User, SAFE_DECISION);
        let path = format!("./src/../src/{operation}.rs");
        let raw = format!("{RAW_CHANGE}{}", "z".repeat(payload_size));
        add_tool_attempt(
            &mut fixture,
            &format!("{operation}-call"),
            operation,
            json!({
                "file_path": path,
                "intent": format!("Update {operation} behavior"),
                "content": raw,
                "old_string": RAW_CHANGE,
                "new_string": raw,
                "patch_text": format!("*** Begin Patch\n+{raw}\n*** End Patch")
            }),
            format!("status=ok bytes={payload_size} lines_added=17 {raw}"),
            false,
        );

        let first = admit(&fixture, 12_000);
        let second = admit(&fixture, 12_000);
        assert_eq!(first.canonical_payload, second.canonical_payload);

        let parsed = payload(&first);
        let receipts = receipt_items(&parsed);
        assert_eq!(receipts.len(), 1, "{operation} should produce one receipt");
        let receipt = receipts[0];
        let encoded = serde_json::to_vec(receipt).expect("receipt should serialize");
        assert!(
            encoded.len() <= 1_024,
            "{operation} receipt was {} bytes",
            encoded.len()
        );
        assert_eq!(receipt["operation"], operation);
        assert_eq!(receipt["path"], format!("src/{operation}.rs"));
        assert_eq!(receipt["status"], "success");
        assert!(receipt["counts"].is_object());
        assert_eq!(receipt["intent"], format!("Update {operation} behavior"));
        assert_sha256(receipt, "input_sha256");
        assert_sha256(receipt, "result_sha256");

        let canonical = payload_text(&first);
        assert!(!canonical.contains(RAW_CHANGE));
        assert!(!canonical.contains("*** Begin Patch"));
    }
}

#[test]
fn verbose_tool_counts_are_bounded_inside_one_receipt() {
    let mut fixture = session();
    add_text(&mut fixture, Role::User, SAFE_DECISION);
    let result = (0..100)
        .map(|index| format!("counter_{index}={index}"))
        .collect::<Vec<_>>()
        .join(" ");
    add_tool_attempt(
        &mut fixture,
        "verbose-counts",
        "bash",
        json!({"intent": "Record bounded validation counters"}),
        result,
        false,
    );

    let admitted = admit(&fixture, 12_000);
    let parsed = payload(&admitted);
    let receipts = receipt_items(&parsed);
    assert_eq!(receipts.len(), 1);
    assert!(
        receipts[0]["counts"]
            .as_object()
            .expect("counts object")
            .len()
            <= 16
    );
}

#[test]
fn repeated_attempts_deduplicate_but_failures_and_newest_continuation_remain_eligible() {
    let mut fixture = session();
    add_text(
        &mut fixture,
        Role::Assistant,
        "Old continuation context that may be reduced.",
    );
    for call_id in ["edit-attempt-1", "edit-attempt-2", "edit-attempt-3"] {
        add_tool_attempt(
            &mut fixture,
            call_id,
            "edit",
            json!({
                "file_path": "./src/lib.rs",
                "intent": "Apply the same safe edit",
                "old_string": "old",
                "new_string": "new"
            }),
            "status=ok replacements=1 lines_added=1 lines_removed=1",
            false,
        );
    }
    add_tool_attempt(
        &mut fixture,
        "failed-read",
        "read",
        json!({"file_path": "src/missing.rs", "intent": "Inspect missing input"}),
        format!(
            "permission denied while reading src/missing.rs; token={SECRET}; {}",
            "e".repeat(8_192)
        ),
        true,
    );
    add_text(&mut fixture, Role::Assistant, SAFE_DECISION);
    add_text(&mut fixture, Role::User, SAFE_CONTINUATION);

    let admitted = admit(&fixture, 12_000);
    let parsed = payload(&admitted);
    let receipts = receipt_items(&parsed);
    let edit_receipts = receipts
        .iter()
        .filter(|receipt| receipt["operation"] == "edit")
        .count();
    let canonical = payload_text(&admitted);

    assert_eq!(edit_receipts, 1, "identical retries must deduplicate");
    assert!(canonical.contains("permission denied"));
    assert!(!canonical.contains(SECRET));
    assert!(canonical.contains(SAFE_DECISION));
    assert!(canonical.contains(SAFE_CONTINUATION));
    assert!(admitted.input_tokens <= 12_000);
}

#[test]
fn admission_uses_only_recorded_metadata_and_enforces_caps_in_stable_order() {
    let temporary = tempfile::tempdir().expect("temporary repository root");
    let source_path = temporary.path().join("source-must-not-be-read.rs");
    std::fs::write(&source_path, "ORIGINAL_FILE_CONTENT_MUST_NOT_BE_ADMITTED")
        .expect("write source sentinel");

    let mut fixture = session();
    for index in 0..12 {
        add_text(
            &mut fixture,
            Role::User,
            format!("decision-{index}: {}", "safe context ".repeat(400)),
        );
    }
    for index in 0..10 {
        add_tool_attempt(
            &mut fixture,
            &format!("read-{index}"),
            "read",
            json!({
                "file_path": source_path,
                "intent": format!("Inspect recorded metadata {index}")
            }),
            format!(
                "status=ok bytes={} {}",
                64 * 1024,
                "recorded output ".repeat(5_000)
            ),
            false,
        );
    }
    add_text(&mut fixture, Role::Assistant, SAFE_DECISION);
    add_text(&mut fixture, Role::User, SAFE_CONTINUATION);

    let first = admit(&fixture, 2_100);
    std::fs::write(
        &source_path,
        "MUTATED_FILE_CONTENT_MUST_NOT_AFFECT_ADMISSION",
    )
    .expect("mutate source after recording session metadata");
    let second = admit(&fixture, 2_100);

    assert_eq!(
        first.canonical_payload, second.canonical_payload,
        "admission changed after the repository file changed, indicating an illicit reread"
    );
    assert!(first.input_tokens <= 2_100, "global cap was exceeded");
    let parsed = payload(&first);
    for item in parsed["items"].as_array().expect("items array") {
        assert!(
            item["estimated_tokens"].as_u64().expect("item token count") <= 768,
            "per-item cap was exceeded"
        );
    }
    let receipt_tokens: u64 = receipt_items(&parsed)
        .iter()
        .map(|item| item["estimated_tokens"].as_u64().expect("receipt tokens"))
        .sum();
    assert!(receipt_tokens <= 2_000, "tool-category cap was exceeded");

    let paths = receipt_items(&parsed)
        .iter()
        .filter_map(|receipt| receipt["path"].as_str())
        .collect::<BTreeSet<_>>();
    assert!(
        paths.len() <= 1,
        "per-file reduction admitted duplicate file context"
    );
    let canonical = payload_text(&first);
    assert!(!canonical.contains("ORIGINAL_FILE_CONTENT_MUST_NOT_BE_ADMITTED"));
    assert!(!canonical.contains("MUTATED_FILE_CONTENT_MUST_NOT_AFFECT_ADMISSION"));
    assert!(canonical.contains(SAFE_DECISION));
    assert!(canonical.contains(SAFE_CONTINUATION));
    assert!(Path::new(&source_path).exists());
}

#[test]
fn empty_eligible_content_fails_before_generation() {
    let mut fixture = session();
    fixture.add_message(
        Role::Assistant,
        vec![ContentBlock::Reasoning {
            text: HIDDEN_REASONING.into(),
        }],
    );
    fixture.add_message(
        Role::User,
        vec![ContentBlock::Image {
            media_type: "image/png".into(),
            data: INLINE_IMAGE.into(),
        }],
    );

    let error = admit_session(&fixture, &budgets(12_000), &caps())
        .expect_err("excluded-only input must fail before provider generation");

    assert_eq!(error.stage, super::LibrarianFailureStage::Admission);
    assert_eq!(error.code, "librarian_empty_content");
    assert!(error.usage.is_none());
    assert!(!error.message.contains(HIDDEN_REASONING));
    assert!(!error.message.contains(INLINE_IMAGE));
}
