use super::*;
use crate::tui::ui_top_bar::{TopBarFieldKind, TopBarLayout, TopBarSuppression, bounded_line};

#[allow(dead_code)]
#[derive(Clone, Copy)]
enum CreditFixture {
    KnownSubscription,
    KnownCost,
    Pending,
    Stale,
    Unavailable,
    NotApplicable,
}

#[allow(dead_code)]
fn active_session(credit: CreditFixture) -> TestState {
    let usage_info = match credit {
        CreditFixture::KnownSubscription | CreditFixture::Stale => Some(info_widget::UsageInfo {
            provider: info_widget::UsageProvider::OpenAI,
            primary_limit_label: Some("5-hour".to_string()),
            five_hour: 0.38,
            secondary_limit_label: Some("Weekly".to_string()),
            seven_day: 0.19,
            available: true,
            ..Default::default()
        }),
        CreditFixture::KnownCost => Some(info_widget::UsageInfo {
            provider: info_widget::UsageProvider::CostBased,
            total_cost: 0.0123,
            input_tokens: 12_000,
            output_tokens: 3_000,
            available: true,
            ..Default::default()
        }),
        CreditFixture::Unavailable => Some(info_widget::UsageInfo {
            provider: info_widget::UsageProvider::OpenAI,
            available: false,
            ..Default::default()
        }),
        CreditFixture::Pending | CreditFixture::NotApplicable => None,
    };

    TestState {
        provider_name: Some("openai".to_string()),
        provider_model: Some("gpt-5.6-sol".to_string()),
        working_dir: Some("/home/ari/repos/jcode".to_string()),
        info_widget_data: info_widget::InfoWidgetData {
            session_name: Some("dolphin".to_string()),
            provider_name: Some("openai".to_string()),
            model: Some("gpt-5.6-sol".to_string()),
            reasoning_effort: Some("medium".to_string()),
            auth_method: info_widget::AuthMethod::OpenAIOAuth,
            usage_info,
            ..Default::default()
        },
        display_messages: vec![
            DisplayMessage::user("Review the current implementation."),
            DisplayMessage::assistant("The transcript remains readable below the chrome."),
        ],
        input: "draft with\nmultiple lines".to_string(),
        cursor_pos: 10,
        top_bar_enabled: true,
        ..Default::default()
    }
}

#[allow(dead_code)]
fn render_state(state: &TestState, width: u16, height: u16) -> String {
    let backend = ratatui::backend::TestBackend::new(width, height);
    let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| crate::tui::ui::draw(frame, state))
        .expect("draw");
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn top_bar_fixture_covers_credit_states_and_terminal_sizes() {
    let states = [
        CreditFixture::KnownSubscription,
        CreditFixture::KnownCost,
        CreditFixture::Pending,
        CreditFixture::Stale,
        CreditFixture::Unavailable,
        CreditFixture::NotApplicable,
    ];
    let sizes = [(40, 12), (60, 16), (80, 24), (120, 32), (160, 48)];

    for state in states {
        for (width, height) in sizes {
            let rendered = render_state(&active_session(state), width, height);
            assert!(!rendered.is_empty());
        }
    }
}

#[test]
fn top_bar_layout_contract_keeps_rows_and_fields_within_their_bounds() {
    let layout = TopBarLayout {
        row_count: 2,
        lines: vec![
            bounded_line("◉ dolphin · OpenAI 62% left", 40),
            bounded_line("gpt-5.6-sol", 40),
        ],
        visible_fields: vec![TopBarFieldKind::Session, TopBarFieldKind::Credit],
        suppression_reason: None,
    };

    assert_eq!(layout.lines.len(), layout.row_count as usize);
    assert!(layout.row_count <= 3);
    assert_eq!(layout.visible_fields[0], TopBarFieldKind::Session);
    assert_eq!(layout.visible_fields[1], TopBarFieldKind::Credit);
    assert!(layout.lines.iter().all(|line| line.width() <= 40));
}

#[test]
fn top_bar_suppression_reserves_no_space_and_is_explicit() {
    for reason in [
        TopBarSuppression::Disabled,
        TopBarSuppression::TooNarrow,
        TopBarSuppression::TooShort,
        TopBarSuppression::NoSession,
    ] {
        let layout = TopBarLayout::suppressed(reason);
        assert_eq!(layout.row_count, 0);
        assert!(layout.lines.is_empty());
        assert!(layout.visible_fields.is_empty());
        assert_eq!(layout.suppression_reason, Some(reason));
    }
}

