use super::info_widget::{AuthMethod, InfoWidgetData, UsageInfo, UsageProvider};
use super::ui::selection_highlight::highlight_line_selection;
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// The semantic freshness or availability of a provider credit snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProviderCreditStatus {
    Known,
    Pending,
    Stale,
    Unavailable,
    NotApplicable,
}

/// A presentation-only, privacy-safe summary of provider capacity or usage.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ProviderCreditState {
    pub(crate) status: ProviderCreditStatus,
    pub(crate) provider: String,
    pub(crate) primary_summary: Option<String>,
    pub(crate) secondary_summary: Option<String>,
    pub(crate) fetched_at: Option<std::time::Instant>,
    primary_remaining_ratio: Option<f32>,
    primary_window_label: Option<String>,
}

impl ProviderCreditState {
    pub(crate) fn known(
        provider: impl AsRef<str>,
        primary_summary: Option<String>,
        secondary_summary: Option<String>,
    ) -> Self {
        Self {
            status: ProviderCreditStatus::Known,
            provider: safe_provider_label(provider.as_ref()),
            primary_summary: primary_summary.and_then(|value| sanitize_label(&value)),
            secondary_summary: secondary_summary.and_then(|value| sanitize_label(&value)),
            fetched_at: Some(std::time::Instant::now()),
            primary_remaining_ratio: None,
            primary_window_label: None,
        }
    }

    pub(crate) fn pending(provider: impl AsRef<str>) -> Self {
        Self::empty(ProviderCreditStatus::Pending, provider)
    }

    pub(crate) fn unavailable(provider: impl AsRef<str>) -> Self {
        Self::empty(ProviderCreditStatus::Unavailable, provider)
    }

    pub(crate) fn not_applicable(provider: impl AsRef<str>) -> Self {
        Self::empty(ProviderCreditStatus::NotApplicable, provider)
    }

    pub(crate) fn stale(
        provider: impl AsRef<str>,
        primary_summary: Option<String>,
        secondary_summary: Option<String>,
    ) -> Self {
        Self {
            status: ProviderCreditStatus::Stale,
            provider: safe_provider_label(provider.as_ref()),
            primary_summary: primary_summary.and_then(|value| sanitize_label(&value)),
            secondary_summary: secondary_summary.and_then(|value| sanitize_label(&value)),
            fetched_at: None,
            primary_remaining_ratio: None,
            primary_window_label: None,
        }
    }

    fn empty(status: ProviderCreditStatus, provider: impl AsRef<str>) -> Self {
        Self {
            status,
            provider: safe_provider_label(provider.as_ref()),
            primary_summary: None,
            secondary_summary: None,
            fetched_at: None,
            primary_remaining_ratio: None,
            primary_window_label: None,
        }
    }

    /// Normalize the existing info-widget usage snapshot without doing I/O.
    pub(crate) fn from_usage_info(
        provider: impl AsRef<str>,
        usage: Option<&UsageInfo>,
        stale: bool,
        pending: bool,
        usage_display_used: bool,
    ) -> Self {
        let provider = provider.as_ref();
        // A refresh in flight must win over the last flattened snapshot. The
        // info widget can legitimately retain an available value while a
        // provider report is being refreshed, but the top bar must not present
        // that value as settled until the refresh completes.
        if pending {
            return Self::pending(provider);
        }

        let Some(usage) = usage else {
            return Self::not_applicable(provider);
        };

        if !usage.available {
            return Self::unavailable(provider);
        }

        let (primary_summary, secondary_summary) = match usage.provider {
            UsageProvider::Anthropic | UsageProvider::OpenAI => {
                let primary = usage_window_summary(
                    usage.primary_limit_label.as_deref(),
                    usage.five_hour,
                    usage_display_used,
                );
                let secondary = usage_window_summary(
                    usage.secondary_limit_label.as_deref(),
                    usage.seven_day,
                    usage_display_used,
                );
                (primary, secondary)
            }
            UsageProvider::CostBased => {
                let primary = if usage.total_cost.is_finite() {
                    Some(format!("${:.2} used", usage.total_cost.max(0.0)))
                } else {
                    None
                };
                let secondary = (usage.input_tokens > 0 || usage.output_tokens > 0).then(|| {
                    format!(
                        "{} tokens",
                        usage.input_tokens.saturating_add(usage.output_tokens)
                    )
                });
                (primary, secondary)
            }
            UsageProvider::Copilot => {
                let total = usage.input_tokens.saturating_add(usage.output_tokens);
                (Some(format!("{} tokens", total)), None)
            }
            UsageProvider::None => (None, None),
        };

        if primary_summary.is_none() && secondary_summary.is_none() {
            return Self::unavailable(provider);
        }

        let mut state = if stale {
            Self::stale(provider, primary_summary, secondary_summary)
        } else {
            Self::known(provider, primary_summary, secondary_summary)
        };
        state.primary_remaining_ratio = subscription_remaining_ratio(usage);
        state.primary_window_label = usage
            .primary_limit_label
            .as_deref()
            .and_then(sanitize_label);
        state.fetched_at = None;
        state
    }

