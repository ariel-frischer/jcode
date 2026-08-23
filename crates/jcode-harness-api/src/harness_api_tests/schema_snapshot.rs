//! Schema snapshot tests: fail if the wire shape changes accidentally.

use crate::*;
use std::collections::HashSet;

#[test]
fn client_frame_wire_shape() {
    let frame = ClientFrame::new(
        7,
        ApiRequest::SendMessage {
            session_id: "s1".into(),
            content: "hi".into(),
            images: vec![],
            no_reply: false,
            max_turns: None,
            token_budget: None,
            deadline: None,
        },
    );
    let json = serde_json::to_string(&frame).unwrap();
    assert_eq!(
        json,
        r#"{"v":1,"id":7,"req":"send_message","session_id":"s1","content":"hi"}"#
    );
}

#[test]
fn create_session_profile_wire_shape_and_legacy_default() {
    let profile = serde_json::json!({"profile_name": "integration", "allowed_tools": []});
    let frame = ClientFrame::new(
        8,
        ApiRequest::CreateSession {
            working_dir: Some("/work".into()),
            profile: Some(profile.clone()),
        },
    );
    let value = serde_json::to_value(frame).unwrap();
    assert_eq!(value["profile"], profile);

    let legacy: ClientFrame =
        serde_json::from_str(r#"{"v":1,"id":9,"req":"create_session","working_dir":"/work"}"#)
            .unwrap();
    assert!(matches!(
        legacy.request,
        ApiRequest::CreateSession { profile: None, .. }
    ));
}

#[test]
fn send_message_no_reply_wire_shape_and_legacy_default() {
    let frame = ClientFrame::new(
        8,
        ApiRequest::SendMessage {
            session_id: "s1".into(),
            content: "context".into(),
            images: vec![],
            no_reply: true,
            max_turns: None,
            token_budget: None,
            deadline: None,
        },
    );
    let json = serde_json::to_string(&frame).unwrap();
    assert_eq!(
        json,
        r#"{"v":1,"id":8,"req":"send_message","session_id":"s1","content":"context","no_reply":true}"#
    );

    let legacy: ClientFrame = serde_json::from_str(
        r#"{"v":1,"id":9,"req":"send_message","session_id":"s1","content":"old","images":[]}"#,
    )
    .unwrap();
    assert!(matches!(
        legacy.request,
        ApiRequest::SendMessage {
            no_reply: false,
            ..
        }
    ));
}

#[test]
fn send_message_run_safety_wire_shape_and_legacy_default() {
    let frame = ClientFrame::new(
        10,
        ApiRequest::SendMessage {
            session_id: "s1".into(),
            content: "bounded".into(),
            images: vec![],
            no_reply: false,
            max_turns: Some(2),
            token_budget: Some(1000),
            deadline: Some("2030-01-01T00:00:00+00:00".into()),
        },
    );
    let value = serde_json::to_value(frame).unwrap();
    assert_eq!(value["max_turns"], 2);
    assert_eq!(value["token_budget"], 1000);
    assert_eq!(value["deadline"], "2030-01-01T00:00:00+00:00");

    let legacy: ClientFrame = serde_json::from_str(
        r#"{"v":1,"id":11,"req":"send_message","session_id":"s1","content":"legacy"}"#,
    )
    .unwrap();
    assert!(matches!(
        legacy.request,
        ApiRequest::SendMessage {
            max_turns: None,
            token_budget: None,
            deadline: None,
            ..
        }
    ));
}

#[test]
fn server_frame_wire_shape() {
    let frame = ServerFrame::reply(
        3,
        ApiEvent::HelloOk {
            version: 1,
            server: "jcode/0.55.1".into(),
            capabilities: vec![],
        },
    );
    let json = serde_json::to_string(&frame).unwrap();
    assert_eq!(
        json,
        r#"{"v":1,"reply_to":3,"ev":"hello_ok","version":1,"server":"jcode/0.55.1"}"#
    );
}

#[test]
fn unknown_event_kind_is_skippable() {
    let json = r#"{"v":1,"ev":"some_future_event","payload":123}"#;
    let frame: ServerFrame = serde_json::from_str(json).unwrap();
    assert_eq!(frame.event, ApiEvent::Unknown);
}

#[test]
fn every_api_event_has_one_explicit_publication_contract() {
    let events = [
        (
            ApiEvent::HelloOk {
                version: 1,
                server: "fixture".into(),
                capabilities: vec![],
            },
            "hello_ok",
        ),
        (ApiEvent::Ok, "ok"),
        (
            ApiEvent::Error {
                code: ErrorCode::Internal,
                message: "fixture".into(),
                provider_code: None,
            },
            "error",
        ),
        (ApiEvent::Sessions { sessions: vec![] }, "sessions"),
        (
            ApiEvent::Attached {
                session: SessionInfo {
                    session_id: "s".into(),
                    working_dir: None,
                    title: None,
                    status: "idle".into(),
                    transcript_bytes: None,
                    archived: false,
                    archived_at_ms: None,
                },
            },
            "attached",
        ),
        (
            ApiEvent::History {
                session_id: "s".into(),
                messages: vec![],
                images: vec![],
            },
            "history",
        ),
        (ApiEvent::Pong, "pong"),
        (
            ApiEvent::TextDelta {
                session_id: "s".into(),
                text: "x".into(),
            },
            "text_delta",
        ),
        (
            ApiEvent::ReasoningDelta {
                session_id: "s".into(),
                text: "x".into(),
            },
            "reasoning_delta",
        ),
        (
            ApiEvent::ReasoningDone {
                session_id: "s".into(),
                duration_secs: None,
            },
            "reasoning_done",
        ),
        (
            ApiEvent::ToolStart {
                session_id: "s".into(),
                call_id: "c".into(),
                name: "n".into(),
            },
            "tool_start",
        ),
        (
            ApiEvent::ToolInputDelta {
                session_id: "s".into(),
                call_id: "c".into(),
                delta: "x".into(),
            },
            "tool_input_delta",
        ),
        (
            ApiEvent::ToolExec {
                session_id: "s".into(),
                call_id: "c".into(),
                name: "n".into(),
            },
            "tool_exec",
        ),
        (
            ApiEvent::ToolDone {
                session_id: "s".into(),
                call_id: "c".into(),
                name: "n".into(),
                output: "x".into(),
                error: None,
            },
            "tool_done",
        ),
        (
            ApiEvent::SidePaneImages {
                session_id: "s".into(),
                images: vec![],
            },
            "side_pane_images",
        ),
        (
            ApiEvent::TokenUsage {
                session_id: "s".into(),
                input: 1,
                output: 2,
                cache_read_input: None,
            },
            "token_usage",
        ),
        (
            ApiEvent::TurnDone {
                session_id: "s".into(),
            },
            "turn_done",
        ),
        (
            ApiEvent::BackgroundProgress {
                session_id: "s".into(),
                task_id: "t".into(),
                label: "l".into(),
                percent: None,
                summary: "x".into(),
                done: false,
            },
            "background_progress",
        ),
        (
            ApiEvent::MessageAccepted {
                session_id: "s".into(),
            },
            "message_accepted",
        ),
        (
            ApiEvent::PermissionRequest {
                session_id: "s".into(),
                request_id: "r".into(),
                tool_name: "n".into(),
                description: "d".into(),
            },
            "permission_request",
        ),
        (
            ApiEvent::SessionStatus {
                session_id: "s".into(),
                status: "idle".into(),
            },
            "session_status",
        ),
        (
            ApiEvent::ConnectionPhase {
                session_id: "s".into(),
                phase: "connecting".into(),
            },
            "connection_phase",
        ),
        (
            ApiEvent::ModelInfo {
                session_id: "s".into(),
                provider: None,
                model: None,
                reasoning_effort: None,
            },
            "model_info",
        ),
        (
            ApiEvent::Models {
                session_id: "s".into(),
                models: vec![],
                current: None,
            },
            "models",
        ),
        (
            ApiEvent::RuntimeInfo {
                session_id: "s".into(),
                provider: None,
                model: None,
                reasoning_effort: None,
                routes: vec![],
            },
            "runtime_info",
        ),
        (
            ApiEvent::CredentialUpdated {
                provider: "p".into(),
                configured: true,
            },
            "credential_updated",
        ),
        (
            ApiEvent::FileContent {
                session_id: "s".into(),
                path: "p".into(),
                content: "x".into(),
                size: 1,
                truncated: false,
            },
            "file_content",
        ),
        (
            ApiEvent::Files {
                session_id: "s".into(),
                paths: vec![],
            },
            "files",
        ),
        (
            ApiEvent::TextMatches {
                session_id: "s".into(),
                matches: vec![],
            },
            "text_matches",
        ),
        (
            ApiEvent::FileStatus {
                session_id: "s".into(),
                path: "p".into(),
                exists: true,
                kind: "file".into(),
                size: None,
                modified_ms: None,
            },
            "file_status",
        ),
        (
            ApiEvent::Compacted {
                session_id: "s".into(),
                message: "x".into(),
            },
            "compacted",
        ),
        (
            ApiEvent::SessionRenamed {
                session_id: "s".into(),
                title: None,
                display_title: "x".into(),
            },
            "session_renamed",
        ),
        (ApiEvent::Unknown, "unknown"),
    ];
    assert_eq!(events.len(), api_event_variant_count());
    validate_publication_inventory(
        &events
            .iter()
            .map(|(event, _)| event.publication_contract())
            .collect::<Vec<_>>(),
    )
    .expect("canonical ApiEvent inventory must be complete and explicit");
    for (event, wire_kind) in events {
        let contract = event.publication_contract();
        assert_eq!(contract.wire_kind, wire_kind);
        match contract.disposition {
            EventPublicationDisposition::OwnedTurn => {
                assert!(
                    contract.semantic_class.is_some(),
                    "{wire_kind} lacks a semantic class"
                );
            }
            EventPublicationDisposition::Filtered
            | EventPublicationDisposition::OutsideOwnedTurn => {
                assert!(
                    contract.semantic_class.is_none(),
                    "{wire_kind} has a class outside the owned turn"
                );
            }
        }
    }
}

#[test]
fn exhaustive_inventory_rejects_uncovered_or_unsafe_fixtures() {
    let valid = EventPublicationContract {
        wire_kind: "fixture",
        disposition: EventPublicationDisposition::OwnedTurn,
        semantic_class: Some(EventSemanticClass::ContentProgress),
        filter_rationale: None,
    };
    let mut missing_class = valid;
    missing_class.semantic_class = None;
    assert!(validate_publication_inventory(&[missing_class]).is_err());

    let undeclared_filter = EventPublicationContract {
        wire_kind: "fixture_filter",
        disposition: EventPublicationDisposition::Filtered,
        semantic_class: None,
        filter_rationale: None,
    };
    assert!(validate_publication_inventory(&[undeclared_filter]).is_err());

    let duplicate = [valid, valid];
    assert!(validate_publication_inventory(&duplicate).is_err());
}

fn validate_publication_inventory(entries: &[EventPublicationContract]) -> Result<(), String> {
    let mut wire_kinds = HashSet::new();
    for entry in entries {
        if !wire_kinds.insert(entry.wire_kind) {
            return Err(format!("duplicate wire kind {}", entry.wire_kind));
        }
        match entry.disposition {
            EventPublicationDisposition::OwnedTurn => {
                if entry.semantic_class.is_none() || entry.filter_rationale.is_some() {
                    return Err(format!(
                        "owned event {} must have one class and no filter rationale",
                        entry.wire_kind
                    ));
                }
            }
            EventPublicationDisposition::Filtered => {
                if entry.semantic_class.is_some()
                    || entry
                        .filter_rationale
                        .map_or(true, |rationale| rationale.trim().is_empty())
                {
                    return Err(format!(
                        "filtered event {} requires a rationale and no class",
                        entry.wire_kind
                    ));
                }
            }
            EventPublicationDisposition::OutsideOwnedTurn => {
                if entry.semantic_class.is_some() || entry.filter_rationale.is_some() {
                    return Err(format!(
                        "outside-turn event {} cannot have class/filter metadata",
                        entry.wire_kind
                    ));
                }
            }
        }
    }
    Ok(())
}

fn api_event_variant_count() -> usize {
    let source = include_str!("../events.rs");
    let (_, rest) = source
        .split_once("pub enum ApiEvent {")
        .expect("ApiEvent declaration");
    let (body, _) = rest.split_once("\n}").expect("ApiEvent source body");
    body.lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            trimmed
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_uppercase())
                && trimmed
                    .chars()
                    .take_while(|ch| ch.is_ascii_alphanumeric())
                    .count()
                    > 0
                && (trimmed.contains('{') || trimmed.ends_with(','))
        })
        .count()
}

