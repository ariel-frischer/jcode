#[tokio::test]
async fn session_librarian_is_registered_and_respects_tool_filtering() {
    let provider: Arc<dyn Provider> = Arc::new(MockProvider);
    let registry = Registry::new(provider).await;

    assert!(
        registry
            .tool_names()
            .await
            .iter()
            .any(|name| name == "session_librarian"),
        "session_librarian must be exposed through the existing registry"
    );

    let excluded = registry.definitions(Some(&HashSet::new())).await;
    assert!(
        excluded
            .iter()
            .all(|definition| definition.name != "session_librarian"),
        "the existing allow-list must be able to exclude session_librarian"
    );

    let allowed = HashSet::from(["session_librarian".to_string()]);
    let included = registry.definitions(Some(&allowed)).await;
    assert_eq!(
        included
            .iter()
            .map(|definition| definition.name.as_str())
            .collect::<Vec<_>>(),
        vec!["session_librarian"]
    );
}