    pub(crate) fn summary(&self) -> Option<String> {
        self.primary_summary
            .clone()
            .or_else(|| self.secondary_summary.clone())
            .or_else(|| match self.status {
                ProviderCreditStatus::Pending => Some("pending".to_string()),
                ProviderCreditStatus::Stale => Some("stale".to_string()),
                ProviderCreditStatus::Unavailable => Some("unavailable".to_string()),
                ProviderCreditStatus::NotApplicable => Some("not applicable".to_string()),
                ProviderCreditStatus::Known => None,
            })
    }
}

fn subscription_remaining_ratio(usage: &UsageInfo) -> Option<f32> {
    if !matches!(
        usage.provider,
        UsageProvider::Anthropic | UsageProvider::OpenAI
    ) || !usage.five_hour.is_finite()
        || !(0.0..=1.0).contains(&usage.five_hour)
        || (usage.primary_limit_label.is_none() && usage.five_hour == 0.0)
    {
        return None;
    }
    Some(1.0 - usage.five_hour)
}

fn usage_window_summary(label: Option<&str>, ratio: f32, used: bool) -> Option<String> {
    if !ratio.is_finite() || !(0.0..=1.0).contains(&ratio) {
        return None;
    }
    // The flattened usage snapshot uses zero as the sentinel for a window
    // that was not reported. Do not turn that missing window into a fabricated
    // "quota 100% left" summary.
    if label.is_none() && ratio == 0.0 {
        return None;
    }
    let label = label
        .filter(|label| !label.trim().is_empty())
        .unwrap_or("quota");
    let percent = if used {
        ratio * 100.0
    } else {
        (1.0 - ratio) * 100.0
    };
    Some(format!(
        "{} {:.0}% {}",
        label,
        percent,
        if used { "used" } else { "left" }
    ))
}

/// Safe renderer-facing values for the active session and its optional context.
#[derive(Clone, Debug, PartialEq)]
pub struct TopBarContext {
    pub(crate) session_label: String,
    pub(crate) server_label: Option<String>,
    pub(crate) client_label: Option<String>,
    pub(crate) provider_label: Option<String>,
    pub(crate) model_label: Option<String>,
    pub(crate) auth_label: Option<String>,
    pub(crate) reasoning_label: Option<String>,
    pub(crate) connection_label: Option<String>,
    pub(crate) version_label: Option<String>,
    pub(crate) credit: ProviderCreditState,
}

impl TopBarContext {
    pub(crate) fn new(session_label: impl AsRef<str>, credit: ProviderCreditState) -> Self {
        Self {
            session_label: sanitize_label(session_label.as_ref()).unwrap_or_default(),
            server_label: None,
            client_label: None,
            provider_label: None,
            model_label: None,
            auth_label: None,
            reasoning_label: None,
            connection_label: None,
            version_label: None,
            credit,
        }
    }

    pub(crate) fn with_provider_label(mut self, value: impl AsRef<str>) -> Self {
        self.provider_label = sanitize_label(value.as_ref());
        self
    }

    pub(crate) fn with_model_label(mut self, value: impl AsRef<str>) -> Self {
        self.model_label = sanitize_label(value.as_ref());
        self
    }

    #[cfg(test)]
    pub(crate) fn with_auth_label(mut self, value: impl AsRef<str>) -> Self {
        self.auth_label = safe_auth_text(value.as_ref());
        self
    }

    pub(crate) fn with_reasoning_label(mut self, value: impl AsRef<str>) -> Self {
        self.reasoning_label = sanitize_label(value.as_ref());
        self
    }

    pub(crate) fn with_server_label(mut self, value: impl AsRef<str>) -> Self {
        self.server_label = sanitize_label(value.as_ref());
        self
    }

    pub(crate) fn with_client_label(mut self, value: impl AsRef<str>) -> Self {
        self.client_label = sanitize_label(value.as_ref());
        self
    }

    pub(crate) fn with_connection_label(mut self, value: impl AsRef<str>) -> Self {
        self.connection_label = sanitize_label(value.as_ref());
        self
    }

    pub(crate) fn with_version_label(mut self, value: impl AsRef<str>) -> Self {
        self.version_label = sanitize_label(value.as_ref());
        self
    }

    pub(crate) fn with_auth_method(mut self, auth: AuthMethod) -> Self {
        self.auth_label = safe_auth_label(auth);
        self
    }

