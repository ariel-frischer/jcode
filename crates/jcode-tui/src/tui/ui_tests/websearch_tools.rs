use super::*;

#[test]
fn test_websearch_prefers_selected_engine_title_without_query_noise() {
    let msg = DisplayMessage {
        role: "tool".to_string(),
        content:
            "Search results for: private fixture query\n\n1. **Result**\n   https://example.test\n"
                .to_string(),
        tool_calls: vec![],
        duration_secs: None,
        title: Some("bing".to_string()),
        tool_data: Some(ToolCall {
            id: "websearch-title".to_string(),
            name: "websearch".to_string(),
            input: serde_json::json!({"query": "private fixture query"}),
            intent: None,
            thought_signature: None,
        }),
    };

    let lines = render_tool_message(&msg, 120, crate::config::DiffDisplayMode::Off);
    let rendered: Vec<String> = lines.iter().map(extract_line_text).collect();
    assert_eq!(
        rendered.len(),
        1,
        "clean search should remain one compact row"
    );
    assert!(rendered[0].contains("websearch"));
    assert!(rendered[0].contains("bing"));
    assert!(!rendered[0].contains("private fixture query"));
}

#[test]
fn test_websearch_activity_title_is_bounded_and_deterministic() {
    let title = "websearch: exhausted (attempts 6, retries 3, skipped 1)";
    let msg = DisplayMessage {
        role: "tool".to_string(),
        content: "All eligible websearch engines failed.".to_string(),
        tool_calls: vec![],
        duration_secs: None,
        title: Some(title.to_string()),
        tool_data: Some(ToolCall {
            id: "websearch-exhausted".to_string(),
            name: "websearch".to_string(),
            input: serde_json::json!({"query": "secret query"}),
            intent: None,
            thought_signature: None,
        }),
    };

    let first = render_tool_message(&msg, 96, crate::config::DiffDisplayMode::Off);
    let second = render_tool_message(&msg, 96, crate::config::DiffDisplayMode::Off);
    let first_text: Vec<String> = first.iter().map(extract_line_text).collect();
    let second_text: Vec<String> = second.iter().map(extract_line_text).collect();
    assert_eq!(first_text, second_text);
    assert!(
        first_text
            .iter()
            .all(|line| unicode_width::UnicodeWidthStr::width(line.as_str()) <= 96)
    );
    assert!(first_text.iter().all(|line| !line.contains("secret query")));
}