#[test]
fn top_bar_default_and_explicit_true_preferences_select_the_same_adaptive_layout() {
    let state = active_session(CreditFixture::KnownSubscription);
    let context = state.top_bar_context().expect("active session context");

    // The persisted setting's omitted value resolves to the documented true
    // default before it reaches the pure selector. Explicit true must follow
    // exactly the same path.
    let omitted = crate::tui::ui_top_bar::select_top_bar_layout(
        Some(&context),
        crate::config::DisplayConfig::default().top_bar,
        120,
        32,
        8,
    );
    let explicit_true =
        crate::tui::ui_top_bar::select_top_bar_layout(Some(&context), true, 120, 32, 8);

    assert!(omitted.row_count > 0);
    assert_eq!(omitted, explicit_true);
    assert_eq!(omitted.suppression_reason, None);
}

#[test]
fn rendered_top_bar_regions_do_not_overlap_chat_or_required_chrome() {
    let _guard = crate::tui::ui::render_state_test_lock();
    for (width, height) in [(40, 12), (60, 16), (80, 24), (120, 32), (160, 48)] {
        let state = active_session(CreditFixture::KnownSubscription);
        let _ = render_state(&state, width, height);
        let snapshot = crate::tui::ui::last_layout_snapshot().expect("layout snapshot");
        if let Some(top_bar) = snapshot.top_bar_area {
            assert_eq!(top_bar.y, 0);
            assert_eq!(top_bar.width, width);
            assert_eq!(top_bar.height, snapshot.top_bar_row_count);
            assert!(top_bar.y + top_bar.height <= snapshot.messages_area.y);
            assert!(!crate::tui::ui::rects_overlap_for_tests(
                top_bar,
                snapshot.messages_area
            ));
            if let Some(diagram) = snapshot.diagram_area {
                assert!(!crate::tui::ui::rects_overlap_for_tests(top_bar, diagram));
            }
            if let Some(diff) = snapshot.diff_pane_area {
                assert!(!crate::tui::ui::rects_overlap_for_tests(top_bar, diff));
            }
            if let Some(input) = snapshot.input_area {
                assert!(!crate::tui::ui::rects_overlap_for_tests(top_bar, input));
            }
        }
    }
}

#[test]
fn disabling_rendered_top_bar_reclaims_the_original_top_rows() {
    let _guard = crate::tui::ui::render_state_test_lock();
    let enabled = active_session(CreditFixture::KnownSubscription);
    let _ = render_state(&enabled, 120, 32);
    let enabled_layout = crate::tui::ui::last_layout_snapshot().expect("enabled layout");

    let mut disabled = enabled.clone();
    disabled.top_bar_enabled = false;
    let _ = render_state(&disabled, 120, 32);
    let disabled_layout = crate::tui::ui::last_layout_snapshot().expect("disabled layout");

    assert!(enabled_layout.top_bar_row_count > 0);
    assert_eq!(disabled_layout.top_bar_row_count, 0);
    assert!(disabled_layout.top_bar_area.is_none());
    assert_eq!(
        disabled_layout.messages_area.y,
        enabled_layout.messages_area.y - enabled_layout.top_bar_row_count
    );
}

#[test]
fn disabling_top_bar_in_a_very_small_terminal_reserves_no_rows_or_stale_snapshot() {
    let _guard = crate::tui::ui::render_state_test_lock();
    let mut state = active_session(CreditFixture::KnownSubscription);
    state.top_bar_enabled = true;
    let _ = render_state(&state, 40, 12);
    let enabled_layout = crate::tui::ui::last_layout_snapshot().expect("enabled layout");
    assert!(enabled_layout.top_bar_row_count > 0);

    state.top_bar_enabled = false;
    let _ = render_state(&state, 24, 8);
    let disabled_layout = crate::tui::ui::last_layout_snapshot().expect("disabled layout");

    assert_eq!(disabled_layout.top_bar_row_count, 0);
    assert!(disabled_layout.top_bar_area.is_none());
    assert_eq!(disabled_layout.messages_area.y, 0);
    assert!(disabled_layout.input_area.is_some());
}