    pub(crate) fn fields(&self) -> Vec<TopBarField> {
        let mut fields = Vec::with_capacity(6);
        if !self.session_label.is_empty() {
            fields.push(TopBarField::new(
                TopBarFieldKind::Session,
                format!("📋  ◉ {}", self.session_label),
                Some(format!("📋 ◉ {}", self.session_label)),
            ));
        }
        let (credit_full, credit_compact) = self.credit_labels();
        fields.push(TopBarField::new(
            TopBarFieldKind::Credit,
            credit_full,
            Some(credit_compact),
        ));
        if self.provider_label.is_some() || self.model_label.is_some() {
            fields.push(TopBarField::new(
                TopBarFieldKind::ProviderModel,
                join_labels(
                    self.provider_label.as_deref(),
                    self.model_label.as_deref(),
                    " / ",
                ),
                self.model_label
                    .clone()
                    .or_else(|| self.provider_label.clone()),
            ));
        }
        if self.auth_label.is_some() || self.reasoning_label.is_some() {
            fields.push(TopBarField::new(
                TopBarFieldKind::AuthReasoning,
                join_labels(
                    self.auth_label.as_deref(),
                    self.reasoning_label.as_deref(),
                    " · ",
                ),
                self.auth_label
                    .clone()
                    .or_else(|| self.reasoning_label.clone()),
            ));
        }
        if self.server_label.is_some() || self.client_label.is_some() {
            fields.push(TopBarField::new(
                TopBarFieldKind::ServerClient,
                join_labels(
                    self.server_label.as_deref(),
                    self.client_label.as_deref(),
                    " · ",
                ),
                self.server_label
                    .clone()
                    .or_else(|| self.client_label.clone()),
            ));
        }
        if self.connection_label.is_some() || self.version_label.is_some() {
            fields.push(TopBarField::new(
                TopBarFieldKind::VersionConnection,
                join_labels(
                    self.connection_label.as_deref(),
                    self.version_label.as_deref(),
                    " · ",
                ),
                self.connection_label
                    .clone()
                    .or_else(|| self.version_label.clone()),
            ));
        }
        fields
    }

    pub(crate) fn clipboard_text(&self) -> String {
        let mut lines = vec![format!("Session: {}", self.session_label)];
        push_clipboard_field(&mut lines, "Provider", self.provider_label.as_deref());
        push_clipboard_field(&mut lines, "Model", self.model_label.as_deref());
        push_clipboard_field(&mut lines, "Auth", self.auth_label.as_deref());
        push_clipboard_field(&mut lines, "Reasoning", self.reasoning_label.as_deref());
        push_clipboard_field(&mut lines, "Credit", self.credit.summary().as_deref());
        push_clipboard_field(&mut lines, "Server", self.server_label.as_deref());
        push_clipboard_field(&mut lines, "Client", self.client_label.as_deref());
        push_clipboard_field(&mut lines, "Connection", self.connection_label.as_deref());
        push_clipboard_field(&mut lines, "Version", self.version_label.as_deref());
        lines.join("\n")
    }

    fn credit_labels(&self) -> (String, String) {
        if let Some(remaining_ratio) = self.credit.primary_remaining_ratio {
            let percent = (remaining_ratio.clamp(0.0, 1.0) * 100.0).round() as u16;
            let label = self
                .credit
                .primary_window_label
                .as_deref()
                .unwrap_or("quota");
            return (
                format!(
                    "{} {} {} {}% left",
                    self.credit.provider,
                    label,
                    credit_progress_text(remaining_ratio, 10),
                    percent
                ),
                credit_progress_text(remaining_ratio, 6),
            );
        }

        let summary = self
            .credit
            .summary()
            .unwrap_or_else(|| "unavailable".to_string());
        (format!("{} {summary}", self.credit.provider), summary)
    }
}

fn push_clipboard_field(lines: &mut Vec<String>, label: &str, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        lines.push(format!("{label}: {value}"));
    }
}

fn credit_progress_text(remaining_ratio: f32, cells: usize) -> String {
    let filled = (remaining_ratio.clamp(0.0, 1.0) * cells as f32).round() as usize;
    format!(
        "{}{}",
        "▰".repeat(filled),
        "▱".repeat(cells.saturating_sub(filled))
    )
}

fn join_labels(first: Option<&str>, second: Option<&str>, separator: &str) -> String {
    match (first, second) {
        (Some(first), Some(second)) => format!("{first}{separator}{second}"),
        (Some(first), None) => first.to_string(),
        (None, Some(second)) => second.to_string(),
        (None, None) => String::new(),
    }
}

/// Fixed semantic field order. Lower values are retained longer when space is tight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TopBarFieldKind {
    Session,
    Credit,
    ProviderModel,
    AuthReasoning,
    ServerClient,
    VersionConnection,
}

