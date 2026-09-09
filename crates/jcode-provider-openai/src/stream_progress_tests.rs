#[test]
fn parser_progress_sink_observes_buffered_tool_deltas_not_control_frames() {
    let progress_at = Arc::new(Mutex::new(Instant::now() - Duration::from_secs(1)));
    let mut stream = OpenAIResponsesStream::new_with_progress(
        futures::stream::empty::<Result<Bytes, reqwest::Error>>(),
        Arc::clone(&progress_at),
    );

    stream.buffer =
            "data: {\"type\":\"response.function_call_arguments.delta\",\"delta\":\"{\\\"path\\\":\\\"file\\\"}\"}\n\n"
                .to_string();
    let before_tool_delta = *progress_at.lock().expect("progress timestamp");
    assert!(stream.parse_next_event().is_none());
    assert!(
        *progress_at.lock().expect("progress timestamp") > before_tool_delta,
        "buffered tool argument deltas must count as provider progress"
    );

    let before_control = *progress_at.lock().expect("progress timestamp");
    stream.buffer = "data: {\"type\":\"response.in_progress\"}\n\n".to_string();
    assert!(stream.parse_next_event().is_none());
    assert_eq!(
        *progress_at.lock().expect("progress timestamp"),
        before_control,
        "lifecycle frames must not reset the meaningful-progress deadline"
    );

    stream.buffer = ": keepalive\n\n".to_string();
    assert!(stream.parse_next_event().is_none());
    assert_eq!(
        *progress_at.lock().expect("progress timestamp"),
        before_control,
        "SSE comments must not reset the meaningful-progress deadline"
    );
}
