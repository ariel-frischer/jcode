#[test]
fn test_agents_review_picker_saves_config_override() {
    with_temp_jcode_home(|| {
        let mut app = create_test_app();
        configure_test_remote_models(&mut app);
        app.open_agent_model_picker(crate::tui::AgentModelTarget::Review);

        let selected = app
            .inline_interactive_state
            .as_ref()
            .and_then(|picker| {
                picker.filtered.iter().position(|&idx| {
                    matches!(
                        picker.entries[idx].action,
                        crate::tui::PickerAction::AgentModelChoice {
                            target: crate::tui::AgentModelTarget::Review,
                            clear_override: false,
                        }
                    )
                })
            })
            .expect("review picker should include at least one model option");
        app.inline_interactive_state.as_mut().unwrap().selected = selected;
        let selected_model_idx = app.inline_interactive_state.as_ref().unwrap().filtered[selected];
        app.inline_interactive_state.as_mut().unwrap().entries[selected_model_idx].options[0]
            .available = true;

        let expected = {
            let picker = app.inline_interactive_state.as_ref().unwrap();
            let entry = &picker.entries[picker.filtered[selected]];
            let base = if entry.effort.is_some() {
                entry
                    .name
                    .rsplit_once(" (")
                    .map(|(base, _)| base.to_string())
                    .unwrap_or_else(|| entry.name.clone())
            } else {
                entry.name.clone()
            };
            let route = &entry.options[entry.selected_option];
            if route.api_method == "copilot" {
                format!("copilot:{}", base)
            } else if route.api_method == "cursor" {
                format!("cursor:{}", base)
            } else if route.api_method == "openai-oauth" {
                format!("openai-oauth:{}", base)
            } else if route.api_method == "openai-api" {
                format!("openai-api:{}", base)
            } else if route.api_method == "claude-oauth" {
                format!("claude-oauth:{}", base)
            } else if route.api_method == "claude-api" && route.provider == "Anthropic" {
                format!("claude-api:{}", base)
            } else if route.api_method == "bedrock" {
                format!("bedrock:{}", base)
            } else if route.api_method == "openrouter" && route.provider != "auto" {
                let catalog_model = crate::provider::openrouter_catalog_model_id(&base)
                    .unwrap_or_else(|| base.clone());
                format!("{}@{}", catalog_model, route.provider)
            } else {
                base
            }
        };

        app.handle_inline_interactive_key(KeyCode::Enter, KeyModifiers::NONE)
            .expect("save agent model override");

        let cfg = crate::config::Config::load();
        assert_eq!(cfg.autoreview.model.as_deref(), Some(expected.as_str()));
        assert!(app.inline_interactive_state.is_none());
    });
}

#[test]
fn test_model_command_suggestions_include_matching_models() {
    let mut app = create_test_app();
    configure_test_remote_models(&mut app);

    let suggestions = app.get_suggestions_for("/model g52c");
    assert_eq!(
        suggestions.first().map(|(cmd, _)| cmd.as_str()),
        Some("/model gpt-5.2-codex")
    );
}

#[test]
fn test_model_command_trailing_space_shows_model_suggestions() {
    let mut app = create_test_app();
    configure_test_remote_models(&mut app);

    let suggestions = app.get_suggestions_for("/model ");
    assert!(
        suggestions
            .iter()
            .any(|(cmd, _)| cmd == "/model gpt-5.3-codex")
    );
}

#[test]
fn test_model_command_provider_suggestions_include_openrouter_routes() {
    let mut app = create_test_app();
    configure_test_remote_openrouter_provider_routes(&mut app);

    let suggestions = app.get_suggestions_for("/model anthropic/claude-sonnet-4@");
    let commands: Vec<&str> = suggestions.iter().map(|(cmd, _)| cmd.as_str()).collect();

    assert!(commands.contains(&"/model anthropic/claude-sonnet-4@auto"));
    assert!(commands.contains(&"/model anthropic/claude-sonnet-4@Fireworks"));
    assert!(commands.contains(&"/model anthropic/claude-sonnet-4@OpenAI"));
}

