use crate::test_support::*;

#[test]
fn complete_profile_resolves_for_plain_json_and_ndjson_runs() -> Result<()> {
    let _env = setup_test_env()?;
    let config_path = jcode::config::Config::path().expect("config path");
    std::fs::create_dir_all(config_path.parent().expect("config parent"))?;
    std::fs::write(
        &config_path,
        r#"
[profiles.review]
provider = "openrouter"
model = "openai/gpt-5.6"
reasoning_effort = "high"
tool_profile = "minimal"
tools = ["read", "agentgrep"]
disabled_tools = ["bash"]
"#,
    )?;

    for output_mode in ["plain", "json", "ndjson"] {
        let resolved = jcode::cli::profile::resolve_run_profile(
            Some("review"),
            jcode::cli::profile::RunProfileOverrides::default(),
        )?
        .expect("selected profile should resolve");

        assert_eq!(resolved.name, "review", "mode={output_mode}");
        assert_eq!(
            resolved.provider.as_arg_value(),
            "openrouter",
            "mode={output_mode}"
        );
        assert_eq!(resolved.model.as_deref(), Some("openai/gpt-5.6"));
        assert_eq!(resolved.reasoning_effort.as_deref(), Some("high"));
        let selection = resolved.tools.selection();
        assert_eq!(
            selection.allowed_tools,
            Some(std::collections::HashSet::from([
                "read".to_string(),
                "agentgrep".to_string(),
            ]))
        );
        assert!(selection.disabled_tools.contains("bash"));
    }
    Ok(())
}

#[test]
fn explicit_run_overrides_win_without_discarding_unset_profile_fields() -> Result<()> {
    let _env = setup_test_env()?;
    let config_path = jcode::config::Config::path().expect("config path");
    std::fs::create_dir_all(config_path.parent().expect("config parent"))?;
    std::fs::write(
        &config_path,
        r#"
[profiles.review]
provider = "openrouter"
model = "profile-model"
reasoning_effort = "medium"
provider_profile = "profile-gateway"
tool_profile = "minimal"
tools = ["read"]
disabled_tools = ["bash"]
instructions = "Keep this profile-only instruction."
"#,
    )?;

    let resolved = jcode::cli::profile::resolve_run_profile(
        Some("review"),
        jcode::cli::profile::RunProfileOverrides {
            provider: Some(jcode::cli::provider_init::ProviderChoice::Auto),
            model: Some("invocation-model".to_string()),
            reasoning_effort: Some("high".to_string()),
            provider_profile: Some("invocation-gateway".to_string()),
            tool_profile: Some("none".to_string()),
            tools: Some(vec!["agentgrep".to_string()]),
            disabled_tools: Some(vec!["write".to_string()]),
        },
    )?
    .expect("selected profile");

    assert_eq!(resolved.provider.as_arg_value(), "auto");
    assert_eq!(resolved.model.as_deref(), Some("invocation-model"));
    assert_eq!(resolved.reasoning_effort.as_deref(), Some("high"));
    assert_eq!(
        resolved.provider_profile.as_deref(),
        Some("invocation-gateway")
    );
    assert_eq!(resolved.tools.profile, "none");
    assert_eq!(resolved.tools.enabled, ["agentgrep"]);
    assert_eq!(resolved.tools.disabled, ["write"]);
    assert_eq!(
        resolved.instructions.as_deref(),
        Some("Keep this profile-only instruction.")
    );
    Ok(())
}

#[test]
fn environment_overrides_win_when_invocation_omits_the_field() -> Result<()> {
    let _env = setup_test_env()?;
    let config_path = jcode::config::Config::path().expect("config path");
    std::fs::create_dir_all(config_path.parent().expect("config parent"))?;
    std::fs::write(
        &config_path,
        r#"
[profiles.review]
provider = "openrouter"
model = "profile-model"
reasoning_effort = "medium"
tool_profile = "minimal"
"#,
    )?;
    jcode::env::set_var("JCODE_PROVIDER", "openai");
    jcode::env::set_var("JCODE_MODEL", "environment-model");
    jcode::env::set_var("JCODE_OPENAI_REASONING_EFFORT", "high");
    jcode::env::set_var("JCODE_TOOL_PROFILE", "none");

    let resolved = jcode::cli::profile::resolve_run_profile(
        Some("review"),
        jcode::cli::profile::RunProfileOverrides::default(),
    )?
    .expect("selected profile");

    assert_eq!(resolved.provider.as_arg_value(), "openai");
    assert_eq!(resolved.model.as_deref(), Some("environment-model"));
    assert_eq!(resolved.reasoning_effort.as_deref(), Some("high"));
    assert_eq!(resolved.tools.profile, "none");
    Ok(())
}

#[test]
fn selected_profile_prompt_context_is_ordered_isolated_and_read_only() -> Result<()> {
    let _env = setup_test_env()?;
    let config_path = jcode::config::Config::path().expect("config path");
    let jcode_home = config_path.parent().expect("config parent");
    std::fs::create_dir_all(jcode_home)?;
    for (name, body) in [
        ("alpha-skill", "alpha selected skill content"),
        ("beta-skill", "beta selected skill content"),
    ] {
        let skill_dir = jcode_home.join("skills").join(name);
        std::fs::create_dir_all(&skill_dir)?;
        std::fs::write(
            skill_dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: test skill\n---\n{body}\n"),
        )?;
    }
    let config_bytes = br#"
[profiles.alpha]
instructions = "alpha profile instructions"
skills = ["alpha-skill"]

[profiles.beta]
instructions = "beta profile instructions"
skills = ["beta-skill"]
"#;
    std::fs::write(&config_path, config_bytes)?;

    let alpha = jcode::cli::profile::resolve_run_profile(
        Some("alpha"),
        jcode::cli::profile::RunProfileOverrides::default(),
    )?
    .expect("alpha profile");
    let beta = jcode::cli::profile::resolve_run_profile(
        Some("beta"),
        jcode::cli::profile::RunProfileOverrides::default(),
    )?
    .expect("beta profile");

    let mut alpha_prompt = jcode::prompt::SplitSystemPrompt::default();
    alpha.prompt_overlay.append_to_split(&mut alpha_prompt);
    let mut beta_prompt = jcode::prompt::SplitSystemPrompt::default();
    beta.prompt_overlay.append_to_split(&mut beta_prompt);
    assert!(
        alpha_prompt
            .static_part
            .find("alpha profile instructions")
            .unwrap()
            < alpha_prompt
                .static_part
                .find("alpha selected skill content")
                .unwrap()
    );
    assert!(
        !alpha_prompt
            .static_part
            .contains("beta profile instructions")
    );
    assert!(
        beta_prompt
            .static_part
            .contains("beta profile instructions")
    );
    assert!(
        !beta_prompt
            .static_part
            .contains("alpha profile instructions")
    );
    assert!(
        !alpha_prompt
            .dynamic_part
            .contains("alpha profile instructions")
    );
    assert!(
        !beta_prompt
            .dynamic_part
            .contains("beta profile instructions")
    );
    assert_eq!(std::fs::read(&config_path)?, config_bytes);
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
