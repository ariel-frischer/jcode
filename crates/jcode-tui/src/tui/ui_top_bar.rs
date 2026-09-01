use super::info_widget::{AuthMethod, InfoWidgetData, UsageInfo, UsageProvider};
use ratatui::prelude::*;
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
        }
    }

    fn empty(status: ProviderCreditStatus, provider: impl AsRef<str>) -> Self {
        Self {
            status,
            provider: safe_provider_label(provider.as_ref()),
            primary_summary: None,
            secondary_summary: None,
            fetched_at: None,
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
                format!("◉ {}", self.session_label),
                Some(format!("◉ {}", self.session_label)),
            ));
        }
        fields.push(TopBarField::new(
            TopBarFieldKind::Credit,
            format!(
                "{} {}",
                self.credit.provider,
                self.credit.summary().unwrap_or_default()
            ),
            Some(
                self.credit
                    .summary()
                    .unwrap_or_else(|| "unavailable".to_string()),
            ),
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

/// Convert the existing info-widget snapshot into a safe active-session context.
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
}
