use crate::test_support::*;
use std::sync::atomic::Ordering;

#[test]
fn lifecycle_sidecars_preserve_session_replay_and_cleanup_compatibility() -> Result<()> {
    let _env = setup_test_env()?;
    let session_id = "session_lifecycle_e2e_compat";
    let mut session = Session::create_with_id(
        session_id.to_string(),
        None,
        Some("lifecycle compatibility smoke".to_string()),
    );
    session.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "ordinary transcript content".to_string(),
            cache_control: None,
        }],
    );
    session.save()?;

    let session_path = jcode::session::session_path(session_id)?;
    let jcode_dir = session_path
        .parent()
        .and_then(std::path::Path::parent)
        .context("session path should be nested beneath the jcode directory")?;
    let active = jcode::session::lifecycle_path_in_dir(jcode_dir, session_id)?;
    std::fs::write(&active, "{\"event\":\"internal lifecycle metadata\"}\n")?;
    for rotation in 1..=jcode::session::LIFECYCLE_MAX_ROTATIONS {
        std::fs::write(
            jcode::session::lifecycle_rotation_path_in_dir(jcode_dir, session_id, rotation)?,
            "{\"event\":\"rotated internal lifecycle metadata\"}\n",
        )?;
    }

    let loaded = Session::load(session_id)?;
    let timeline = jcode::replay::export_timeline(&loaded);
    let serialized_timeline = serde_json::to_string(&timeline)?;
    assert!(serialized_timeline.contains("ordinary transcript content"));
    assert!(!serialized_timeline.contains("internal lifecycle metadata"));

    jcode::session::remove_session_artifacts_in_dir(jcode_dir, session_id)?;
    assert!(!jcode::session::session_exists(session_id));
    assert!(!active.exists());
    for rotation in 1..=jcode::session::LIFECYCLE_MAX_ROTATIONS {
        assert!(
            !jcode::session::lifecycle_rotation_path_in_dir(jcode_dir, session_id, rotation)?
                .exists()
        );
    }
    Ok(())
}