impl TopBarFieldKind {
    pub(crate) const fn priority(self) -> u8 {
        match self {
            Self::Session => 0,
            Self::Credit => 1,
            Self::ProviderModel => 2,
            Self::AuthReasoning => 3,
            Self::ServerClient => 4,
            Self::VersionConnection => 5,
        }
    }
}

/// One full and optional compact representation of a top-bar field.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TopBarField {
    pub(crate) kind: TopBarFieldKind,
    pub(crate) priority: u8,
    pub(crate) full: Line<'static>,
    pub(crate) compact: Option<Line<'static>>,
}

impl TopBarField {
    pub(crate) fn new(
        kind: TopBarFieldKind,
        full: impl Into<String>,
        compact: Option<String>,
    ) -> Self {
        Self {
            kind,
            priority: kind.priority(),
            full: Line::from(full.into()),
            compact: compact.map(Line::from),
        }
    }
}

/// Why no top-bar rows were reserved for a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TopBarSuppression {
    Disabled,
    TooNarrow,
    TooShort,
    NoSession,
}

/// Renderer-facing layout contract. Selection and allocation are completed by later phases.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TopBarLayout {
    pub(crate) row_count: u16,
    pub(crate) lines: Vec<Line<'static>>,
    pub(crate) visible_fields: Vec<TopBarFieldKind>,
    pub(crate) suppression_reason: Option<TopBarSuppression>,
}

impl TopBarLayout {
    pub(crate) fn suppressed(reason: TopBarSuppression) -> Self {
        Self {
            row_count: 0,
            lines: Vec::new(),
            visible_fields: Vec::new(),
            suppression_reason: Some(reason),
        }
    }
}

const TOP_BAR_MIN_WIDTH: u16 = 32;
const TOP_BAR_MIN_CONTENT_ROWS: u16 = 3;
const TOP_BAR_SEPARATOR: &str = "  ·  ";

pub(crate) const fn top_bar_width_is_usable(width: u16) -> bool {
    width >= TOP_BAR_MIN_WIDTH
}

/// Select the deterministic top-bar layout for one frame.
///
/// `minimum_content_rows` is the amount of vertical space the caller must keep
/// for the conversation and its required chrome. The selector owns the
/// zero-to-three-row policy, while the caller owns the actual rectangle split.
pub(crate) fn select_top_bar_layout(
    context: Option<&TopBarContext>,
    enabled: bool,
    width: u16,
    height: u16,
    minimum_content_rows: u16,
) -> TopBarLayout {
    if !enabled {
        return TopBarLayout::suppressed(TopBarSuppression::Disabled);
    }
    let Some(context) = context.filter(|context| !context.session_label.is_empty()) else {
        return TopBarLayout::suppressed(TopBarSuppression::NoSession);
    };
    if !top_bar_width_is_usable(width) {
        return TopBarLayout::suppressed(TopBarSuppression::TooNarrow);
    }

    let remaining = height.saturating_sub(minimum_content_rows.max(TOP_BAR_MIN_CONTENT_ROWS));
    if remaining == 0 {
        return TopBarLayout::suppressed(TopBarSuppression::TooShort);
    }

    let max_rows: u16 = if width >= 140 && remaining >= 3 {
        3
    } else if width >= 64 && remaining >= 2 {
        2
    } else {
        1
    };
    let fields = context.fields();
    let core = fields
        .iter()
        .filter(|field| {
            matches!(
                field.kind,
                TopBarFieldKind::Session | TopBarFieldKind::Credit
            )
        })
        .collect::<Vec<_>>();
    let optional = fields
        .iter()
        .filter(|field| {
            !matches!(
                field.kind,
                TopBarFieldKind::Session | TopBarFieldKind::Credit
            )
        })
        .collect::<Vec<_>>();

    let mut lines = Vec::with_capacity(max_rows as usize);
    let mut visible_fields = Vec::with_capacity(fields.len());
    let core_line = render_core_line(&core, width as usize);
    if !core_line.is_empty() {
        lines.push(Line::from(core_line));
        visible_fields.extend(core.iter().map(|field| field.kind));
    }

    if max_rows >= 2 {
        let mut optional_lines = Vec::new();
        let optional_capacity = max_rows.saturating_sub(1) as usize;
        for field in optional {
            let Some(text) = field_text_that_fits(field, width as usize) else {
                continue;
            };
            if let Some(last) = optional_lines.last_mut()
                && append_field(last, &text, width as usize)
            {
                visible_fields.push(field.kind);
                continue;
            }
            if optional_lines.len() >= optional_capacity {
                continue;
            }
            optional_lines.push(text);
            visible_fields.push(field.kind);
        }
        lines.extend(optional_lines.into_iter().map(Line::from));
    } else if let Some(last) = lines.last_mut() {
        // A one-row bar may show one optional field only when it fits without
        // weakening the session/credit pair.
        let mut line = line_text(last);
        for field in optional {
            let Some(text) = field_text_that_fits(field, width as usize) else {
                continue;
            };
            if append_field(&mut line, &text, width as usize) {
                *last = Line::from(line);
                visible_fields.push(field.kind);
                break;
            }
        }
    }

    let row_count = lines.len().min(3) as u16;
    if row_count == 0 {
        TopBarLayout::suppressed(TopBarSuppression::TooNarrow)
    } else {
        TopBarLayout {
            row_count,
            lines,
            visible_fields,
            suppression_reason: None,
        }
    }
}

