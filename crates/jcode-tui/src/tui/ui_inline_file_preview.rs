use ratatui::style::Style;
use ratatui::text::{Line, Span};

pub(crate) const INLINE_FILE_PREVIEW_HEADER_PREFIX: &str = "▾ Inline file · ";

pub(super) fn render(preview: &crate::tui::InlineFilePreview) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(
                format!("  {INLINE_FILE_PREVIEW_HEADER_PREFIX}"),
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
        lines.extend(
            preview
                .content
                .lines()
                .map(|line| Line::from(Span::styled(format!("  {line}"), Style::default()))),
        );
    }
    lines.push(Line::from(Span::styled(
        "  ────────────────────────────────────────",
        Style::default().fg(super::rgb(70, 95, 110)),
    )));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Modifier;

    #[test]
    fn ordinary_file_content_uses_normal_foreground() {
        let preview = crate::tui::InlineFilePreview {
            display_path: "src/main.rs".to_string(),
            content: "fn main() {}\n🙂 unicode".to_string(),
            markdown: false,
        };

        let lines = render(&preview);
        let first_content = &lines[2].spans[0];
        assert_eq!(first_content.content.as_ref(), "  fn main() {}");
        assert_eq!(first_content.style.fg, None);
        assert!(!first_content.style.add_modifier.contains(Modifier::DIM));
        assert_eq!(lines[3].spans[0].content.as_ref(), "  🙂 unicode");
    }

    #[test]
    fn ordinary_empty_file_renders_without_dim_content() {
        let preview = crate::tui::InlineFilePreview {
            display_path: "empty.txt".to_string(),
            content: String::new(),
            markdown: false,
        };

        let lines = render(&preview);
        assert_eq!(lines.len(), 3, "header, spacer, and divider remain stable");
    }
}
