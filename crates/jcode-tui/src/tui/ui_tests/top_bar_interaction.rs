use super::*;

fn active_chrome_state() -> TestState {
    TestState {
        provider_name: Some("openai".to_string()),
        provider_model: Some("gpt-5.6-sol".to_string()),
        info_widget_data: info_widget::InfoWidgetData {
            session_name: Some("dolphin".to_string()),
            provider_name: Some("openai".to_string()),
            model: Some("gpt-5.6-sol".to_string()),
            reasoning_effort: Some("medium".to_string()),
            auth_method: info_widget::AuthMethod::OpenAIOAuth,
            usage_info: Some(info_widget::UsageInfo {
                provider: info_widget::UsageProvider::OpenAI,
                primary_limit_label: Some("5-hour".to_string()),
                five_hour: 0.38,
                available: true,
                ..Default::default()
            }),
            ..Default::default()
        },
        display_messages: vec![
            DisplayMessage::user("A prior prompt"),
            DisplayMessage::assistant(
                "The transcript remains visible below the chrome. ".repeat(30),
            ),
        ],
        chat_native_scrollbar: true,
        queued_messages: vec!["queued follow-up".to_string()],
        inline_view_state: Some(crate::tui::InlineViewState {
            title: "USAGE".to_string(),
            status: Some("refreshing".to_string()),
            lines: vec!["Refreshing usage".to_string()],
        }),
        input: "draft with\nmultiple lines".to_string(),
        cursor_pos: "draft with\nmultiple ".len(),
        status_notice: Some("provider usage is refreshing".to_string()),
        chat_overscroll_active: true,
        side_panel: crate::side_panel::SidePanelSnapshot {
            focused_page_id: Some("plan".to_string()),
            pages: vec![crate::side_panel::SidePanelPage {
                id: "plan".to_string(),
                title: "Plan".to_string(),
                file_path: "/tmp/plan.md".to_string(),
                format: crate::side_panel::SidePanelPageFormat::Markdown,
                source: crate::side_panel::SidePanelPageSource::Managed,
                content: "# Plan\n\nThe side pane remains separate from the top bar.".to_string(),
                updated_at_ms: 1,
            }],
        },
        top_bar_enabled: true,
        ..Default::default()
    }
}

fn render_state(state: &TestState, width: u16, height: u16) {
    let backend = ratatui::backend::TestBackend::new(width, height);
    let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| crate::tui::ui::draw(frame, state))
        .expect("draw");
}

fn assert_disjoint(first: ratatui::layout::Rect, second: ratatui::layout::Rect) {
    assert!(!crate::tui::ui::rects_overlap_for_tests(first, second));
}

#[test]
fn top_bar_surface_split_clamps_rows_without_mutating_content_width() {
    let area = ratatui::layout::Rect::new(3, 5, 80, 10);
    for requested_rows in [0, 1, 2, 3, 20] {
        let (top_bar, session) = super::super::split_top_bar_surface(area, requested_rows);
        let rows = requested_rows.min(area.height);
        assert_eq!(top_bar.map(|rect| rect.height).unwrap_or(0), rows);
        assert_eq!(session.x, area.x);
        assert_eq!(session.width, area.width);
        assert_eq!(session.y, area.y + rows);
        assert_eq!(session.height, area.height - rows);
        if let Some(top_bar) = top_bar {
            assert_disjoint(top_bar, session);
        }
    }
}

#[test]
fn active_chrome_keeps_top_bar_and_required_regions_distinct() {
    let _guard = crate::tui::ui::render_state_test_lock();
    for (width, height) in [(40, 12), (64, 16), (100, 24), (160, 40)] {
        render_state(&active_chrome_state(), width, height);
        let snapshot = crate::tui::ui::last_layout_snapshot().expect("layout snapshot");
        let status = crate::tui::ui::last_status_area().expect("status area");

        if let Some(top_bar) = snapshot.top_bar_area {
            assert_eq!(top_bar.y, 0);
            assert_eq!(top_bar.width, width);
            assert!(top_bar.height <= 3);
            assert_disjoint(top_bar, snapshot.messages_area);
            assert_disjoint(top_bar, status);
            if let Some(input) = snapshot.input_area {
                assert_disjoint(top_bar, input);
                assert!(input.height > 0);
            }
            if let Some(diagram) = snapshot.diagram_area {
                assert_disjoint(top_bar, diagram);
            }
            if let Some(diff) = snapshot.diff_pane_area {
                assert_disjoint(top_bar, diff);
            }
        }
        assert!(snapshot.messages_area.height > 0);
        assert!(snapshot.input_area.is_some());
    }
}

#[test]
fn disabled_top_bar_reclaims_space_even_with_active_chrome() {
    let _guard = crate::tui::ui::render_state_test_lock();
    let mut state = active_chrome_state();
    render_state(&state, 120, 32);
    let enabled = crate::tui::ui::last_layout_snapshot().expect("enabled layout");
    state.top_bar_enabled = false;
    render_state(&state, 120, 32);
    let disabled = crate::tui::ui::last_layout_snapshot().expect("disabled layout");

    assert!(enabled.top_bar_row_count > 0);
    assert_eq!(disabled.top_bar_row_count, 0);
    assert!(disabled.top_bar_area.is_none());
    assert_eq!(
        disabled.messages_area.y,
        enabled.messages_area.y - enabled.top_bar_row_count
    );
    assert!(disabled.input_area.is_some());
}
