use super::*;

#[test]
fn thinking_status_distinguishes_phase_from_fifteen_minute_turn() {
    let _lock = viewport_snapshot_test_lock();
    let state = TestState {
        status: ProcessingStatus::Thinking(Instant::now() - Duration::from_secs(2)),
        elapsed: Some(Duration::from_secs(15 * 60)),
        time_since_activity: Some(Duration::from_secs(1)),
        ..Default::default()
    };
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 1)).expect("test terminal");
    terminal
        .draw(|frame| input_ui::draw_status(frame, &state, frame.area(), 0))
        .expect("render thinking status");
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains("thinking… 2s · turn"), "{text}");
    assert!(text.contains("turn 15m"), "{text}");
    assert!(!text.contains("thinking… 15m"), "{text}");
}

#[test]
fn connection_status_distinguishes_attempt_from_hour_long_turn() {
    let _lock = viewport_snapshot_test_lock();
    let state = TestState {
        provider_name: Some("OpenAI".to_string()),
        status: ProcessingStatus::Connecting(crate::message::ConnectionPhase::Connecting),
        elapsed: Some(Duration::from_secs(71 * 60)),
        connection_phase_elapsed: Some(Duration::from_secs(2)),
        ..Default::default()
    };
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 1)).expect("test terminal");
    terminal
        .draw(|frame| input_ui::draw_status(frame, &state, frame.area(), 0))
        .expect("render connection status");
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains("2s · turn 1h 11m"), "{text}");
    assert!(text.contains("OpenAI: connecting"), "{text}");
    assert!(!text.contains("… 1h 11m"), "{text}");
}

#[test]
fn running_bash_status_does_not_claim_provider_connection_activity() {
    let _lock = viewport_snapshot_test_lock();
    let state = TestState {
        status: ProcessingStatus::RunningTool("bash".to_string()),
        status_detail: Some("fresh websocket".to_string()),
        connection_type: Some("websocket/persistent-fresh".to_string()),
        elapsed: Some(Duration::from_secs(71 * 60)),
        ..Default::default()
    };
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 1)).expect("test terminal");
    terminal
        .draw(|frame| input_ui::draw_status(frame, &state, frame.area(), 0))
        .expect("render tool status");
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains("bash"), "{text}");
    assert!(text.contains("turn 1h 11m"), "{text}");
    assert!(!text.contains("websocket"), "{text}");
}

#[test]
fn thinking_status_does_not_repeat_finished_connection_activity() {
    let _lock = viewport_snapshot_test_lock();
    let state = TestState {
        status: ProcessingStatus::Thinking(Instant::now()),
        status_detail: Some("fresh websocket".to_string()),
        connection_type: Some("websocket/persistent-fresh".to_string()),
        ..Default::default()
    };
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 1)).expect("test terminal");
    terminal
        .draw(|frame| input_ui::draw_status(frame, &state, frame.area(), 0))
        .expect("render thinking status");
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains("thinking"), "{text}");
    assert!(!text.contains("opening websocket"), "{text}");
}

#[test]
fn thinking_status_keeps_non_connection_warnings_visible() {
    let _lock = viewport_snapshot_test_lock();
    let state = TestState {
        status: ProcessingStatus::Thinking(Instant::now()),
        status_detail: Some("No provider activity for 60s".to_string()),
        ..Default::default()
    };
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 1)).expect("test terminal");
    terminal
        .draw(|frame| input_ui::draw_status(frame, &state, frame.area(), 0))
        .expect("render thinking warning");
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains("No provider activity for 60s"), "{text}");
}
