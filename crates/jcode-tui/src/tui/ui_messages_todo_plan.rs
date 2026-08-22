use super::*;

pub(super) fn wrap_todo_detail(value: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut chunks = Vec::new();
    let mut current = String::new();

    for word in value.split_whitespace() {
        let word_width = word.width();
        if !current.is_empty() && current.width() + 1 + word_width <= width {
            current.push(' ');
            current.push_str(word);
            continue;
        }
        if current.is_empty() && word_width <= width {
            current.push_str(word);
            continue;
        }
        if !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
        }
        if word_width <= width {
            current.push_str(word);
            continue;
        }
        let mut word_chunks = split_by_display_width(word, width).into_iter().peekable();
        while let Some(chunk) = word_chunks.next() {
            if word_chunks.peek().is_some() {
                chunks.push(chunk);
            } else {
                current = chunk;
            }
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// Plan-level assessment lines shown once above the todo groups.
pub(super) fn push_todo_plan_details(
    lines: &mut Vec<Line<'static>>,
    plan: &crate::todo::TodoPlan,
    base_indent: &str,
    inner_width: usize,
    compact_details: bool,
) {
    let intention = plan
        .user_intention
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(state) = plan.understands_user_intent {
        let state_color = match state {
            crate::todo::IntentUnderstanding::Uncertain => todo_failure_color(),
            crate::todo::IntentUnderstanding::Partial => todo_warning_color(),
            crate::todo::IntentUnderstanding::Clear
            | crate::todo::IntentUnderstanding::Complete => todo_score_color(),
        };
        if let Some(intention) = intention {
            let prefix_width = "Intent ".width() + state.as_str().width() + 2;
            if compact_details {
                lines.push(todo_card_line(
                    vec![
                        Span::styled("Intent ", Style::default().fg(todo_label_color())),
                        Span::styled(state.as_str().to_string(), Style::default().fg(state_color)),
                        Span::styled(": ", Style::default().fg(todo_label_color())),
                        Span::styled(
                            intention.to_string(),
                            Style::default().fg(todo_meta_color()),
                        ),
                    ],
                    base_indent,
                    inner_width,
                ));
            } else {
                let available = inner_width.saturating_sub(prefix_width).max(1);
                for (index, chunk) in wrap_todo_detail(intention, available)
                    .into_iter()
                    .enumerate()
                {
                    let mut spans = if index == 0 {
                        vec![
                            Span::styled("Intent ", Style::default().fg(todo_label_color())),
                            Span::styled(
                                state.as_str().to_string(),
                                Style::default().fg(state_color),
                            ),
                            Span::styled(": ", Style::default().fg(todo_label_color())),
                        ]
                    } else {
                        vec![Span::raw(" ".repeat(prefix_width))]
                    };
                    spans.push(Span::styled(chunk, Style::default().fg(todo_meta_color())));
                    lines.push(todo_card_line(spans, base_indent, inner_width));
                }
            }
        } else {
            lines.push(todo_card_line(
                vec![
                    Span::styled("Intent ", Style::default().fg(todo_label_color())),
                    Span::styled(state.as_str().to_string(), Style::default().fg(state_color)),
                ],
                base_indent,
                inner_width,
            ));
        }
    } else if let Some(intention) = intention {
        push_todo_detail(
            lines,
            "Intent",
            intention,
            base_indent,
            inner_width,
            compact_details,
        );
    }
}
