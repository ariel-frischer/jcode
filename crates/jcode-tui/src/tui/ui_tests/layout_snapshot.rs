#[cfg(test)]
pub fn record_layout_snapshot(
    messages_area: Rect,
    diagram_area: Option<Rect>,
    diff_pane_area: Option<Rect>,
    input_area: Option<Rect>,
) {
    record_layout_snapshot_with_top_bar(
        messages_area,
        diagram_area,
        diff_pane_area,
        input_area,
        None,
        0,
    );
}
