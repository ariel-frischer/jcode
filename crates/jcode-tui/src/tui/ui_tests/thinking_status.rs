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