fn field_text_that_fits(field: &TopBarField, width: usize) -> Option<String> {
    let full = line_text(&field.full);
    if full.width() <= width {
        return Some(full);
    }
    field.compact.as_ref().map(line_text).map(|compact| {
        if compact.width() <= width {
            compact
        } else {
            truncate_display_width(&compact, width)
        }
    })
}

fn line_text(line: &Line<'static>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}

fn append_field(line: &mut String, field: &str, width: usize) -> bool {
    let separator = if line.is_empty() {
        ""
    } else {
        TOP_BAR_SEPARATOR
    };
    let candidate = format!("{line}{separator}{field}");
    if candidate.width() > width {
        return false;
    }
    line.push_str(separator);
    line.push_str(field);
    true
}

fn render_core_line(core: &[&TopBarField], width: usize) -> String {
    if core.is_empty() || width == 0 {
        return String::new();
    }
    let values = core
        .iter()
        .map(|field| field_text_that_fits(field, width))
        .collect::<Option<Vec<_>>>();
    let Some(values) = values else {
        return String::new();
    };
    let joined = values.join(TOP_BAR_SEPARATOR);
    if joined.width() <= width {
        return joined;
    }

    // Keep both immutable core fields discoverable even on the one-row path.
    // Split the available cells around the separator, then truncate each field
    // independently so a long session name cannot erase the credit state.
    if values.len() == 2 {
        let separator_width = TOP_BAR_SEPARATOR.width();
        let left_width = width.saturating_sub(separator_width).saturating_add(1) / 2;
        let right_width = width
            .saturating_sub(separator_width)
            .saturating_sub(left_width);
        let left = truncate_display_width(&values[0], left_width.max(1));
        let right = truncate_display_width(&values[1], right_width.max(1));
        return format!("{left}{TOP_BAR_SEPARATOR}{right}");
    }
    truncate_display_width(&joined, width)
}

/// Render a selected layout into its already-allocated rectangle.
pub(crate) fn render_top_bar(
    frame: &mut Frame,
    area: Rect,
    layout: &TopBarLayout,
    selection: Option<crate::tui::CopySelectionRange>,
) -> Vec<Line<'static>> {
    if area.width == 0 || area.height == 0 || layout.row_count == 0 {
        return Vec::new();
    }
    let mut visible_fields = layout.visible_fields.iter().copied();
    let mut lines = layout
        .lines
        .iter()
        .take(area.height as usize)
        .map(|line| {
            let text = line_text(line);
            let mut spans = Vec::new();
            for (index, field) in text.split(TOP_BAR_SEPARATOR).enumerate() {
                if index > 0 {
                    spans.push(Span::styled(
                        TOP_BAR_SEPARATOR,
                        Style::default().fg(Color::DarkGray),
                    ));
                }
                spans.extend(styled_top_bar_field(
                    visible_fields
                        .next()
                        .unwrap_or(TopBarFieldKind::VersionConnection),
                    field,
                ));
            }
            Line::from(spans)
        })
        .collect::<Vec<_>>();
    if let Some(range) = selection.filter(|range| {
        range.start.pane == crate::tui::CopySelectionPane::TopBar
            && range.end.pane == crate::tui::CopySelectionPane::TopBar
    }) {
        for (line_index, line) in lines.iter_mut().enumerate() {
            let start = if line_index == range.start.abs_line {
                range.start.column
            } else if line_index > range.start.abs_line && line_index <= range.end.abs_line {
                0
            } else {
                continue;
            };
            let end = if line_index == range.end.abs_line {
                range.end.column
            } else {
                line_text(line).width()
            };
            *line = highlight_line_selection(line, start, end);
        }
    }
    frame.render_widget(Paragraph::new(lines.clone()), area);
    lines
}