#[test]
fn test_model_command_provider_suggestions_rank_matching_provider_prefix() {
    let mut app = create_test_app();
    configure_test_remote_openrouter_provider_routes(&mut app);

    let suggestions = app.get_suggestions_for("/model anthropic/claude-sonnet-4@fi");
    assert_eq!(
        suggestions.first().map(|(cmd, _)| cmd.as_str()),
        Some("/model anthropic/claude-sonnet-4@Fireworks")
    );
}

#[test]
fn test_model_command_provider_suggestions_normalize_bare_openai_model_to_openrouter_catalog_id() {
    let (app, _set_model_calls) = create_openrouter_spec_capture_test_app();

    let suggestions = app.get_suggestions_for("/model gpt-5.4@op");
    assert_eq!(
        suggestions.first().map(|(cmd, _)| cmd.as_str()),
        Some("/model openai/gpt-5.4@OpenAI")
    );
}

#[test]
fn test_model_command_provider_suggestions_include_auto_for_normalized_bare_openai_model() {
    let (app, _set_model_calls) = create_openrouter_spec_capture_test_app();

    let suggestions = app.get_suggestions_for("/model gpt-5.4@");
    let commands: Vec<&str> = suggestions.iter().map(|(cmd, _)| cmd.as_str()).collect();

    assert!(commands.contains(&"/model openai/gpt-5.4@auto"));
    assert!(commands.contains(&"/model openai/gpt-5.4@OpenAI"));
}

#[test]
fn test_remote_fallback_provider_suggestions_normalize_bare_openai_openrouter_routes() {
    with_temp_jcode_home(|| {
        let prev_api_key = std::env::var_os("OPENROUTER_API_KEY");
        crate::env::set_var("OPENROUTER_API_KEY", "test-openrouter-key");
        crate::auth::AuthStatus::invalidate_cache();

        let mut app = create_test_app();
        app.is_remote = true;
        app.remote_provider_model = Some("gpt-5.4".to_string());
        app.remote_available_entries = vec!["gpt-5.4".to_string()];
        app.remote_model_options.clear();

        let suggestions = app.get_suggestions_for("/model gpt-5.4@");
        let commands: Vec<&str> = suggestions.iter().map(|(cmd, _)| cmd.as_str()).collect();

        assert!(commands.contains(&"/model openai/gpt-5.4@auto"));
        assert!(commands.contains(&"/model openai/gpt-5.4@OpenAI"));

        if let Some(prev_api_key) = prev_api_key {
            crate::env::set_var("OPENROUTER_API_KEY", prev_api_key);
        } else {
            crate::env::remove_var("OPENROUTER_API_KEY");
        }
        crate::auth::AuthStatus::invalidate_cache();
    });
}

#[test]
fn test_login_command_suggestions_follow_provider_catalog() {
    let app = create_test_app();
    let suggestions = app.get_suggestions_for("/login ");

    for provider in crate::provider_catalog::tui_login_providers() {
        assert!(
            suggestions
                .iter()
                .any(|(cmd, detail)| cmd == &format!("/login {}", provider.id)
                    && detail == &provider.menu_detail),
            "missing /login suggestion for provider {}",
            provider.id
        );
    }
}

#[test]
fn test_model_autocomplete_completes_unique_match() {
    let mut app = create_test_app();
    configure_test_remote_models(&mut app);
    app.input = "/model g52c".to_string();
    app.cursor_pos = app.input.len();

    assert!(app.autocomplete());
    assert_eq!(app.input(), "/model gpt-5.2-codex");
}

#[test]
fn test_model_autocomplete_completes_unique_provider_match() {
    let mut app = create_test_app();
    configure_test_remote_openrouter_provider_routes(&mut app);

    app.input = "/model anthropic/claude-sonnet-4@fi".to_string();
    app.cursor_pos = app.input.len();

    assert!(app.autocomplete());
    assert_eq!(app.input(), "/model anthropic/claude-sonnet-4@Fireworks");
}

#[test]
fn test_model_picker_preview_stays_open_and_updates_filter() {
    let mut app = create_test_app();
    configure_test_remote_models(&mut app);

    for c in "/model g52c".chars() {
        app.handle_key(KeyCode::Char(c), KeyModifiers::empty())
            .unwrap();
    }

    let picker = app
        .inline_interactive_state
        .as_ref()
        .expect("model picker preview should be open");
    assert!(picker.preview);
    assert_eq!(picker.filter, "g52c");
    assert!(
        picker
            .filtered
            .iter()
            .any(|&i| picker.entries[i].name.starts_with("gpt-5.2-codex ("))
    );
    assert_eq!(app.input(), "/model g52c");
}