#[test]
fn interactive_profile_startup_wire_is_optional_and_session_scoped() -> Result<()> {
    let request = jcode::protocol::Request::Subscribe {
        id: 17,
        working_dir: Some("/tmp/profile-e2e".to_owned()),
        selfdev: None,
        target_session_id: None,
        client_instance_id: Some("client-profile".to_owned()),
        client_has_local_history: false,
        allow_session_takeover: false,
        crash_on_disconnect: false,
        terminal_env: Vec::new(),
        profile: Some(jcode::protocol::SessionProfileStartup {
            profile_name: Some("review".to_owned()),
            provider: Some("openai".to_owned()),
            model: Some("fixture-model".to_owned()),
            provider_profile: None,
            reasoning_effort: Some("low".to_owned()),
            allowed_tools: Some(vec!["read".to_owned()]),
            disabled_tools: vec!["write".to_owned()],
            skill_names: Vec::new(),
            skills_mode: None,
            disabled_skills: Vec::new(),
            skill_prompts: Vec::new(),
            instructions: None,
        }),
    };
    let encoded = serde_json::to_string(&request)?;
    let decoded: jcode::protocol::Request = serde_json::from_str(&encoded)?;
    let jcode::protocol::Request::Subscribe { profile, .. } = decoded else {
        anyhow::bail!("expected Subscribe request");
    };
    assert_eq!(
        profile.and_then(|profile| profile.profile_name),
        Some("review".to_owned())
    );

    let legacy: jcode::protocol::Request =
        serde_json::from_str(r#"{"type":"subscribe","id":18,"working_dir":"/tmp/legacy"}"#)?;
    let jcode::protocol::Request::Subscribe { profile, .. } = legacy else {
        anyhow::bail!("expected legacy Subscribe request");
    };
    assert!(profile.is_none());
    Ok(())
}

#[tokio::test]
async fn selected_profile_uses_mock_setup_without_mutating_config_or_no_profile_session()
-> Result<()> {
    let _env = setup_test_env()?;
    let config_path = jcode::config::Config::path().expect("test config path should be available");
    std::fs::create_dir_all(config_path.parent().expect("config parent should exist"))?;
    std::fs::write(
        &config_path,
        r#"[profiles.review]
provider = "auto"
model = "profile-model"
reasoning_effort = "high"
tool_profile = "minimal"
tools = ["read", "write"]
disabled_tools = ["write"]
"#,
    )?;
    let persisted_before = std::fs::read(&config_path)?;
    jcode::config::Config::invalidate_cache();

    // Create the no-profile session before the profile run so the later request
    // proves that a per-run tool policy does not leak through shared state.
    let legacy_provider = Arc::new(MockProvider::new());
    let legacy_provider_for_agent: Arc<dyn Provider> = legacy_provider.clone();
    let legacy_registry = Registry::new(legacy_provider_for_agent.clone()).await;
    let mut legacy_agent = Agent::new(legacy_provider_for_agent, legacy_registry);
    let legacy_tools = legacy_agent.tool_names().await;

    let config = jcode::config::Config::load();
    let resolved = config
        .resolve_session_profile(Some("review"))
        .expect("the complete review profile should resolve");
    assert_eq!(resolved.provider.as_deref(), Some("auto"));
    assert_eq!(resolved.model.as_deref(), Some("profile-model"));
    assert_eq!(resolved.reasoning_effort.as_deref(), Some("high"));

    let profile_tools = jcode::config::ToolConfig {
        profile: resolved.tool_profile.clone().unwrap_or_default(),
        enabled: resolved.tools.clone(),
        disabled: resolved.disabled_tools.clone(),
        ..jcode::config::ToolConfig::default()
    }
    .selection()
    .allowed_tools
    .expect("the profile allow-list should produce a restricted tool set");

    let profile_provider = Arc::new(MockProvider::new());
    profile_provider.set_model(
        resolved
            .model
            .as_deref()
            .expect("the selected profile should provide a model"),
    )?;
    profile_provider.queue_response(vec![
        StreamEvent::TextDelta("profile response".to_string()),
        StreamEvent::MessageEnd {
            stop_reason: Some("end_turn".to_string()),
        },
    ]);
    let profile_provider_for_agent: Arc<dyn Provider> = profile_provider.clone();
    let profile_registry = Registry::new(profile_provider_for_agent.clone()).await;
    let profile_session = Session::create_with_id(
        "session_selected_profile_setup_test".to_string(),
        None,
        Some("selected profile setup test".to_string()),
    );
    let mut profile_agent = Agent::new_with_session(
        profile_provider_for_agent,
        profile_registry,
        profile_session,
        Some(profile_tools),
    );

    let profile_response = profile_agent
        .run_once_capture("profile setup request")
        .await?;
    assert_eq!(profile_response, "profile response");
    assert_eq!(profile_provider.request_count.load(Ordering::SeqCst), 1);
    assert_eq!(
        profile_provider.captured_models.lock().unwrap().clone(),
        vec![
            resolved
                .model
                .clone()
                .expect("profile model should be present")
        ]
    );
    let profile_tools_sent = {
        let captured_tools = profile_provider.captured_tools.lock().unwrap();
        captured_tools.clone()
    };
    assert_eq!(profile_tools_sent.len(), 1);
    assert_eq!(
        profile_tools_sent[0]
            .iter()
            .map(|tool| tool.name.clone())
            .collect::<Vec<_>>(),
        vec!["read".to_string()],
        "the profile allow/deny lists should expose only read"
    );
    legacy_provider.queue_response(vec![
        StreamEvent::TextDelta("legacy response".to_string()),
        StreamEvent::MessageEnd {
            stop_reason: Some("end_turn".to_string()),
        },
    ]);
    let legacy_response = legacy_agent
        .run_once_capture("no profile setup request")
        .await?;
    assert_eq!(legacy_response, "legacy response");
    assert_eq!(legacy_provider.request_count.load(Ordering::SeqCst), 1);
    assert_eq!(
        legacy_provider.captured_models.lock().unwrap().clone(),
        vec!["mock".to_string()]
    );
    let legacy_tools_sent = legacy_provider.captured_tools.lock().unwrap();
    assert_eq!(legacy_tools_sent.len(), 1);
    assert_eq!(
        legacy_tools_sent[0]
            .iter()
            .map(|tool| tool.name.clone())
            .collect::<Vec<_>>(),
        legacy_tools,
        "the no-profile session should retain its original tool surface"
    );
    drop(legacy_tools_sent);

    assert_eq!(
        std::fs::read(&config_path)?,
        persisted_before,
        "selecting a profile must not rewrite persisted configuration"
    );

    Ok(())
}

#[tokio::test]
async fn explicit_run_overrides_profile_values_in_request_and_preserves_next_session() -> Result<()>
{
    let _env = setup_test_env()?;
    let config_path = jcode::config::Config::path().expect("test config path should be available");
    std::fs::create_dir_all(config_path.parent().expect("config parent should exist"))?;
    std::fs::write(
        &config_path,
        r#"[profiles.review]
provider = "openai"
model = "profile-model"
reasoning_effort = "high"
provider_profile = "profile-gateway"
tool_profile = "full"
tools = ["read", "write"]
disabled_tools = ["write"]
"#,
    )?;
    let persisted_before = std::fs::read(&config_path)?;
    jcode::config::Config::invalidate_cache();

    let config = jcode::config::Config::load();
    let resolved = config
        .resolve_session_profile(Some("review"))
        .expect("the review profile should resolve");
    assert_eq!(resolved.provider.as_deref(), Some("openai"));
    assert_eq!(resolved.model.as_deref(), Some("profile-model"));
    assert_eq!(resolved.reasoning_effort.as_deref(), Some("high"));
    assert_eq!(
        resolved.provider_profile.as_deref(),
        Some("profile-gateway")
    );

    // These values model explicit invocation flags. The profile's disabled
    // list remains effective because the invocation did not supply a deny-list.
    let explicit_model = "explicit-model";
    let explicit_reasoning = "low";
    let explicit_tools = jcode::config::ToolConfig {
        profile: "none".to_owned(),
        enabled: vec!["bash".to_owned(), "write".to_owned()],
        disabled: resolved.disabled_tools.clone(),
        ..jcode::config::ToolConfig::default()
    }
    .selection();

    let provider = Arc::new(MockProvider::with_models(vec![
        "profile-model",
        explicit_model,
    ]));
    provider.set_model(explicit_model)?;
    provider.set_reasoning_effort(explicit_reasoning)?;
    provider.queue_response(vec![
        StreamEvent::TextDelta("explicit response".to_string()),
        StreamEvent::MessageEnd {
            stop_reason: Some("end_turn".to_string()),
        },
    ]);
    let provider_for_agent: Arc<dyn Provider> = provider.clone();
    let registry = Registry::new(provider_for_agent.clone()).await;
    let mut agent = Agent::new_with_tool_selection(provider_for_agent, registry, explicit_tools);

    assert_eq!(
        agent.run_once_capture("explicit override request").await?,
        "explicit response"
    );
    assert_eq!(provider.request_count.load(Ordering::SeqCst), 1);
    assert_eq!(
        provider.captured_models.lock().unwrap().as_slice(),
        &[explicit_model.to_owned()]
    );
    assert_eq!(
        provider.reasoning_effort().as_deref(),
        Some(explicit_reasoning)
    );
    assert_eq!(
        provider
            .captured_reasoning_efforts
            .lock()
            .unwrap()
            .as_slice(),
        &[Some(explicit_reasoning.to_owned())]
    );
    assert_eq!(
        provider.captured_tools.lock().unwrap()[0]
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        vec!["bash"],
        "the explicit allow-list wins while the unset profile deny-list still removes write"
    );

    assert_eq!(
        std::fs::read(&config_path)?,
        persisted_before,
        "explicit profile overrides must not rewrite persisted configuration"
    );

    let legacy_provider = Arc::new(MockProvider::new());
    legacy_provider.queue_response(vec![
        StreamEvent::TextDelta("legacy response".to_string()),
        StreamEvent::MessageEnd {
            stop_reason: Some("end_turn".to_string()),
        },
    ]);
    let legacy_provider_for_agent: Arc<dyn Provider> = legacy_provider.clone();
    let legacy_registry = Registry::new(legacy_provider_for_agent.clone()).await;
    let mut legacy_agent = Agent::new(legacy_provider_for_agent, legacy_registry);
    let legacy_tools = legacy_agent.tool_names().await;

    assert_eq!(
        legacy_agent.run_once_capture("legacy request").await?,
        "legacy response"
    );
    assert_eq!(legacy_provider.request_count.load(Ordering::SeqCst), 1);
    assert_eq!(
        legacy_provider.captured_models.lock().unwrap().as_slice(),
        &["mock".to_owned()]
    );
    assert_eq!(
        legacy_provider.captured_tools.lock().unwrap()[0]
            .iter()
            .map(|tool| tool.name.clone())
            .collect::<Vec<_>>(),
        legacy_tools,
        "the next no-profile session must retain the legacy tool surface"
    );
    assert_eq!(
        std::fs::read(&config_path)?,
        persisted_before,
        "the no-profile session must not observe a persisted profile mutation"
    );

    Ok(())
}

#[tokio::test]
async fn no_profile_request_preserves_legacy_provider_setup_and_prompt() -> Result<()> {
    let _env = setup_test_env()?;
    let provider = Arc::new(MockProvider::new());
    provider.queue_response(vec![
        StreamEvent::TextDelta("legacy response".to_string()),
        StreamEvent::MessageEnd {
            stop_reason: Some("end_turn".to_string()),
        },
    ]);

    let provider_for_agent: Arc<dyn Provider> = provider.clone();
    let registry = Registry::new(provider_for_agent.clone()).await;
    let mut agent = Agent::new(provider_for_agent, registry);
    let legacy_tools = agent.tool_names().await;

    let response = agent
        .run_once_capture("legacy no-profile regression prompt")
        .await?;

    assert_eq!(response, "legacy response");
    assert_eq!(provider.request_count.load(Ordering::SeqCst), 1);
    assert_eq!(provider.name(), "mock");
    assert_eq!(provider.model(), "mock");
    assert_eq!(
        provider.captured_models.lock().unwrap().clone(),
        vec!["mock".to_string()],
        "the no-profile path must retain the legacy provider model"
    );

    let prompts = provider.captured_system_prompts.lock().unwrap();
    assert_eq!(prompts.len(), 1);
    assert!(
        prompts[0].starts_with("## Identity\n\nYour name is Jcode."),
        "the legacy static prompt bytes must be sent unchanged at the provider boundary"
    );
    drop(prompts);

    let messages = provider.captured_messages.lock().unwrap();
    assert_eq!(messages.len(), 1);
    assert!(
        messages[0].iter().any(|message| {
            message.role == Role::User
                && message.content.iter().any(|block| {
                    matches!(block, ContentBlock::Text { text, .. } if text.contains("legacy no-profile regression prompt"))
                })
        }),
        "the no-profile request must preserve the user prompt bytes"
    );
    drop(messages);

    let tools = provider.captured_tools.lock().unwrap();
    assert_eq!(tools.len(), 1);
    let captured_tool_names = tools[0]
        .iter()
        .map(|tool| tool.name.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        captured_tool_names, legacy_tools,
        "the no-profile request must expose the legacy tool surface"
    );
    assert!(captured_tool_names.iter().any(|name| name == "read"));
    assert!(captured_tool_names.iter().any(|name| name == "bash"));

    Ok(())
}

#[tokio::test]
async fn resume_session_restores_persisted_compaction_for_provider_context() -> Result<()> {
    let _env = setup_test_env()?;
    let runtime_dir = short_runtime_dir(format!(
        "jcode-compaction-resume-test-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&runtime_dir)?;
    let socket_path = runtime_dir.join("jcode.sock");
    let debug_socket_path = runtime_dir.join("jcode-debug.sock");

    let provider = CapturingCompactionProvider::new();
    let captured_messages = provider.captured_messages();
    let provider: Arc<dyn Provider> = Arc::new(provider);
    let server_instance =
        server::Server::new_with_paths(provider, socket_path.clone(), debug_socket_path.clone());
    let server_handle = tokio::spawn(async move { server_instance.run().await });

    let result = async {
        let mut session = Session::create_with_id(
            "session_resume_compaction_restore_test".to_string(),
            None,
            Some("resume compaction restore test".to_string()),
        );
        session.add_message(
            Role::User,
            vec![ContentBlock::Text {
                text: "older user turn".to_string(),
                cache_control: None,
            }],
        );
        session.add_message(
            Role::Assistant,
            vec![ContentBlock::Text {
                text: "older assistant turn".to_string(),
                cache_control: None,
            }],
        );
        session.add_message(
            Role::User,
            vec![ContentBlock::Text {
                text: "recent preserved turn".to_string(),
                cache_control: None,
            }],
        );
        session.compaction = Some(StoredCompactionState {
            summary_text: "Worked on Gemini OAuth reload fixes.".to_string(),
            openai_encrypted_content: None,
            covers_up_to_turn: 2,
            original_turn_count: 2,
            compacted_count: 2,
        });
        session.save()?;

        wait_for_server_ready(&socket_path, &debug_socket_path).await?;
        let mut client = server::Client::connect_with_path(socket_path.clone()).await?;

        let subscribe_id = client.subscribe().await?;
        let _ = collect_until_done_unix(&mut client, subscribe_id).await?;

        let resume_id = client.resume_session(&session.id).await?;
        let _ = collect_until_history_unix(&mut client, resume_id).await?;

        let message_id = client
            .send_message("continue from the restored session")
            .await?;
        let mut seen_events = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            let event = timeout(Duration::from_secs(1), client.read_event()).await??;
            let is_done = matches!(event, ServerEvent::Done { id } if id == message_id);
            let is_error = matches!(event, ServerEvent::Error { id, .. } if id == message_id);
            seen_events.push(format!("{event:?}"));
            if is_done {
                break;
            }
            if is_error {
                anyhow::bail!(
                    "message request failed while validating compaction restore: {}",
                    seen_events.join(" | ")
                );
            }
        }

        let captured = captured_messages.lock().unwrap();
        assert_eq!(
            captured.len(),
            1,
            "expected exactly one provider completion call"
        );
        let provider_messages = &captured[0];
        assert!(
            provider_messages.len() >= 3,
            "expected summary + preserved tail + new user message"
        );

        let summary_text = flatten_text_blocks(&provider_messages[0]);
        assert!(summary_text.contains("Previous Conversation Summary"));
        assert!(summary_text.contains("Gemini OAuth reload fixes"));

        let joined = provider_messages
            .iter()
            .map(flatten_text_blocks)
            .collect::<Vec<_>>()
            .join("\n---\n");
        assert!(joined.contains("recent preserved turn"));
        assert!(joined.contains("continue from the restored session"));
        assert!(!joined.contains("older user turn"));
        assert!(!joined.contains("older assistant turn"));

        Ok::<_, anyhow::Error>(())
    }
    .await;

    abort_server_and_cleanup(&server_handle, &socket_path, &debug_socket_path);
    result
}

/// Test that a simple text response works
#[tokio::test]
async fn test_simple_response() -> Result<()> {
    let _env = setup_test_env()?;
    let provider = MockProvider::new();

    // Queue a simple response
    provider.queue_response(vec![
        StreamEvent::TextDelta("Hello! ".to_string()),
        StreamEvent::TextDelta("How can I help?".to_string()),
        StreamEvent::MessageEnd {
            stop_reason: Some("end_turn".to_string()),
        },
        StreamEvent::SessionId("test-session-123".to_string()),
    ]);

    let provider: Arc<dyn jcode::provider::Provider> = Arc::new(provider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);

    let response = agent.run_once_capture("Say hello").await?;
    let saved = Session::load(agent.session_id())?;

    assert_eq!(response, "Hello! How can I help?");
    assert!(saved.is_debug, "test sessions should be marked debug");
    Ok(())
}

#[tokio::test]
async fn test_agent_clear_preserves_debug_flag() -> Result<()> {
    let _env = setup_test_env()?;
    let provider = MockProvider::new();
    let provider: Arc<dyn jcode::provider::Provider> = Arc::new(provider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);
    agent.set_debug(true);
    let old_session_id = agent.session_id().to_string();

    agent.clear();

    assert_ne!(agent.session_id(), old_session_id);
    assert!(agent.is_debug());
    Ok(())
}

#[tokio::test]
async fn test_debug_create_session_marks_debug() -> Result<()> {
    let _env = setup_test_env()?;
    let runtime_dir = short_runtime_dir(format!(
        "jcode-debug-test-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&runtime_dir)?;
    let socket_path = runtime_dir.join("jcode.sock");
    let debug_socket_path = runtime_dir.join("jcode-debug.sock");

    let provider = MockProvider::new();
    let provider: Arc<dyn jcode::provider::Provider> = Arc::new(provider);
    let server_instance =
        server::Server::new_with_paths(provider, socket_path.clone(), debug_socket_path.clone());
    let server_handle = tokio::spawn(async move { server_instance.run().await });

    wait_for_server_ready(&socket_path, &debug_socket_path).await?;

    let session_id = debug_create_headless_session(debug_socket_path.clone()).await?;
    let session = Session::load(&session_id)?;
    assert!(session.is_debug);

    abort_server_and_cleanup(&server_handle, &socket_path, &debug_socket_path);

    Ok(())
}

#[tokio::test]
async fn test_debug_create_selfdev_session_marks_canary() -> Result<()> {
    let _env = setup_test_env()?;
    let runtime_dir = short_runtime_dir(format!(
        "jcode-debug-selfdev-test-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&runtime_dir)?;
    let socket_path = runtime_dir.join("jcode.sock");
    let debug_socket_path = runtime_dir.join("jcode-debug.sock");

    let provider = MockProvider::new();
    let provider: Arc<dyn jcode::provider::Provider> = Arc::new(provider);
    let server_instance =
        server::Server::new_with_paths(provider, socket_path.clone(), debug_socket_path.clone());
    let server_handle = tokio::spawn(async move { server_instance.run().await });

    wait_for_server_ready(&socket_path, &debug_socket_path).await?;

    let session_id = debug_create_headless_session_with_command(
        debug_socket_path.clone(),
        "create_session:selfdev:/tmp",
    )
    .await?;
    let session = Session::load(&session_id)?;
    assert!(session.is_debug);
    assert!(session.is_canary);

    abort_server_and_cleanup(&server_handle, &socket_path, &debug_socket_path);

    Ok(())
}

#[tokio::test]
async fn test_clear_preserves_debug_for_resumed_debug_session() -> Result<()> {
    let _env = setup_test_env()?;
    let runtime_dir = short_runtime_dir(format!(
        "jcode-clear-debug-test-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&runtime_dir)?;
    let socket_path = runtime_dir.join("jcode.sock");
    let debug_socket_path = runtime_dir.join("jcode-debug.sock");

    let provider = MockProvider::new();
    let provider: Arc<dyn jcode::provider::Provider> = Arc::new(provider);
    let server_instance =
        server::Server::new_with_paths(provider, socket_path.clone(), debug_socket_path.clone());
    let server_handle = tokio::spawn(async move { server_instance.run().await });

    wait_for_server_ready(&socket_path, &debug_socket_path).await?;

    let debug_session_id = debug_create_headless_session(debug_socket_path.clone()).await?;
    let mut client = server::Client::connect_with_path(socket_path.clone()).await?;
    subscribe_client(&mut client).await?;
    let resume_id = client.resume_session(&debug_session_id).await?;

    // Drain resume completion so clear() events are unambiguous.
    let mut saw_resume_history = false;
    let resume_deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < resume_deadline {
        let event = tokio::time::timeout(Duration::from_secs(1), client.read_event()).await??;
        match event {
            ServerEvent::Ack { .. } => continue,
            ServerEvent::History { id, .. } if id == resume_id => {
                saw_resume_history = true;
                break;
            }
            ServerEvent::Error { id, message, .. } if id == resume_id => {
                anyhow::bail!("resume_session failed: {}", message);
            }
            _ => {}
        }
    }
    if !saw_resume_history {
        anyhow::bail!("Timed out waiting for resume history event");
    }

    client.clear().await?;

    let mut new_session_id = None;
    let clear_deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < clear_deadline {
        let event = tokio::time::timeout(Duration::from_secs(1), client.read_event()).await??;
        match event {
            ServerEvent::Ack { .. } => continue,
            ServerEvent::SessionId { session_id } => {
                new_session_id = Some(session_id);
            }
            ServerEvent::Done { .. } if new_session_id.is_some() => break,
            _ => {}
        }
    }

    let new_session_id = new_session_id
        .ok_or_else(|| anyhow::anyhow!("Did not receive new session id after clear"))?;
    assert_ne!(new_session_id, debug_session_id);
    let session = Session::load(&new_session_id)?;
    assert!(session.is_debug);

    abort_server_and_cleanup(&server_handle, &socket_path, &debug_socket_path);

    Ok(())
}