fn styled_top_bar_field(kind: TopBarFieldKind, text: &str) -> Vec<Span<'static>> {
    let base = match kind {
        TopBarFieldKind::Session => Color::Rgb(100, 200, 100),
        TopBarFieldKind::Credit => credit_status_color(text),
        TopBarFieldKind::ProviderModel => Color::Rgb(255, 150, 200),
        TopBarFieldKind::AuthReasoning => Color::Rgb(140, 180, 255),
        TopBarFieldKind::ServerClient => Color::Rgb(120, 200, 220),
        TopBarFieldKind::VersionConnection => Color::Gray,
    };
    if kind != TopBarFieldKind::Credit {
        return vec![Span::styled(text.to_string(), Style::default().fg(base))];
    }

    let Some(progress_start) = text.find(['▰', '▱']) else {
        return vec![Span::styled(text.to_string(), Style::default().fg(base))];
    };
    let prefix = &text[..progress_start];
    let progress_and_suffix = &text[progress_start..];
    let progress_end = progress_and_suffix
        .char_indices()
        .find(|(_, character)| !matches!(character, '▰' | '▱'))
        .map(|(index, _)| index)
        .unwrap_or(progress_and_suffix.len());
    let progress = &progress_and_suffix[..progress_end];
    let suffix = &progress_and_suffix[progress_end..];
    let filled_len = progress
        .chars()
        .take_while(|character| *character == '▰')
        .count();
    let filled_bytes = progress
        .char_indices()
        .nth(filled_len)
        .map(|(index, _)| index)
        .unwrap_or(progress.len());
    vec![
        Span::styled(prefix.to_string(), Style::default().fg(base).bold()),
        Span::styled(
            progress[..filled_bytes].to_string(),
            Style::default().fg(base),
        ),
        Span::styled(
            progress[filled_bytes..].to_string(),
            Style::default().fg(Color::Rgb(50, 50, 60)),
        ),
        Span::styled(suffix.to_string(), Style::default().fg(base).bold()),
    ]
}

fn credit_status_color(text: &str) -> Color {
    if text.contains("unavailable") || text.contains("not applicable") {
        Color::DarkGray
    } else if text.contains("pending") || text.contains("stale") {
        Color::Rgb(255, 200, 100)
    } else if let Some(percent) = text
        .split_whitespace()
        .find_map(|part| part.strip_suffix('%')?.parse::<u16>().ok())
    {
        if percent <= 20 {
            Color::Rgb(255, 100, 100)
        } else if percent <= 50 {
            Color::Rgb(255, 200, 100)
        } else {
            Color::Rgb(100, 200, 100)
        }
    } else {
        let filled = text.chars().filter(|character| *character == '▰').count();
        let empty = text.chars().filter(|character| *character == '▱').count();
        if filled + empty == 0 {
            Color::Rgb(140, 180, 255)
        } else {
            let remaining_percent = filled * 100 / (filled + empty);
            if remaining_percent <= 20 {
                Color::Rgb(255, 100, 100)
            } else if remaining_percent <= 50 {
                Color::Rgb(255, 200, 100)
            } else {
                Color::Rgb(100, 200, 100)
            }
        }
    }
}

