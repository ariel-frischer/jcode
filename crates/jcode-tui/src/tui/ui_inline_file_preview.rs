use ratatui::style::Style;
use ratatui::text::{Line, Span};

pub(super) fn render(preview: &crate::tui::InlineFilePreview) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "  ▾ Inline file · ",
                Style::default().fg(super::rgb(100, 190, 230)).bold(),
            ),
            Span::styled(
                preview.display_path.clone(),
                Style::default().fg(super::rgb(170, 210, 230)),
            ),
        ]),
    ];
    if preview.markdown {
        lines.extend(super::markdown::render_markdown(&preview.content));
    } else {
        lines.extend(preview.content.lines().map(|line| {
            Line::from(Span::styled(
                format!("  {line}"),
                Style::default().fg(super::dim_color()),
            ))
        }));
    }
    lines.push(Line::from(Span::styled(
        "  ────────────────────────────────────────",
        Style::default().fg(super::rgb(70, 95, 110)),
    )));
    lines
}