#[test]
fn owned_turn_contract_covers_each_semantic_class_without_wire_changes() {
    let classes = [
        (
            ApiEvent::TextDelta {
                session_id: "s".into(),
                text: "x".into(),
            },
            EventSemanticClass::ContentProgress,
        ),
        (
            ApiEvent::ConnectionPhase {
                session_id: "s".into(),
                phase: "connecting".into(),
            },
            EventSemanticClass::AdvisoryLifecycle,
        ),
        (
            ApiEvent::TurnDone {
                session_id: "s".into(),
            },
            EventSemanticClass::Terminal,
        ),
        (
            ApiEvent::PermissionRequest {
                session_id: "s".into(),
                request_id: "r".into(),
                tool_name: "n".into(),
                description: "d".into(),
            },
            EventSemanticClass::Permission,
        ),
        (
            ApiEvent::ToolExec {
                session_id: "s".into(),
                call_id: "c".into(),
                name: "n".into(),
            },
            EventSemanticClass::ToolEffect,
        ),
    ];
    for (event, expected) in classes {
        let contract = event.publication_contract();
        assert_eq!(contract.disposition, EventPublicationDisposition::OwnedTurn);
        assert_eq!(contract.semantic_class, Some(expected));
    }
}

#[test]
fn unknown_fields_are_ignored() {
    let json = r#"{"v":1,"ev":"turn_done","session_id":"s1","future_field":true}"#;
    let frame: ServerFrame = serde_json::from_str(json).unwrap();
    assert_eq!(
        frame.event,
        ApiEvent::TurnDone {
            session_id: "s1".into()
        }
    );
}