pub(crate) fn top_bar_clipboard_rect(lines: &[Line<'static>], area: Rect) -> Option<Rect> {
    lines.iter().enumerate().find_map(|(line_index, line)| {
        let text = line_text(line);
        let byte = text.find('📋')?;
        let x_offset = text[..byte].width() as u16;
        Some(Rect::new(
            area.x.saturating_add(x_offset),
            area.y.saturating_add(line_index as u16),
            UnicodeWidthChar::width('📋').unwrap_or(1) as u16,
            1,
        ))
    })
}

/// Convert the existing info-widget snapshot into a safe active-session context.
#[cfg(test)]
pub(crate) fn context_from_info_widget_data(
    session_label: Option<&str>,
    data: &InfoWidgetData,
) -> Option<TopBarContext> {
    context_from_info_widget_data_with_state(session_label, data, false, false)
}

/// Convert the existing info-widget snapshot while preserving the caller's
/// non-blocking freshness state.
pub(crate) fn context_from_info_widget_data_with_state(
    session_label: Option<&str>,
    data: &InfoWidgetData,
    stale: bool,
    pending: bool,
) -> Option<TopBarContext> {
    let session_label = session_label.filter(|label| !label.trim().is_empty())?;
    let provider = data.provider_name.as_deref().unwrap_or("provider");
    let credit = ProviderCreditState::from_usage_info(
        provider,
        data.usage_info.as_ref(),
        stale,
        pending,
        data.usage_display_used,
    );
    let mut context = TopBarContext::new(session_label, credit).with_auth_method(data.auth_method);
    if let Some(provider) = data.provider_name.as_deref() {
        context = context.with_provider_label(provider);
    }
    if let Some(model) = data.model.as_deref() {
        context = context.with_model_label(model);
    }
    if let Some(reasoning) = data.reasoning_effort.as_deref() {
        context = context.with_reasoning_label(reasoning);
    }
    Some(context)
}

pub(crate) fn safe_auth_label(auth: AuthMethod) -> Option<String> {
    let label = match auth {
        AuthMethod::Unknown => return None,
        AuthMethod::ApiKey
        | AuthMethod::AnthropicApiKey
        | AuthMethod::OpenAIApiKey
        | AuthMethod::OpenRouterApiKey
        | AuthMethod::OpenCodeApiKey => "API key",
        AuthMethod::AnthropicOAuth
        | AuthMethod::OpenAIOAuth
        | AuthMethod::CopilotOAuth
        | AuthMethod::GeminiOAuth => "OAuth",
    };
    Some(label.to_string())
}

#[cfg(test)]
fn safe_auth_text(value: &str) -> Option<String> {
    let value = sanitize_label(value)?;
    (!value.to_ascii_lowercase().contains("key=")).then_some(value)
}

fn safe_provider_label(value: &str) -> String {
    sanitize_label(value).unwrap_or_else(|| "provider".to_string())
}

/// Sanitize a display label before it enters renderer-owned state.
pub(crate) fn sanitize_label(value: &str) -> Option<String> {
    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() || collapsed.len() > 256 {
        return None;
    }
    let lower = collapsed.to_ascii_lowercase();
    let sensitive_markers = [
        "bearer ",
        "api_key=",
        "access_token=",
        "refresh_token=",
        "credential=",
        "password=",
        "secret=",
        "sk-",
    ];
    if sensitive_markers
        .iter()
        .any(|marker| lower.contains(marker))
    {
        return None;
    }
    Some(collapsed)
}

/// Truncate text by terminal display cells without cutting a wide character or
/// leaving a dangling combining mark/zero-width joiner at the boundary.
pub(crate) fn truncate_display_width(value: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if value.width() <= max_width {
        return value.to_string();
    }

    let mut output = String::new();
    let mut used = 0usize;
    let target = max_width.saturating_sub(1);
    for ch in value.chars() {
        let width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used.saturating_add(width) > target {
            break;
        }
        output.push(ch);
        used += width;
    }
    while output
        .chars()
        .last()
        .is_some_and(|ch| matches!(ch, '\u{200d}' | '\u{200c}' | '\u{301}' | '\u{fe0f}'))
    {
        output.pop();
    }
    output.push('…');
    output
}

#[cfg(test)]
pub(crate) fn bounded_line(value: &str, max_width: usize) -> Line<'static> {
    Line::from(truncate_display_width(value, max_width))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::text::Line;
    use unicode_width::UnicodeWidthStr;

    fn credit() -> ProviderCreditState {
        ProviderCreditState::known(
            "OpenAI",
            Some("62% left".to_string()),
            Some("5-hour".to_string()),
        )
    }

    fn context() -> TopBarContext {
        TopBarContext::new("dolphin", credit())
            .with_provider_label("openai")
            .with_model_label("gpt-5.6-sol")
            .with_auth_label("OAuth")
            .with_reasoning_label("medium")
            .with_server_label("local")
            .with_client_label("jcode")
            .with_connection_label("https")
            .with_version_label("v0.81.727")
    }

    #[test]
    fn credit_states_are_explicit_and_unknown_never_becomes_zero() {
        let known = credit();
        assert_eq!(known.status, ProviderCreditStatus::Known);
        assert_eq!(known.primary_summary.as_deref(), Some("62% left"));

        for state in [
            ProviderCreditState::pending("OpenAI"),
            ProviderCreditState::stale(
                "OpenAI",
                Some("62% left".to_string()),
                Some("5-hour".to_string()),
            ),
            ProviderCreditState::unavailable("OpenAI"),
            ProviderCreditState::not_applicable("OpenCode"),
        ] {
            assert_ne!(state.status, ProviderCreditStatus::Known);
            assert_ne!(state.primary_summary.as_deref(), Some("0% left"));
        }
    }

    #[test]
    fn subscription_credit_uses_a_percentage_bar_with_glyph_only_fallback() {
        let usage = UsageInfo {
            provider: UsageProvider::OpenAI,
            primary_limit_label: Some("7-day".to_string()),
            five_hour: 0.34,
            available: true,
            ..Default::default()
        };
        let credit =
            ProviderCreditState::from_usage_info("OpenAI", Some(&usage), false, false, false);
        let field = TopBarContext::new("dolphin", credit)
            .fields()
            .into_iter()
            .find(|field| field.kind == TopBarFieldKind::Credit)
            .expect("credit field");
        let full = line_text(&field.full);
        let compact = line_text(field.compact.as_ref().expect("compact credit"));

        assert_eq!(full, "OpenAI 7-day ▰▰▰▰▰▰▰▱▱▱ 66% left");
        assert_eq!(compact, "▰▰▰▰▱▱");
    }

    #[test]
    fn field_priorities_keep_session_and_credit_before_optional_context() {
        assert!(TopBarFieldKind::Session.priority() < TopBarFieldKind::ProviderModel.priority());
        assert!(TopBarFieldKind::Credit.priority() < TopBarFieldKind::AuthReasoning.priority());
        assert!(
            TopBarFieldKind::AuthReasoning.priority() < TopBarFieldKind::ServerClient.priority()
        );
        assert!(
            TopBarFieldKind::ServerClient.priority()
                < TopBarFieldKind::VersionConnection.priority()
        );

        let fields = context().fields();
        assert_eq!(fields[0].kind, TopBarFieldKind::Session);
        assert_eq!(fields[1].kind, TopBarFieldKind::Credit);
    }

    #[test]
    fn renderer_context_accepts_safe_labels_but_rejects_secret_bearing_text() {
        let ctx = context();
        assert_eq!(ctx.auth_label.as_deref(), Some("OAuth"));
        assert_eq!(ctx.provider_label.as_deref(), Some("openai"));

        for secret in [
            "Bearer eyJhbGciOiJIUzI1NiIs.test",
            "sk-proj-1234567890",
            "refresh_token=secret-value",
            "provider error: credential=super-secret",
        ] {
            assert!(
                sanitize_label(secret).is_none(),
                "secret was accepted: {secret}"
            );
        }
        assert_eq!(
            sanitize_label("connected via OAuth").as_deref(),
            Some("connected via OAuth")
        );
    }

    #[test]
    fn width_helpers_do_not_split_wide_or_combining_text() {
        for (text, width) in [("session 👩‍💻", 10), ("caf́e", 4), ("東京", 3)] {
            let truncated = truncate_display_width(text, width);
            assert!(truncated.width() <= width, "{truncated:?} exceeds {width}");
            assert!(!truncated.ends_with('\u{200d}'));
            assert!(!truncated.ends_with('\u{301}'));
        }

        let line = bounded_line("a very long session name", 8);
        assert!(line.width() <= 8);
        assert_eq!(
            line,
            Line::from(truncate_display_width("a very long session name", 8))
        );
    }

    #[test]
    fn adaptive_selector_preserves_core_fields_across_size_table() {
        let context = context();
        let cases = [
            (24, 12, 0, Some(TopBarSuppression::TooNarrow)),
            (40, 12, 1, None),
            (80, 24, 2, None),
            (120, 32, 2, None),
            (160, 48, 2, None),
            (80, 8, 0, Some(TopBarSuppression::TooShort)),
        ];

        for (width, height, expected_rows, expected_suppression) in cases {
            let layout = select_top_bar_layout(Some(&context), true, width, height, 8);
            assert_eq!(layout.row_count, expected_rows, "size {width}x{height}");
            assert_eq!(layout.suppression_reason, expected_suppression);
            assert!(
                layout
                    .lines
                    .iter()
                    .all(|line| line.width() <= width as usize)
            );
            if expected_suppression.is_none() {
                assert!(layout.visible_fields.contains(&TopBarFieldKind::Session));
                assert!(layout.visible_fields.contains(&TopBarFieldKind::Credit));
            }
        }
    }

    #[test]
    fn adaptive_selector_is_width_safe_for_unicode_and_long_labels() {
        let context = TopBarContext::new(
            "a-session-name-that-is-long-enough-to-wrap 👩‍💻",
            ProviderCreditState::known(
                "東京",
                Some("5-hour 62% left".to_string()),
                Some("weekly".to_string()),
            ),
        )
        .with_model_label("claudé-sonnet-東京")
        .with_reasoning_label("medium 👩‍💻")
        .with_server_label("server-with-a-long-name")
        .with_client_label("jcode");

        for width in 24..=160 {
            let layout = select_top_bar_layout(Some(&context), true, width, 48, 8);
            assert!(layout.row_count <= 3);
            assert!(
                layout
                    .lines
                    .iter()
                    .all(|line| line.width() <= width as usize)
            );
            for line in &layout.lines {
                let text = line.to_string();
                assert!(!text.ends_with('\u{200d}'));
                assert!(!text.ends_with('\u{301}'));
            }
        }
    }

    #[test]
    fn adaptive_selector_is_stable_for_one_hundred_unchanged_refreshes() {
        let context = context();
        let first = select_top_bar_layout(Some(&context), true, 120, 32, 8);
        for _ in 0..100 {
            assert_eq!(
                select_top_bar_layout(Some(&context), true, 120, 32, 8),
                first
            );
        }
    }
}