#[test]
fn test_model_picker_cold_preview_immediately_filters_sol_medium() {
    ensure_test_jcode_home_if_unset();
    clear_persisted_test_ui_state();
    crate::tui::ui::clear_test_render_state_for_tests();

    let provider: Arc<dyn Provider> = Arc::new(MixedModelRoutesProvider {
        model: StdArc::new(StdMutex::new("gpt-5.5".to_string())),
    });
    let rt = tokio::runtime::Runtime::new().unwrap();
    let registry = rt.block_on(crate::tool::Registry::new(provider.clone()));
    let mut app = App::new_for_test_harness(provider, registry);
    app.queue_mode = false;
    app.diff_mode = crate::config::DiffDisplayMode::Inline;

    for c in "/model sol (med)".chars() {
        app.handle_key(KeyCode::Char(c), KeyModifiers::empty())
            .unwrap();
    }

    let picker = app
        .inline_interactive_state
        .as_ref()
        .expect("model picker preview should be open");
    assert_eq!(picker.filter, "sol (med)");
    assert!(
        picker
            .filtered
            .iter()
            .any(|&i| picker.entries[i].name == "gpt-5.6-sol (med)"),
        "the complete Sol effort rows must exist before the first filtered paint"
    );
    assert!(app.pending_model_picker_load.is_none());
}

#[test]
fn test_remote_large_catalog_cold_preview_immediately_filters_sol_medium() {
    let mut app = create_test_app();
    app.is_remote = true;
    app.remote_provider_name = Some("OpenAI".to_string());
    app.remote_provider_model = Some("gpt-5.6-luna".to_string());
    app.remote_available_entries = (0..65)
        .map(|idx| format!("catalog-model-{idx}"))
        .chain(std::iter::once("gpt-5.6-sol".to_string()))
        .collect();
    app.remote_model_options.clear();

    for c in "/model sol (med)".chars() {
        app.handle_key(KeyCode::Char(c), KeyModifiers::empty())
            .unwrap();
    }

    let loading_picker = app
        .inline_interactive_state
        .as_ref()
        .expect("remote model picker loading shell should open immediately");
    assert!(loading_picker.preview);
    assert_eq!(app.input(), "/model sol (med)");
    assert!(
        app.pending_model_picker_load.is_some(),
        "large remote catalog classification must not block input handling"
    );

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while app.pending_model_picker_load.is_some() && std::time::Instant::now() < deadline {
        app.poll_model_picker_load();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    let picker = app
        .inline_interactive_state
        .as_ref()
        .expect("completed remote model picker preview should be open");
    assert_eq!(picker.filter, "sol (med)");
    assert!(
        picker
            .filtered
            .iter()
            .any(|&i| picker.entries[i].name == "gpt-5.6-sol (med)"),
        "large remote catalogs must expose complete Sol effort rows on the first paint"
    );
    assert!(app.pending_model_picker_load.is_none());

    let selected_entry = picker.filtered[picker.selected];
    assert_eq!(
        picker.entries[selected_entry].name, "gpt-5.6-sol (med)",
        "the exact filtered row must be selected instead of xhigh"
    );

    app.handle_key(KeyCode::Enter, KeyModifiers::empty())
        .unwrap();
    assert_eq!(
        app.pending_reasoning_effort.as_deref(),
        Some("medium"),
        "selecting `sol (med)` must stage medium effort"
    );
}

#[test]
fn test_model_picker_preview_enter_selects_model() {
    let mut app = create_test_app();
    configure_test_remote_models(&mut app);

    for c in "/model g52c".chars() {
        app.handle_key(KeyCode::Char(c), KeyModifiers::empty())
            .unwrap();
    }
    app.handle_key(KeyCode::Enter, KeyModifiers::empty())
        .unwrap();

    // Enter from preview mode selects the model and closes the picker
    assert!(app.inline_interactive_state.is_none());
    assert!(app.input().is_empty());
    assert_eq!(app.cursor_pos(), 0);
}
