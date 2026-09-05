//! Bounded presentation of server-owned workflow snapshots.
use crate::bus::{WorkflowHealth, WorkflowSnapshot};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

pub(super) fn draw(
    frame: &mut Frame,
    area: Rect,
    workflows: &[WorkflowSnapshot],
    enabled: bool,
    max_visible: usize,
) -> Rect {
    // Keep at least eight rows for transcript/input, and never take more than
    // half the remaining surface. No panel is preferable to hiding user input.
    let budget = area.height.saturating_sub(8).min(area.height / 2);
    let visible = workflows
        .len()
        .min(max_visible.min(8))
        .min(budget.saturating_sub(2) as usize / 3);
    if !enabled || visible == 0 || area.width < 24 {
        return area;
    }
    let height = (visible * 3 + 2) as u16;
    let panel = Rect::new(area.x, area.y, area.width, height);
    let hidden = workflows.len() - visible;
    let title = if hidden == 0 {
        " Workflows ".into()
    } else {
        format!(" Workflows (+{hidden} more) ")
    };
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(panel);
    frame.render_widget(block, panel);
    // Keep explicit failures visible ahead of ordinary running work, even
    // when the bounded panel cannot show every retained snapshot.
    let urgent = |s: &&WorkflowSnapshot| {
        matches!(
            s.health,
            WorkflowHealth::Failed | WorkflowHealth::Blocked | WorkflowHealth::ObserverError
        )
    };
    let ordered = workflows
        .iter()
        .filter(urgent)
        .chain(workflows.iter().filter(|s| !urgent(s)));
    let mut lines = Vec::with_capacity(visible * 3);
    for snapshot in ordered.take(visible) {
        let (health, color) = health_style(snapshot.health);
        let counts = match (snapshot.completed, snapshot.total) {
            (Some(done), Some(total)) => format!(" {done}/{total}"),
            _ => String::new(),
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!("{health}{counts}"),
                Style::default().fg(color).bold(),
            ),
            Span::raw(format!("  {}", safe_text(&snapshot.label))),
        ]));
        let activity = snapshot
            .activity
            .as_deref()
            .map(safe_text)
            .unwrap_or_else(|| "No activity reported".into());
        let stage = snapshot
            .stage
            .as_deref()
            .map(|value| format!("{}: ", safe_text(value)))
            .unwrap_or_default();
        let detail = snapshot
            .detail
            .as_deref()
            .map(|value| format!("{} | ", safe_text(value)))
            .unwrap_or_default();
        lines.push(Line::raw(format!("{detail}{stage}{activity}")));
        lines.push(Line::raw(format!(
            "activity {} | checkpoint {}",
            age(snapshot.activity_age_secs),
            age(snapshot.checkpoint_age_secs)
        )));
    }
    frame.render_widget(Paragraph::new(lines), inner);
    Rect::new(area.x, area.y + height, area.width, area.height - height)
}

fn health_style(health: WorkflowHealth) -> (&'static str, Color) {
    match health {
        WorkflowHealth::Running => ("Running", Color::Cyan),
        WorkflowHealth::Quiet => ("Quiet?", Color::Yellow),
        WorkflowHealth::Waiting => ("Waiting", Color::Yellow),
        WorkflowHealth::Blocked => ("Blocked", Color::Yellow),
        WorkflowHealth::Failed => ("Failed", Color::Red),
        WorkflowHealth::Completed => ("Completed", Color::Green),
        WorkflowHealth::Stopped => ("Stopped", Color::Gray),
        WorkflowHealth::ObserverError => ("Observer error", Color::Red),
    }
}

fn age(seconds: Option<u64>) -> String {
    match seconds {
        None => "?".into(),
        Some(n) if n < 60 => format!("{n}s"),
        Some(n) if n < 3600 => format!("{}m", n / 60),
        Some(n) if n < 86400 => format!("{}h", n / 3600),
        Some(n) => format!("{}d", n / 86400),
    }
}

fn safe_text(value: &str) -> String {
    value
        .chars()
        .take(160)
        .filter(|ch| !matches!(*ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn sample() -> WorkflowSnapshot {
        WorkflowSnapshot {
            label: "Owned work".into(),
            health: WorkflowHealth::Failed,
            completed: Some(4),
            total: Some(9),
            detail: Some("Credits exhausted".into()),
            activity_age_secs: Some(12),
            checkpoint_age_secs: Some(180),
            ..Default::default()
        }
    }

    fn render(
        width: u16,
        height: u16,
        items: &[WorkflowSnapshot],
        enabled: bool,
    ) -> (Rect, String) {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let mut remaining = Rect::default();
        terminal
            .draw(|frame| {
                remaining = draw(frame, frame.area(), items, enabled, 3);
            })
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        (remaining, text)
    }

    #[test]
    fn workflow_disabled_empty_and_short_preserve_layout() {
        for (items, enabled, height) in [
            (vec![sample()], false, 24),
            (vec![], true, 24),
            (vec![sample()], true, 10),
        ] {
            assert_eq!(
                render(80, height, &items, enabled).0,
                Rect::new(0, 0, 80, height)
            );
        }
    }

    #[test]
    fn workflow_health_counts_and_independent_ages_fit_common_widths() {
        for width in [40, 80, 120] {
            let (remaining, text) = render(width, 24, &[sample()], true);
            assert_eq!(remaining.y, 5);
            for expected in [
                "Failed",
                "4/9",
                "Credits exhausted",
                "activity 12s",
                "checkpoint 3m",
            ] {
                assert!(
                    text.contains(expected),
                    "missing {expected} at {width}: {text}"
                );
            }
        }
    }

    #[test]
    fn workflow_overflow_is_bounded_and_unknown_is_not_zero() {
        let samples = vec![
            WorkflowSnapshot {
                label: "Quiet work".into(),
                health: WorkflowHealth::Quiet,
                ..Default::default()
            };
            256
        ];
        let (remaining, text) = render(40, 24, &samples, true);
        assert!(remaining.height >= 8 && remaining.y <= 12);
        assert!(text.contains("+253 more"));
        assert!(text.contains("activity ?") && text.contains("checkpoint ?"));
        assert!(!text.contains("0%") && !text.contains("Failed"));
    }
    #[test]
    fn workflow_errors_precede_running_and_display_text_is_bounded() {
        let mut items = vec![
            WorkflowSnapshot {
                label: "running".into(),
                ..Default::default()
            };
            8
        ];
        items.push(sample());
        let (_, text) = render(40, 24, &items, true);
        assert!(text.contains("Credits exhausted"));
        assert_eq!(safe_text("a\n\u{001b}\u{202e}b"), "a  b");
        assert_eq!(safe_text(&"x".repeat(10000)).len(), 160);
        for width in [0, 1, 23, 24, 40, 80, 120] {
            for height in [0, 1, 8, 10, 12, 13, 24] {
                let (rest, _) = render(width, height, &items, true);
                assert_eq!(rest.y + rest.height, height);
                assert_eq!(rest.width, width);
            }
        }
    }
}