#[test]
fn request_roundtrip() {
    let reqs = [
        ApiRequest::Hello {
            min_version: 1,
            max_version: 1,
            client: "test/0".into(),
        },
        ApiRequest::ListSessions {
            include_archived: false,
            limit: None,
        },
        ApiRequest::ArchiveSession {
            session_id: "s1".into(),
        },
        ApiRequest::RestoreSession {
            session_id: "s1".into(),
        },
        ApiRequest::SetRetentionPolicy {
            archive_after_days: Some(30),
        },
        ApiRequest::CreateSession {
            working_dir: None,
            profile: None,
        },
        ApiRequest::AttachSession {
            session_id: "s1".into(),
        },
        ApiRequest::Cancel {
            session_id: "s1".into(),
        },
        ApiRequest::PermissionResponse {
            session_id: "s1".into(),
            request_id: "p1".into(),
            decision: PermissionDecision::Allow,
        },
        ApiRequest::GetRuntimeInfo {
            session_id: "s1".into(),
        },
        ApiRequest::SetApiKey {
            provider: "gemini".into(),
            api_key: "secret".into(),
        },
        ApiRequest::ClearApiKey {
            provider: "gemini".into(),
        },
        ApiRequest::ReadFile {
            session_id: "s1".into(),
            path: "src/lib.rs".into(),
            max_bytes: Some(1024),
        },
        ApiRequest::FindFiles {
            session_id: "s1".into(),
            query: "lib".into(),
            limit: Some(10),
        },
        ApiRequest::SearchText {
            session_id: "s1".into(),
            query: "needle".into(),
            path: Some("src".into()),
            limit: Some(10),
        },
        ApiRequest::FileStatus {
            session_id: "s1".into(),
            path: "src/lib.rs".into(),
        },
        ApiRequest::Ping,
    ];
    for req in reqs {
        let frame = ClientFrame::new(1, req);
        let json = serde_json::to_string(&frame).unwrap();
        let back: ClientFrame = serde_json::from_str(&json).unwrap();
        assert_eq!(frame, back);
    }
}

