use super::super::{
    CoordinatorSpawnIdentity, prepare_visible_spawn_session, resolve_coordinator_spawn_identity,
    resolve_spawn_profile_snapshot,
};
use crate::agent::Agent;
use crate::config::{
    ProfileRestoreStatus, ProviderModelReasoningSnapshot, ResolvedProfileSnapshot, SkillPolicy,
    SkillsMode, ToolPolicySnapshot,
};
use crate::provider::Provider;
use crate::tool::Registry;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

fn profiled_snapshot() -> ResolvedProfileSnapshot {
    ResolvedProfileSnapshot {
        profile_name: Some("review".to_owned()),
        provider_model_reasoning: ProviderModelReasoningSnapshot {
            provider: Some("openai".to_owned()),
            model: Some("gpt-review".to_owned()),
            reasoning_effort: Some("high".to_owned()),
            provider_profile: Some("review-route".to_owned()),
        },
        tool_policy: ToolPolicySnapshot {
            profile: Some("safe".to_owned()),
            allowed_tools: Some(vec!["read".to_owned(), "search".to_owned()]),
            disabled_tools: vec!["write".to_owned()],
        },
        skill_policy: SkillPolicy {
            mode: Some(SkillsMode::Allowlist),
            selected_skills: vec!["review".to_owned(), "tests".to_owned()],
            disabled_skills: vec!["tests".to_owned()],
            effective_skills: vec!["review".to_owned()],
        },
        prompt_overlay: Default::default(),
        fingerprint: String::new(),
    }
    .with_fingerprint()
}

#[tokio::test]
async fn coordinator_identity_inherits_profile_snapshot_and_restore_warning() {
    let _guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    crate::env::set_var("JCODE_HOME", temp_home.path());
    let snapshot = profiled_snapshot();
    let mut parent =
        crate::session::Session::create_with_id("profiled-parent".to_owned(), None, None);
    parent.model = Some("gpt-review".to_owned());
    parent.provider_key = Some("openai".to_owned());
    parent.profile_name = Some("review".to_owned());
    parent.profile_snapshot = Some(snapshot.clone());
    parent.profile_restore_status = Some(ProfileRestoreStatus::Changed {
        profile_name: "review".to_owned(),
        changed_fields: vec!["tool_policy".to_owned()],
    });
    parent.save().expect("persist parent metadata");

    let sessions = Arc::new(RwLock::new(HashMap::new()));
    let identity = resolve_coordinator_spawn_identity("profiled-parent", &sessions).await;
    assert_eq!(identity.profile_name.as_deref(), Some("review"));
    assert_eq!(identity.profile_snapshot, Some(snapshot));
    assert_eq!(
        identity.profile_restore_status,
        parent.profile_restore_status
    );
    crate::env::remove_var("JCODE_HOME");
}

#[test]
fn inherited_profile_snapshot_keeps_policy_and_updates_only_explicit_worker_fields() {
    let identity = CoordinatorSpawnIdentity {
        model: Some("gpt-review".to_owned()),
        provider_key: Some("openai".to_owned()),
        route_api_method: None,
        is_canary: false,
        profile_name: Some("review".to_owned()),
        profile_snapshot: Some(profiled_snapshot()),
        profile_restore_status: Some(ProfileRestoreStatus::Matching),
    };
    let parent_fingerprint = identity
        .profile_snapshot
        .as_ref()
        .unwrap()
        .fingerprint
        .clone();
    let child = resolve_spawn_profile_snapshot(&identity, Some("gpt-child"), Some("low"))
        .expect("profile snapshot should be inherited");
    assert_eq!(child.profile_name.as_deref(), Some("review"));
    assert_eq!(
        child.provider_model_reasoning.model.as_deref(),
        Some("gpt-child")
    );
    assert_eq!(
        child.provider_model_reasoning.reasoning_effort.as_deref(),
        Some("low")
    );
    assert_eq!(
        child.tool_policy.allowed_tools,
        Some(vec!["read".to_owned(), "search".to_owned()])
    );
    assert_eq!(child.skill_policy.mode, Some(SkillsMode::Allowlist));
    assert_eq!(child.skill_policy.selected_skills, vec!["review", "tests"]);
    assert_eq!(child.skill_policy.disabled_skills, vec!["tests"]);
    assert_ne!(child.fingerprint, parent_fingerprint);
    assert_eq!(
        identity.profile_snapshot.as_ref().unwrap().fingerprint,
        parent_fingerprint
    );
}

