#[test]
fn profile_startup_fixture_shapes_keep_selection_optional() {
    let legacy: serde_json::Value =
        serde_json::from_str(legacy_subscribe_payload()).expect("legacy payload should be JSON");
    let selected: serde_json::Value = serde_json::from_str(&profile_subscribe_payload("review"))
        .expect("profile payload should be JSON");

    assert!(legacy.get("profile_name").is_none());
    assert_eq!(selected.get("profile_name").and_then(|value| value.as_str()), Some("review"));
    assert_eq!(
        selected
            .get("profile_snapshot")
            .and_then(|value| value.get("fingerprint"))
            .and_then(|value| value.as_str()),
        Some("fixture-fingerprint")
    );
}

#[test]
fn typed_profile_startup_roundtrips_without_changing_legacy_shape() {
    let request = Request::Subscribe {
        id: 9,
        working_dir: Some("/tmp/interactive-profile".to_owned()),
        selfdev: None,
        target_session_id: None,
        client_instance_id: None,
        client_has_local_history: false,
        allow_session_takeover: false,
        terminal_env: Vec::new(),
        profile: Some(SessionProfileStartup {
            profile_name: Some("review".to_owned()),
            provider: Some("openai".to_owned()),
            model: Some("fixture-model".to_owned()),
            provider_profile: None,
            reasoning_effort: Some("low".to_owned()),
            allowed_tools: Some(vec!["read".to_owned()]),
            disabled_tools: Vec::new(),
            skill_names: Vec::new(),
            skills_mode: None,
            disabled_skills: Vec::new(),
            skill_prompts: Vec::new(),
            instructions: None,
        }),
    };
    let encoded = serde_json::to_string(&request).expect("profile request should serialize");
    let decoded: Request = serde_json::from_str(&encoded).expect("profile request should decode");
    let Request::Subscribe { profile, .. } = decoded else {
        panic!("expected Subscribe");
    };
    assert_eq!(profile.and_then(|profile| profile.profile_name), Some("review".to_owned()));

    let legacy: Request = serde_json::from_str(legacy_subscribe_payload())
        .expect("legacy Subscribe should still decode");
    let Request::Subscribe { profile, .. } = legacy else {
        panic!("expected legacy Subscribe");
    };
    assert!(profile.is_none());
}

#[test]
fn resume_session_profile_metadata_is_optional_and_roundtrips() {
    let request = Request::ResumeSession {
        id: 11,
        session_id: "profile-session".to_owned(),
        client_instance_id: None,
        client_has_local_history: false,
        allow_session_takeover: false,
        profile: Some(SessionProfileStartup {
            profile_name: Some("review".to_owned()),
            provider: Some("openai".to_owned()),
            model: Some("fixture-model".to_owned()),
            provider_profile: Some("team".to_owned()),
            reasoning_effort: Some("low".to_owned()),
            allowed_tools: Some(vec!["read".to_owned()]),
            disabled_tools: vec!["write".to_owned()],
            skill_names: vec!["review".to_owned()],
            skills_mode: Some(jcode_config_types::SkillsMode::Allowlist),
            disabled_skills: vec!["unsafe".to_owned()],
            skill_prompts: Vec::new(),
            instructions: None,
        }),
    };
    let encoded = serde_json::to_string(&request).expect("profile resume should serialize");
    let decoded: Request = serde_json::from_str(&encoded).expect("profile resume should decode");
    let Request::ResumeSession { profile, .. } = decoded else {
        panic!("expected ResumeSession");
    };
    let profile = profile.expect("profile metadata should survive roundtrip");
    assert_eq!(profile.profile_name.as_deref(), Some("review"));
    assert_eq!(profile.skills_mode, Some(jcode_config_types::SkillsMode::Allowlist));

    let legacy = Request::ResumeSession {
        id: 12,
        session_id: "legacy-session".to_owned(),
        client_instance_id: None,
        client_has_local_history: false,
        allow_session_takeover: false,
        profile: None,
    };
    let legacy_json = serde_json::to_value(legacy).expect("legacy resume should serialize");
    assert!(legacy_json.get("profile").is_none());
    let decoded: Request = serde_json::from_value(legacy_json).expect("legacy resume should decode");
    let Request::ResumeSession { profile, .. } = decoded else {
        panic!("expected legacy ResumeSession");
    };
    assert!(profile.is_none());
}

#[test]
fn legacy_session_fixture_has_no_profile_metadata_or_secret_values() {
    let payload = legacy_session_payload();
    let value: serde_json::Value = serde_json::from_str(payload).expect("legacy session JSON");

    assert!(value.get("profile_name").is_none());
    assert!(value.get("profile_snapshot").is_none());
    assert!(!payload.contains("api_key"));
    assert!(!payload.contains("credential"));
    assert!(!payload.contains("secret"));
}

/// Legacy Subscribe payload used by compatibility tests. It intentionally
/// omits every profile field so old clients continue to decode unchanged.
pub(super) fn legacy_subscribe_payload() -> &'static str {
    r#"{"type":"subscribe","id":7,"working_dir":"/tmp/legacy-profile"}"#
}

/// Profile-aware Subscribe fixture. Optional fields are represented as JSON
/// extensions until the wire contract adds typed serde-defaulted members.
pub(super) fn profile_subscribe_payload(profile_name: &str) -> String {
    assert!(!profile_name.trim().is_empty());
    serde_json::json!({
        "type": "subscribe",
        "id": 8,
        "working_dir": "/tmp/interactive-profile",
        "profile_name": profile_name,
        "profile_snapshot": safe_profile_snapshot(profile_name),
    })
    .to_string()
}

/// Minimal legacy session metadata fixture with no profile selection or
/// credential-bearing fields.
pub(super) fn legacy_session_payload() -> &'static str {
    r#"{"id":"legacy-session","messages":[],"model":"fixture-model"}"#
}

/// Session metadata fixture containing the redacted profile snapshot shape
/// shared by persistence and protocol tests.
pub(super) fn profile_session_payload(profile_name: &str) -> String {
    assert!(!profile_name.trim().is_empty());
    serde_json::json!({
        "id": "profile-session",
        "messages": [],
        "profile_name": profile_name,
        "profile_snapshot": safe_profile_snapshot(profile_name),
    })
    .to_string()
}

fn safe_profile_snapshot(profile_name: &str) -> serde_json::Value {
    serde_json::json!({
        "profile_name": profile_name,
        "provider": "openai",
        "model": "fixture-model",
        "reasoning_effort": "low",
        "tool_policy": {
            "profile": "minimal",
            "allowed": ["read"],
            "disabled": [],
        },
        "skill_policy": {
            "mode": "all",
            "selected": [],
            "disabled": [],
        },
        "prompt_overlay": {
            "instructions_present": false,
            "skill_names": [],
        },
        "fingerprint": "fixture-fingerprint",
    })
}

#[test]
fn profile_session_fixture_contains_only_safe_snapshot_fields() {
    let payload = profile_session_payload("review");
    let value: serde_json::Value = serde_json::from_str(&payload).expect("profile session JSON");

    assert_eq!(value.get("profile_name").and_then(|value| value.as_str()), Some("review"));
    assert_eq!(
        value
            .get("profile_snapshot")
            .and_then(|snapshot| snapshot.get("model"))
            .and_then(|value| value.as_str()),
        Some("fixture-model")
    );
    assert!(!payload.contains("api_key"));
    assert!(!payload.contains("credential"));
    assert!(!payload.contains("secret"));
}