#[test]
fn client_handshake_over_in_memory_pipe() {
    // Server side scripted: one hello_ok line.
    let reply = serde_json::to_string(&ServerFrame::reply(
        1,
        ApiEvent::HelloOk {
            version: 1,
            server: "jcode/test".into(),
            capabilities: vec!["sessions".into()],
        },
    ))
    .unwrap()
        + "\n";
    let mut out: Vec<u8> = Vec::new();
    let mut client = HarnessClient::new(std::io::BufReader::new(reply.as_bytes()), &mut out);
    let frame = client.hello("test-client/0.1").unwrap();
    match frame.event {
        ApiEvent::HelloOk { version, .. } => assert_eq!(version, 1),
        other => panic!("unexpected event: {other:?}"),
    }
    let sent = String::from_utf8(out).unwrap();
    assert!(sent.contains(r#""req":"hello""#), "sent: {sent}");
}

/// The TypeScript SDK mirrors these enums by hand, so a variant added here
/// without a matching entry in `sdk/typescript/src/protocol.ts` silently
/// leaves every JS client unable to name the new frame. Checking from the
/// Rust side means the guard runs in the normal `cargo test` suite, where
/// the change is actually being made, rather than only in the SDK's own
/// Node tests which a Rust-only contributor never runs.
#[test]
fn typescript_sdk_lists_every_variant() {
    let Some(sdk) = sdk_protocol_source() else {
        // Absent in vendored/packaged builds: nothing to check.
        return;
    };
    for (file, enum_name) in [("requests.rs", "ApiRequest"), ("events.rs", "ApiEvent")] {
        for variant in enum_variants(file, enum_name) {
            assert!(
                sdk.contains(&format!("\"{variant}\"")),
                "{enum_name}::{variant} is missing from sdk/typescript/src/protocol.ts; \
                 add it to the union and to KNOWN_*_KINDS"
            );
        }
    }
}

fn sdk_protocol_source() -> Option<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../sdk/typescript/src/protocol.ts");
    std::fs::read_to_string(path).ok()
}

/// Tag parity is not enough: a variant can keep its name and gain a field,
/// and every JS client would then be unable to read the new data while all
/// existing tests stay green. Renaming a field is worse, because the SDK
/// keeps compiling against a name the wire no longer carries. Check the
/// payload shape too, from the same side the change is made on.
#[test]
fn typescript_sdk_lists_every_field() {
    let Some(sdk) = sdk_protocol_source() else {
        return;
    };
    for (file, enum_name, tag) in [
        ("requests.rs", "ApiRequest", "req"),
        ("events.rs", "ApiEvent", "ev"),
    ] {
        for (variant, fields) in enum_variant_fields(file, enum_name) {
            let Some(block) = sdk_variant_block(&sdk, tag, &variant) else {
                panic!(
                    "{enum_name}::{variant} has no `{tag}: \"{variant}\"` member in \
                     sdk/typescript/src/protocol.ts"
                );
            };
            for field in fields {
                assert!(
                    block.contains(&format!("{field}:")) || block.contains(&format!("{field}?:")),
                    "{enum_name}::{variant}.{field} is missing from its TypeScript member \
                     in sdk/typescript/src/protocol.ts:\n{block}"
                );
            }
        }
    }
}

/// The `{ ev: "tag"; ... }` object literal for one variant, braces balanced.
fn sdk_variant_block(sdk: &str, tag: &str, variant: &str) -> Option<String> {
    let needle = format!("{tag}: \"{variant}\"");
    let hit = sdk.find(&needle)?;
    let open = sdk[..hit].rfind('{')?;
    let mut depth = 0usize;
    for (offset, ch) in sdk[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(sdk[open..open + offset + 1].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// Snake-cased variant names paired with their serialized field names.
fn enum_variant_fields(file: &str, enum_name: &str) -> Vec<(String, Vec<String>)> {
    let body = enum_body(file, enum_name);
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for line in body.lines() {
        let Some(rest) = line.strip_prefix("    ") else {
            continue;
        };
        if rest.starts_with(' ') {
            // A field line inside the variant currently being collected.
            if let Some((_, fields)) = out.last_mut() {
                if let Some(name) = field_name(rest) {
                    fields.push(name);
                }
            }
            continue;
        }
        let name: String = rest
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric())
            .collect();
        let Some(first) = name.chars().next() else {
            continue;
        };
        if !first.is_ascii_uppercase() || name == "Unknown" {
            continue;
        }
        // Single-line variants carry their fields inline: `Foo { a: T, b: U },`.
        let inline = rest
            .split_once('{')
            .and_then(|(_, tail)| tail.rsplit_once('}'))
            .map(|(inner, _)| {
                inner
                    .split(',')
                    .filter_map(|part| field_name(part.trim()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        out.push((snake_case(&name), inline));
    }
    out
}

/// `name` from a `name: Type` field line, skipping attributes and comments.
fn field_name(line: &str) -> Option<String> {
    let line = line.trim();
    if line.starts_with('#') || line.starts_with("//") || line.is_empty() {
        return None;
    }
    let (name, _) = line.split_once(':')?;
    let name = name.trim();
    if name.is_empty()
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return None;
    }
    Some(name.to_string())
}

fn enum_body(file: &str, enum_name: &str) -> String {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join(file),
    )
    .expect("read API source");
    let start = source
        .find(&format!("pub enum {enum_name} {{"))
        .expect("enum present");
    let body = &source[start..];
    let end = body.find("\n}").unwrap_or(body.len());
    body[..end].to_string()
}

/// Snake-cased variant names of `enum_name`, excluding the `Unknown` catch-all.
fn enum_variants(file: &str, enum_name: &str) -> Vec<String> {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join(file),
    )
    .expect("read API source");
    let start = source
        .find(&format!("pub enum {enum_name} {{"))
        .expect("enum present");
    let body = &source[start..];
    let end = body.find("\n}").unwrap_or(body.len());
    body[..end]
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("    ")?;
            if rest.starts_with(' ') {
                return None;
            }
            let name: String = rest
                .chars()
                .take_while(|ch| ch.is_ascii_alphanumeric())
                .collect();
            let mut chars = name.chars();
            let first = chars.next()?;
            if !first.is_ascii_uppercase() || name == "Unknown" {
                return None;
            }
            Some(snake_case(&name))
        })
        .collect()
}

fn snake_case(name: &str) -> String {
    let mut out = String::new();
    for (index, ch) in name.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if index > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}