#[test]
fn visible_spawn_persists_inherited_profile_metadata_before_launch() {
    let _guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    crate::env::set_var("JCODE_HOME", temp_home.path());
    let worktree = tempfile::TempDir::new().expect("temp worktree");
    let snapshot = profiled_snapshot();
    let status = ProfileRestoreStatus::Missing {
        profile_name: "review".to_owned(),
    };
    let (session_id, launched) = prepare_visible_spawn_session(
        Some(worktree.path().to_str().expect("utf8 worktree path")),
        Some("gpt-review"),
        Some("openai"),
        None,
        Some("high"),
        false,
        Some("review".to_owned()),
        Some(snapshot.clone()),
        Some(status.clone()),
        None,
        |_session_id, _cwd: &std::path::Path, _selfdev, _provider_key| Ok(true),
    )
    .expect("visible inherited spawn should prepare");
    assert!(launched);
    let session = crate::session::Session::load(&session_id).expect("load child session");
    assert_eq!(session.profile_name.as_deref(), Some("review"));
    assert_eq!(session.profile_snapshot, Some(snapshot));
    assert_eq!(session.profile_restore_status, Some(status));
    crate::env::remove_var("JCODE_HOME");
}

#[tokio::test]
async fn child_agent_restores_inherited_policy_without_mutating_parent_metadata() {
    let snapshot = profiled_snapshot();
    let mut parent =
        crate::session::Session::create_with_id("policy-parent".to_owned(), None, None);
    parent.profile_name = Some("review".to_owned());
    parent.profile_snapshot = Some(snapshot.clone());
    parent.profile_restore_status = Some(ProfileRestoreStatus::Missing {
        profile_name: "review".to_owned(),
    });
    let parent_before = parent.clone();
    let mut child = crate::session::Session::create_with_id(
        "policy-child".to_owned(),
        Some(parent.id.clone()),
        None,
    );
    child.profile_name = parent.profile_name.clone();
    child.profile_snapshot = Some(
        resolve_spawn_profile_snapshot(
            &CoordinatorSpawnIdentity {
                model: Some("gpt-review".to_owned()),
                provider_key: Some("openai".to_owned()),
                route_api_method: None,
                is_canary: false,
                profile_name: parent.profile_name.clone(),
                profile_snapshot: parent.profile_snapshot.clone(),
                profile_restore_status: parent.profile_restore_status.clone(),
            },
            None,
            None,
        )
        .expect("child should inherit snapshot"),
    );
    child.profile_restore_status = parent.profile_restore_status.clone();
    let provider: Arc<dyn Provider> = Arc::new(super::MockProvider);
    let registry = Registry::new(provider.clone()).await;
    let agent = Agent::new_with_session(
        provider,
        registry,
        child.clone(),
        Some(
            ["read".to_owned(), "write".to_owned()]
                .into_iter()
                .collect(),
        ),
    );
    assert_eq!(agent.session_profile_name(), Some("review"));
    assert_eq!(agent.session_profile_snapshot(), Some(snapshot));
    assert_eq!(
        agent.session_profile_restore_status(),
        parent.profile_restore_status
    );
    assert_eq!(parent.profile_name, parent_before.profile_name);
    assert_eq!(parent.profile_snapshot, parent_before.profile_snapshot);
    assert_eq!(
        parent.profile_restore_status,
        parent_before.profile_restore_status
    );
    assert_eq!(child.parent_id.as_deref(), Some("policy-parent"));
}

#[tokio::test]
async fn child_override_snapshot_restores_effective_values_and_parent_warning() {
    let _guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    crate::env::set_var("JCODE_HOME", temp_home.path());
    let parent_snapshot = profiled_snapshot();
    let parent_status = ProfileRestoreStatus::Changed {
        profile_name: "review".to_owned(),
        changed_fields: vec!["provider".to_owned()],
    };
    let parent = CoordinatorSpawnIdentity {
        model: Some("gpt-review".to_owned()),
        provider_key: Some("openai".to_owned()),
        route_api_method: None,
        is_canary: false,
        profile_name: Some("review".to_owned()),
        profile_snapshot: Some(parent_snapshot.clone()),
        profile_restore_status: Some(parent_status.clone()),
    };
    let child_snapshot = resolve_spawn_profile_snapshot(&parent, Some("gpt-child"), Some("low"))
        .expect("child override should preserve inherited snapshot");
    let mut child = crate::session::Session::create_with_id(
        "override-child".to_owned(),
        Some("override-parent".to_owned()),
        None,
    );
    child.model = Some("gpt-child".to_owned());
    child.reasoning_effort = Some("low".to_owned());
    child.profile_name = parent.profile_name.clone();
    child.profile_snapshot = Some(child_snapshot.clone());
    child.profile_restore_status = Some(parent_status.clone());
    child.save().expect("persist child override metadata");
    let restored = crate::session::Session::load("override-child").expect("restore child metadata");
    assert_eq!(restored.profile_name, parent.profile_name);
    assert_eq!(restored.profile_snapshot, Some(child_snapshot));
    assert_eq!(restored.profile_restore_status, Some(parent_status));
    assert_eq!(restored.model.as_deref(), Some("gpt-child"));
    assert_eq!(restored.reasoning_effort.as_deref(), Some("low"));
    assert_eq!(parent.profile_snapshot, Some(parent_snapshot));
    crate::env::remove_var("JCODE_HOME");
}
