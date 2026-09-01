use super::*;

const REMOTE_STARTUP_HEADER_DEBOUNCE: Duration = Duration::from_millis(400);

/// How long a routine `LoadingSession` phase may keep showing the known model
/// hint before the header falls back to the "loading session…" label. History
/// bootstrap normally lands in ~1s, so the common spawn path never flashes a
/// transient loading label; genuinely stuck loads still surface after this
/// grace period.
const REMOTE_LOADING_HEADER_GRACE: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WidgetProviderKind {
    Anthropic,
    OpenAI,
    OpenCode,
    OpenRouter,
    CostBasedApiKey,
    Copilot,
    Gemini,
    Unknown,
}

impl WidgetProviderKind {
    pub(super) fn from_provider_key(raw: Option<&str>) -> Self {
        match raw.map(|provider| provider.trim().to_ascii_lowercase()) {
            Some(provider) if provider == "openrouter" => Self::OpenRouter,
            Some(provider) if matches!(provider.as_str(), "opencode" | "opencode-go") => {
                Self::OpenCode
            }
            Some(provider)
                if matches!(
                    provider.as_str(),
                    "bedrock" | "aws-bedrock" | "azure-openai"
                ) || crate::provider_catalog::openai_compatible_profile_by_id(&provider)
                    .is_some_and(|profile| profile.requires_api_key) =>
            {
                Self::CostBasedApiKey
            }
            Some(provider) if provider == "copilot" => Self::Copilot,
            Some(provider) if provider == "gemini" => Self::Gemini,
            Some(provider) if provider == "openai" => Self::OpenAI,
            Some(provider) if matches!(provider.as_str(), "anthropic" | "claude") => {
                Self::Anthropic
            }
            _ => Self::Unknown,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct WidgetRouteInfo {
    provider: WidgetProviderKind,
    is_remote: bool,
}

fn optional_env_value(name: &str) -> Option<String> {
    match std::env::var(name) {
        Ok(value) => Some(value),
        Err(std::env::VarError::NotPresent) => None,
        Err(error) => {
            crate::logging::warn(&format!(
                "Ignoring invalid {name} environment value: {error}"
            ));
            None
        }
    }
}

impl App {
    pub(super) fn sanitize_remote_model_hint(model: Option<String>) -> Option<String> {
        model
            .map(|model| model.trim().to_string())
            .filter(|model| !model.is_empty() && !model.eq_ignore_ascii_case("unknown"))
    }

    pub(super) fn configured_remote_provider_hint(&self) -> Option<String> {
        optional_env_value("JCODE_PROVIDER")
            .or_else(|| crate::config::config().provider.default_provider.clone())
            .map(|provider| provider.trim().to_string())
            .filter(|provider| !provider.is_empty())
    }

    pub(super) fn configured_remote_model_hint(&self) -> Option<String> {
        Self::sanitize_remote_model_hint(
            optional_env_value("JCODE_MODEL")
                .or_else(|| crate::config::config().provider.default_model.clone()),
        )
    }

    pub(in crate::tui::app) fn effective_remote_provider_model(&self) -> Option<String> {
        Self::sanitize_remote_model_hint(self.remote_provider_model.clone())
            .or_else(|| Self::sanitize_remote_model_hint(self.session.model.clone()))
            .or_else(|| self.configured_remote_model_hint())
    }

    /// Provider/model identity used for reasoning-effort UI decisions in remote
    /// mode. Prefers the server-reported values, falling back to the same hints
    /// the header uses (session stub, `JCODE_MODEL`, config default) so effort
    /// cycling works during the pre-History bootstrap window instead of
    /// reporting "not available" until the server payload settles.
    pub(in crate::tui::app) fn remote_effort_identity(&self) -> (Option<String>, Option<String>) {
        let model = self.effective_remote_provider_model();
        let provider = self.remote_provider_name.clone().or_else(|| {
            model
                .as_deref()
                .and_then(|model| {
                    crate::provider::provider_for_model_with_hint(model, None).map(str::to_string)
                })
                .or_else(|| self.configured_remote_provider_hint())
        });
        (provider, model)
    }

    /// Best-known current reasoning effort for the remote session. Falls back
    /// to the configured provider-family default when the server has not
    /// reported one yet, so pre-settle effort cycling starts from the value the
    /// session will actually use instead of assuming the maximum.
    pub(in crate::tui::app) fn remote_reasoning_effort_hint(&self) -> Option<String> {
        self.remote_reasoning_effort.clone().or_else(|| {
            let (provider, model) = self.remote_effort_identity();
            let provider = provider.as_deref().unwrap_or("").to_ascii_lowercase();
            let model = model.as_deref().unwrap_or("").to_ascii_lowercase();
            let cfg = &crate::config::config().provider;
            if provider.contains("anthropic")
                || provider.contains("claude")
                || model.starts_with("claude-")
            {
                cfg.anthropic_reasoning_effort.clone()
            } else if provider.contains("openai")
                || provider.contains("codex")
                || model.starts_with("gpt-")
            {
                cfg.openai_reasoning_effort.clone()
            } else {
                None
            }
        })
    }

    pub(super) fn remote_header_provider_model(&self) -> Option<String> {
        let effective_model = self.effective_remote_provider_model();

        self.remote_startup_phase
            .as_ref()
            .and_then(|phase| {
                let elapsed = self
                    .remote_startup_phase_started
                    .map_or(Duration::ZERO, |started| started.elapsed());

                // Routine bootstrap phases (connecting, then loading the
                // session history) should not repaint the header when we
                // already know which model this session runs: the pre-settle
                // flicker ("model -> loading session… -> model") reads as
                // instability. Keep showing the model and only surface the
                // phase label once it overstays its expected budget.
                match phase {
                    super::RemoteStartupPhase::Connecting if effective_model.is_some() => {
                        return effective_model.clone();
                    }
                    super::RemoteStartupPhase::LoadingSession
                        if effective_model.is_some() && elapsed < REMOTE_LOADING_HEADER_GRACE =>
                    {
                        return effective_model.clone();
                    }
                    _ => {}
                }

                let should_defer_header = matches!(phase, super::RemoteStartupPhase::Connecting)
                    && elapsed < REMOTE_STARTUP_HEADER_DEBOUNCE;

                if should_defer_header {
                    None
                } else {
                    Some(phase.header_label_with_elapsed(elapsed))
                }
            })
            .or(effective_model)
            .or_else(|| {
                (self.remote_session_id.is_some() || self.connection_type.is_some())
                    .then(|| "connected".to_string())
            })
    }

    pub(super) fn remote_header_provider_name(&self) -> Option<String> {
        let configured_provider_hint = self.configured_remote_provider_hint();
        self.remote_provider_name
            .clone()
            .or_else(|| {
                self.effective_remote_provider_model().and_then(|model| {
                    crate::provider::provider_for_model_with_hint(&model, None)
                        .or(configured_provider_hint.as_deref())
                        .map(str::to_string)
                })
            })
            .filter(|provider| !provider.trim().is_empty())
    }

    pub(super) fn widget_route_info(&self, model: Option<&str>) -> WidgetRouteInfo {
        let uses_remote_widget_metadata = self.is_remote || self.is_replay_runtime();
        let remote_provider_name = if uses_remote_widget_metadata {
            self.remote_header_provider_name()
        } else {
            None
        };
        let provider_name = if uses_remote_widget_metadata {
            remote_provider_name.as_deref()
        } else {
            Some(self.provider.name())
        };

        let provider_from_hint = WidgetProviderKind::from_provider_key(provider_name);
        let provider = if provider_from_hint != WidgetProviderKind::Unknown {
            provider_from_hint
        } else {
            WidgetProviderKind::from_provider_key(
                model
                    .map(|model| crate::provider::resolve_model_capabilities(model, provider_name))
                    .and_then(|caps| caps.provider)
                    .as_deref(),
            )
        };

        WidgetRouteInfo {
            provider,
            is_remote: uses_remote_widget_metadata,
        }
    }

    /// Resolve the active credential (OAuth vs API key) for a dual-auth
    /// provider (Anthropic / OpenAI). This is the one place billing identity is
    /// decided for the info widget, regardless of transport:
    ///
    /// * Remote sessions use [`App::remote_resolved_credential`], which the
    ///   server resolved authoritatively from its live credentials.
    /// * Local sessions prefer the provider's *explicitly pinned* credential
    ///   ([`Provider::active_explicit_credential`]) so the widget reflects the
    ///   credential the next request will actually use the instant the user
    ///   switches OAuth<->API (model picker, `/account`, header toggle). That
    ///   read is in-memory and cache-free, so it never lingers on a stale
    ///   [`AuthStatus`] snapshot (cached up to 60s) or a `JCODE_RUNTIME_PROVIDER`
    ///   pin that drifted out of sync with the provider. When the provider is in
    ///   auto mode (no explicit pin) it falls back to
    ///   [`resolve_dual_credential_auth`] -- shared with the header tag and
    ///   model-switch line -- which is cheap (cached probe, no per-frame I/O).
    ///
    /// Returns `None` when neither transport can determine the credential (e.g.
    /// the server didn't report one, or no credentials are configured locally).
    pub(super) fn dual_credential_active(
        &self,
        route: WidgetRouteInfo,
        provider: jcode_provider_core::ActiveProvider,
    ) -> Option<crate::auth::ActiveCredential> {
        if route.is_remote {
            if let Some(resolved) = self.remote_resolved_credential {
                return Some(resolved.into());
            }

            // Older history payloads and replay snapshots may not carry the
            // resolved credential, but an explicitly pinned route still tells
            // us whether this is subscription OAuth or metered API-key usage.
            // Never guess from the provider family alone: doing so made an
            // unresolved OpenAI API session render cached subscription limits.
            return self
                .session
                .route_api_method
                .as_deref()
                .and_then(jcode_provider_core::AuthRoute::parse)
                .filter(|auth_route| auth_route.active_provider() == provider)
                .map(|auth_route| auth_route.resolved_credential().into());
        }

        // Authoritative, cache-free answer from the live provider whenever the
        // user has explicitly pinned a credential. This reflects exactly what the
        // next request will use, so an explicit OAuth<->API switch is visible on
        // the very next frame. For local sessions the requested `provider` always
        // matches the live active provider (the widget route is derived from
        // `self.provider.name()`), and remote sessions returned above, so the
        // pin maps onto the right dual-auth provider. Explicit reads do no disk
        // I/O, so the common per-frame path stays cheap; auto mode returns `None`
        // here and falls through to the cached heuristic below.
        if let Some(resolved) = self.provider.active_explicit_credential() {
            return Some(resolved.into());
        }

        // Render path: use the non-blocking probe. `check_fast` blocks on a
        // cold/expired snapshot (~20-30ms of credential-file reads) directly on
        // the frame thread, which shows up as a periodic stall while typing.
        // `auth_status()` above already made this choice; these sibling
        // per-frame lookups must match it.
        let auth_status = crate::auth::AuthStatus::check_fast_nonblocking();
        let runtime_provider = active_runtime_provider_key();
        crate::auth::resolve_dual_credential_auth(
            provider,
            &auth_status,
            runtime_provider.as_deref(),
        )
        .map(|resolved| resolved.active)
    }

    pub(super) fn widget_auth_method(
        &self,
        route: WidgetRouteInfo,
    ) -> crate::tui::info_widget::AuthMethod {
        use crate::auth::ActiveCredential;
        use crate::tui::info_widget::AuthMethod;

        match route.provider {
            WidgetProviderKind::Anthropic => {
                match self
                    .dual_credential_active(route, jcode_provider_core::ActiveProvider::Claude)
                {
                    Some(ActiveCredential::OAuth) => AuthMethod::AnthropicOAuth,
                    Some(ActiveCredential::ApiKey) => AuthMethod::AnthropicApiKey,
                    None => AuthMethod::Unknown,
                }
            }
            WidgetProviderKind::OpenAI => {
                match self
                    .dual_credential_active(route, jcode_provider_core::ActiveProvider::OpenAI)
                {
                    Some(ActiveCredential::OAuth) => AuthMethod::OpenAIOAuth,
                    Some(ActiveCredential::ApiKey) => AuthMethod::OpenAIApiKey,
                    None => AuthMethod::Unknown,
                }
            }
            // Providers below have no OAuth-vs-API-key ambiguity to resolve from
            // remote credentials; remote sessions render usage via
            // `widget_usage_info`'s `is_remote` handling, so report Unknown here
            // and let the local heuristics run only for local sessions.
            _ if route.is_remote => AuthMethod::Unknown,
            WidgetProviderKind::OpenCode => crate::tui::info_widget::AuthMethod::OpenCodeApiKey,
            WidgetProviderKind::OpenRouter => {
                let runtime_provider = active_runtime_provider_key();
                let transport_state =
                    crate::provider::openrouter::OpenRouterTransportState::from_current_env(
                        runtime_provider.as_deref(),
                    );
                if transport_state.is_real_openrouter() {
                    crate::tui::info_widget::AuthMethod::OpenRouterApiKey
                } else if transport_state.accrues_user_api_key_cost() {
                    crate::tui::info_widget::AuthMethod::ApiKey
                } else {
                    crate::tui::info_widget::AuthMethod::Unknown
                }
            }
            WidgetProviderKind::CostBasedApiKey => crate::tui::info_widget::AuthMethod::ApiKey,
            WidgetProviderKind::Copilot => crate::tui::info_widget::AuthMethod::CopilotOAuth,
            WidgetProviderKind::Gemini => {
                // Per-frame: never block the render thread on a credential probe.
                let auth_status = crate::auth::AuthStatus::check_fast_nonblocking();
                if auth_status.gemini == crate::auth::AuthState::Available {
                    crate::tui::info_widget::AuthMethod::GeminiOAuth
                } else {
                    crate::tui::info_widget::AuthMethod::Unknown
                }
            }
            WidgetProviderKind::Unknown => crate::tui::info_widget::AuthMethod::Unknown,
        }
    }

    pub(super) fn widget_usage_info(
        &self,
        route: WidgetRouteInfo,
        auth_method: crate::tui::info_widget::AuthMethod,
    ) -> Option<crate::tui::info_widget::UsageInfo> {
        let output_tps = if matches!(self.status, ProcessingStatus::Streaming) {
            self.compute_streaming_tps()
        } else {
            None
        };

        // On a resumed session, `token_accounting.total_*` is reset to 0 and the
        // prior usage lives in `remote_total_tokens` (restored from history). Add
        // them so the widget's "in + out" reflects the whole session, mirroring
        // the `/cache` stats path, rather than only tokens seen since resume.
        let (display_input_tokens, display_output_tokens) =
            if let Some((hist_in, hist_out)) = self.remote_total_tokens {
                (
                    hist_in.saturating_add(self.token_accounting.total_input_tokens),
                    hist_out.saturating_add(self.token_accounting.total_output_tokens),
                )
            } else {
                (
                    self.token_accounting.total_input_tokens,
                    self.token_accounting.total_output_tokens,
                )
            };

        let cost_based_usage = || crate::tui::info_widget::UsageInfo {
            provider: crate::tui::info_widget::UsageProvider::CostBased,
            primary_limit_label: None,
            five_hour: 0.0,
            five_hour_resets_at: None,
            secondary_limit_label: None,
            seven_day: 0.0,
            seven_day_resets_at: None,
            spark: None,
            spark_resets_at: None,
            total_cost: self.cost.total_cost,
            input_tokens: display_input_tokens,
            output_tokens: display_output_tokens,
            cache_read_tokens: self.streaming.streaming_cache_read_tokens,
            cache_write_tokens: self.streaming.streaming_cache_creation_tokens,
            output_tps,
            available: true,
        };

        match route.provider {
            WidgetProviderKind::Copilot => Some(crate::tui::info_widget::UsageInfo {
                provider: crate::tui::info_widget::UsageProvider::Copilot,
                primary_limit_label: None,
                five_hour: 0.0,
                five_hour_resets_at: None,
                secondary_limit_label: None,
                seven_day: 0.0,
                seven_day_resets_at: None,
                spark: None,
                spark_resets_at: None,
                total_cost: 0.0,
                input_tokens: display_input_tokens,
                output_tokens: display_output_tokens,
                cache_read_tokens: None,
                cache_write_tokens: None,
                output_tps,
                available: display_input_tokens > 0 || display_output_tokens > 0,
            }),
            WidgetProviderKind::Anthropic => {
                match auth_method {
                    crate::tui::info_widget::AuthMethod::AnthropicApiKey => {
                        return Some(cost_based_usage());
                    }
                    crate::tui::info_widget::AuthMethod::AnthropicOAuth => {}
                    _ => return None,
                }

                let usage = crate::usage::get_sync();
                Some(crate::tui::info_widget::UsageInfo {
                    provider: crate::tui::info_widget::UsageProvider::Anthropic,
                    primary_limit_label: Some("5-hour".to_string()),
                    five_hour: usage.five_hour,
                    five_hour_resets_at: usage.five_hour_resets_at.clone(),
                    secondary_limit_label: Some("Weekly".to_string()),
                    seven_day: usage.seven_day,
                    seven_day_resets_at: usage.seven_day_resets_at.clone(),
                    spark: None,
                    spark_resets_at: None,
                    total_cost: 0.0,
                    input_tokens: 0,
                    output_tokens: 0,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    output_tps,
                    available: usage.last_error.is_none(),
                })
            }
            WidgetProviderKind::OpenAI => {
                match auth_method {
                    crate::tui::info_widget::AuthMethod::OpenAIApiKey => {
                        return Some(cost_based_usage());
                    }
                    crate::tui::info_widget::AuthMethod::OpenAIOAuth => {}
                    _ => return None,
                }

                let openai_usage = crate::usage::get_openai_usage_sync();
                Some(crate::tui::info_widget::UsageInfo {
                    provider: crate::tui::info_widget::UsageProvider::OpenAI,
                    primary_limit_label: openai_usage
                        .five_hour
                        .as_ref()
                        .map(|window| window.name.trim_end_matches(" window").to_string()),
                    five_hour: openai_usage
                        .five_hour
                        .as_ref()
                        .map(|w| w.usage_ratio)
                        .unwrap_or(0.0),
                    five_hour_resets_at: openai_usage
                        .five_hour
                        .as_ref()
                        .and_then(|w| w.resets_at.clone()),
                    secondary_limit_label: openai_usage
                        .seven_day
                        .as_ref()
                        .map(|window| window.name.trim_end_matches(" window").to_string()),
                    seven_day: openai_usage
                        .seven_day
                        .as_ref()
                        .map(|w| w.usage_ratio)
                        .unwrap_or(0.0),
                    seven_day_resets_at: openai_usage
                        .seven_day
                        .as_ref()
                        .and_then(|w| w.resets_at.clone()),
                    spark: openai_usage.spark.as_ref().map(|w| w.usage_ratio),
                    spark_resets_at: openai_usage
                        .spark
                        .as_ref()
                        .and_then(|w| w.resets_at.clone()),
                    total_cost: 0.0,
                    input_tokens: 0,
                    output_tokens: 0,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    output_tps,
                    available: openai_usage.has_limits(),
                })
            }
            WidgetProviderKind::Gemini => None,
            WidgetProviderKind::OpenRouter => {
                if route.is_remote {
                    return Some(cost_based_usage());
                }

                let runtime_provider = active_runtime_provider_key();
                let transport_state =
                    crate::provider::openrouter::OpenRouterTransportState::from_current_env(
                        runtime_provider.as_deref(),
                    );
                if transport_state.accrues_user_api_key_cost() {
                    Some(cost_based_usage())
                } else {
                    None
                }
            }
            WidgetProviderKind::OpenCode | WidgetProviderKind::CostBasedApiKey => {
                Some(cost_based_usage())
            }
            WidgetProviderKind::Unknown => None,
        }
    }
}
