use crate::message::ToolCall;
use ratatui::text::Line;

pub(super) fn result_summary(
    tool_call: &ToolCall,
    result_title: Option<&str>,
    width: usize,
) -> String {
    if let Some(title) = result_title
        .map(str::trim)
        .filter(|title| !title.is_empty())
    {
        super::line_plain_text(&super::truncate_line_with_ellipsis_to_width(
            &Line::from(title.to_string()),
            width,
        ))
    } else {
        super::tools_ui::get_tool_summary_with_budget(tool_call, 50, Some(width))
    }
}
