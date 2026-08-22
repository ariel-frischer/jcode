#[tokio::test]
async fn prompt_overlays_are_agent_local() {
    let provider: Arc<dyn Provider> = Arc::new(DelayedProvider {
        open_delay: Duration::ZERO,
        first_event_delay: Duration::ZERO,
    });
    let registry_a = Registry::new(provider.clone()).await;
    let registry_b = Registry::new(provider.clone()).await;
    let mut agent_a = Agent::new(provider.clone(), registry_a);
    let mut agent_b = Agent::new(provider, registry_b);

    agent_a.set_session_prompt_overlay(crate::prompt::SessionPromptOverlay {
        instructions: Some("profile alpha instructions".to_string()),
        selected_skills: vec![("alpha".to_string(), "alpha skill prompt".to_string())],
    });
    agent_b.set_session_prompt_overlay(crate::prompt::SessionPromptOverlay {
        instructions: Some("profile beta instructions".to_string()),
        selected_skills: vec![("beta".to_string(), "beta skill prompt".to_string())],
    });

    let split_a = agent_a.build_system_prompt_split(None);
    let split_b = agent_b.build_system_prompt_split(None);
    let prompt_a = format!("{}\n\n{}", split_a.static_part, split_a.dynamic_part);
    let prompt_b = format!("{}\n\n{}", split_b.static_part, split_b.dynamic_part);
    assert!(prompt_a.contains("profile alpha instructions"));
    assert!(prompt_a.contains("alpha skill prompt"));
    assert!(!prompt_a.contains("profile beta instructions"));
    assert!(prompt_b.contains("profile beta instructions"));
    assert!(prompt_b.contains("beta skill prompt"));
    assert!(!prompt_b.contains("profile alpha instructions"));
}
