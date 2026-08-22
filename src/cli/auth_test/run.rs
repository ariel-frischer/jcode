async fn maybe_run_auth_test_smoke_with_cancellation(
    report: &mut AuthTestProviderReport,
    kind: AuthTestSmokeKind,
    target: AuthTestTarget,
    model: Option<&str>,
    enabled: bool,
    prompt: &str,
    cancellation: Option<&ValidationCancellation>,
) -> Result<()> {
    if enabled && report.success && target.supports_smoke() {
        // Some providers validate chat but cannot run the tool smoke. The
        // Cursor native agent transport is text-only (no tool calls over
        // agent.v1.AgentService/Run), so skip the tool smoke with an
        // explanation instead of hanging waiting for a tool call.
        if matches!(kind, AuthTestSmokeKind::Tool)
            && matches!(target, AuthTestTarget::Cursor)
        {
            report.push_step(
                kind.step_name(),
                true,
                "Skipped: the Cursor native agent transport is text-only in jcode (it does not \
                 expose tool calls over agent.v1.AgentService/Run). Basic provider smoke still \
                 validates chat."
                    .to_string(),
            );
            return Ok(());
        }
        match run_validation_step_if_guarded(
            kind.step_name(),
            kind.run(target, model, prompt),
            cancellation,
        )
        .await
        {
            Ok(output) => {
                let ok = output.contains("AUTH_TEST_OK");
                kind.set_output(report, output.clone());
                report.push_step(
                    kind.step_name(),
                    ok,
                    if ok {
                        kind.success_detail().to_string()
                    } else {
                        kind.failure_detail(&output)
                    },
                );
            }
            Err(err) if is_validation_guard_failure(&err) => return Err(err),
            Err(err) => report.push_step(kind.step_name(), false, format!("{err:#}")),
        }
    } else if !target.supports_smoke() {
        report.push_step(kind.step_name(), true, kind.unsupported_detail());
    } else if !enabled {
        report.push_step(kind.step_name(), true, kind.skipped_by_flag_detail());
    }
    Ok(())
}

async fn maybe_run_auth_test_smoke_for_choice_with_cancellation(
    report: &mut AuthTestProviderReport,
    kind: AuthTestSmokeKind,
    choice: &super::provider_init::ProviderChoice,
    model: Option<&str>,
    enabled: bool,
    prompt: &str,
    cancellation: Option<&ValidationCancellation>,
) -> Result<()> {
    if enabled && report.success {
        match run_validation_step_if_guarded(
            kind.step_name(),
            auth_test_choice_plan(choice, model),
            cancellation,
        )
        .await
        {
            Ok(AuthTestChoicePlan::Run { model }) => {
                if matches!(kind, AuthTestSmokeKind::Tool)
                    && let Some(detail) =
                        tool_smoke_skip_detail_for_choice(choice, model.as_deref())
                {
                    report.push_step(kind.step_name(), true, detail);
                    return Ok(());
                }
                match run_validation_step_if_guarded(
                    kind.step_name(),
                    kind.run_for_choice(choice, model.as_deref(), prompt),
                    cancellation,
                )
                .await
                {
                    Ok(output) => {
                        let ok = output.contains("AUTH_TEST_OK");
                        kind.set_output(report, output.clone());
                        report.push_step(
                            kind.step_name(),
                            ok,
                            if ok {
                                kind.success_detail().to_string()
                            } else {
                                kind.failure_detail(&output)
                            },
                        );
                    }
                    Err(err) if is_validation_guard_failure(&err) => return Err(err),
                    Err(err) => report.push_step(kind.step_name(), false, format!("{err:#}")),
                }
            }
            Ok(AuthTestChoicePlan::Skip(detail)) => {
                report.push_step(kind.step_name(), true, detail);
            }
            Err(err) if is_validation_guard_failure(&err) => return Err(err),
            Err(err) => report.push_step(kind.step_name(), false, format!("{err:#}")),
        }
    } else if !enabled {
        report.push_step(kind.step_name(), true, kind.skipped_by_flag_detail());
    }
    Ok(())
}

const POST_LOGIN_VALIDATION_STEP_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug)]
enum ValidationGuardReason {
    Cancelled,
    TimedOut(Duration),
}

#[derive(Debug)]
struct ValidationGuardFailure {
    step: &'static str,
    reason: ValidationGuardReason,
}

impl std::fmt::Display for ValidationGuardFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.reason {
            ValidationGuardReason::Cancelled => write!(
                formatter,
                "Post-login validation cancelled during `{}`. Credentials were saved; Ctrl+C was received and saved credentials were kept.",
                self.step
            ),
            ValidationGuardReason::TimedOut(timeout) => write!(
                formatter,
                "Post-login validation timed out during `{}` after {} seconds. Credentials were saved; re-run `jcode auth-test` to retry.",
                self.step,
                timeout.as_secs()
            ),
        }
    }
}

impl std::error::Error for ValidationGuardFailure {}

struct ValidationCancellation {
    notify: std::sync::Arc<tokio::sync::Notify>,
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    watcher: tokio::task::JoinHandle<()>,
}

impl ValidationCancellation {
    fn start() -> Self {
        let notify = std::sync::Arc::new(tokio::sync::Notify::new());
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let signal = std::sync::Arc::clone(&notify);
        let signal_state = std::sync::Arc::clone(&cancelled);
        let watcher = tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                signal_state.store(true, std::sync::atomic::Ordering::Release);
                signal.notify_waiters();
            }
        });
        Self {
            notify,
            cancelled,
            watcher,
        }
    }

    async fn cancelled(&self) {
        while !self
            .cancelled
            .load(std::sync::atomic::Ordering::Acquire)
        {
            self.notify.notified().await;
        }
    }
}

impl Drop for ValidationCancellation {
    fn drop(&mut self) {
        self.watcher.abort();
    }
}

async fn run_validation_step<T, F>(
    step: &'static str,
    timeout: Duration,
    operation: F,
    cancellation: impl std::future::Future<Output = ()>,
) -> Result<T>
where
    F: std::future::Future<Output = Result<T>>,
{
    match tokio::time::timeout(
        timeout,
        async {
            tokio::select! {
                biased;
                _ = cancellation => Err(anyhow::Error::new(ValidationGuardFailure {
                    step,
                    reason: ValidationGuardReason::Cancelled,
                })),
                result = operation => result,
            }
        },
    )
    .await
    {
        Ok(result) => result,
        Err(_) => Err(anyhow::Error::new(ValidationGuardFailure {
            step,
            reason: ValidationGuardReason::TimedOut(timeout),
        })),
    }
}

async fn run_validation_step_if_guarded<T, F>(
    step: &'static str,
    operation: F,
    cancellation: Option<&ValidationCancellation>,
) -> Result<T>
where
    F: std::future::Future<Output = Result<T>>,
{
    match cancellation {
        Some(cancellation) => {
            run_validation_step(
                step,
                POST_LOGIN_VALIDATION_STEP_TIMEOUT,
                operation,
                cancellation.cancelled(),
            )
            .await
        }
        None => operation.await,
    }
}

fn is_validation_guard_failure(error: &anyhow::Error) -> bool {
    error.downcast_ref::<ValidationGuardFailure>().is_some()
}

pub(crate) async fn run_post_login_validation(
    provider: crate::provider_catalog::LoginProviderDescriptor,
) -> Result<()> {
    run_post_login_validation_inner(provider, true).await
}

pub(crate) async fn run_post_login_validation_quiet(
    provider: crate::provider_catalog::LoginProviderDescriptor,
) -> Result<()> {
    run_post_login_validation_inner(provider, false).await
}

async fn run_post_login_validation_inner(
    provider: crate::provider_catalog::LoginProviderDescriptor,
    verbose: bool,
) -> Result<()> {
    let Some(choice) = super::provider_init::choice_for_login_provider(provider) else {
        crate::logging::auth_event(
            "post_login_validation_skipped",
            provider.id,
            &[("reason", "no_runtime_provider_choice")],
        );
        if verbose {
            eprintln!(
                "\nSkipping automatic runtime validation for {}. Auto Import can add multiple providers; run `jcode auth-test --all-configured` to validate them.",
                provider.display_name
            );
        }
        return Ok(());
    };

    super::provider_init::apply_login_provider_profile_env(provider);
    crate::logging::auth_event(
        "post_login_validation_started",
        provider.id,
        &[("choice", choice.as_arg_value())],
    );

    if verbose {
        eprintln!(
            "\nValidating {} login with live auth/runtime checks...",
            provider.display_name
        );
    }

    let cancellation = ValidationCancellation::start();

    let report = if let Some(target) = AuthTestTarget::from_provider_choice(&choice) {
        populate_auth_test_target_report_with_cancellation(
            target,
            None,
            true,
            true,
            DEFAULT_AUTH_TEST_PROVIDER_PROMPT,
            DEFAULT_AUTH_TEST_TOOL_PROMPT,
            AuthTestProviderReport::new(target),
            Some(&cancellation),
        )
        .await?
    } else {
        populate_generic_auth_test_report_with_cancellation(
            provider,
            choice,
            None,
            true,
            true,
            DEFAULT_AUTH_TEST_PROVIDER_PROMPT,
            DEFAULT_AUTH_TEST_TOOL_PROMPT,
            AuthTestProviderReport::new_generic(
                choice.as_arg_value().to_string(),
                generic_credential_paths_for_provider(provider),
            ),
            Some(&cancellation),
        )
        .await?
    };

    persist_auth_test_report(&report, None);
    let step_count = report.steps.len().to_string();
    crate::logging::auth_event(
        "post_login_validation_completed",
        provider.id,
        &[
            ("choice", choice.as_arg_value()),
            ("success", if report.success { "true" } else { "false" }),
            ("steps", step_count.as_str()),
        ],
    );
    if verbose {
        print_auth_test_reports(std::slice::from_ref(&report));
    }

    if report.success {
        Ok(())
    } else if AuthTestTarget::from_provider_choice(&choice).is_some() {
        anyhow::bail!(
            "Post-login validation failed for {}. Credentials were saved, but jcode could not verify runtime readiness. Re-run `jcode auth-test --provider {}` for details.",
            provider.display_name,
            choice.as_arg_value()
        )
    } else {
        anyhow::bail!(
            "Post-login validation failed for {}. Credentials were saved, but jcode could not verify runtime readiness. Re-test with `jcode --provider {} run \"Reply with exactly AUTH_TEST_OK and nothing else.\"` after fixing the provider/runtime.",
            provider.display_name,
            choice.as_arg_value()
        )
    }
}

pub fn run_auth_test_coverage_command(
    emit_json: bool,
    output_path: Option<&str>,
    coverage_path: Option<&str>,
    gap_limit: usize,
) -> Result<()> {
    let coverage_path = coverage_path.map(std::path::Path::new);
    let (coverage, path) = crate::live_tests::load_coverage(coverage_path)?;
    let summary = crate::live_tests::strict_live_provider_model_coverage_summary(
        &coverage,
        path.display().to_string(),
    );

    if emit_json || output_path.is_some() {
        let json = serde_json::to_string_pretty(&summary)?;
        if let Some(path) = output_path {
            std::fs::write(path, &json)
                .with_context(|| format!("failed to write auth-test coverage report to {path}"))?;
        }
        if emit_json {
            println!("{json}");
        }
    } else {
        print!(
            "{}",
            crate::live_tests::format_strict_live_provider_model_coverage_summary(
                &summary, gap_limit,
            )
        );
    }

    Ok(())
}

pub async fn run_auth_test_context_audit_command(
    choice: &super::provider_init::ProviderChoice,
    all_configured: bool,
    emit_json: bool,
    output_path: Option<&str>,
) -> Result<()> {
    let targets = resolve_auth_test_targets(choice, all_configured)?;
    let mut reports = Vec::new();

    for target in targets {
        reports.push(run_context_audit_for_target(target).await);
    }

    let report_json = (emit_json || output_path.is_some())
        .then(|| serde_json::to_string_pretty(&reports))
        .transpose()?;

    if let Some(path) = output_path {
        std::fs::write(path, report_json.as_deref().unwrap_or("[]"))
            .with_context(|| format!("failed to write auth-test context audit report to {path}"))?;
    }

    if emit_json {
        println!("{}", report_json.as_deref().unwrap_or("[]"));
    } else {
        print_context_audit_reports(&reports);
    }

    if reports.iter().all(|report| report.success) {
        Ok(())
    } else {
        anyhow::bail!("One or more live context audits failed")
    }
}

async fn run_context_audit_for_target(
    target: ResolvedAuthTestTarget,
) -> AuthTestContextAuditReport {
    let (provider_id, display_name, supports_openrouter_catalog) = match target {
        ResolvedAuthTestTarget::Generic { provider, choice } => {
            super::provider_init::apply_login_provider_profile_env(provider);
            let supports_openrouter_catalog = matches!(
                provider.target,
                crate::provider_catalog::LoginProviderTarget::OpenRouter
                    | crate::provider_catalog::LoginProviderTarget::OpenAiCompatible(_)
            );
            (
                choice.as_arg_value().to_string(),
                provider.display_name.to_string(),
                supports_openrouter_catalog,
            )
        }
        ResolvedAuthTestTarget::Detailed(target) => (
            target.label().to_string(),
            target.label().to_string(),
            false,
        ),
    };

    if !supports_openrouter_catalog {
        return AuthTestContextAuditReport {
            provider: provider_id,
            display_name,
            checked_models: 0,
            skipped_models_without_context: 0,
            mismatches: Vec::new(),
            success: true,
            detail:
                "Skipped: provider does not use the OpenRouter/OpenAI-compatible live catalog path."
                    .to_string(),
        };
    }

    audit_openrouter_context_windows(provider_id, display_name).await
}

async fn audit_openrouter_context_windows(
    provider_id: String,
    display_name: String,
) -> AuthTestContextAuditReport {
    use crate::provider::Provider as _;

    let provider = match jcode_provider_openrouter_runtime::OpenRouterProvider::new() {
        Ok(provider) => provider,
        Err(err) => {
            return AuthTestContextAuditReport {
                provider: provider_id,
                display_name,
                checked_models: 0,
                skipped_models_without_context: 0,
                mismatches: Vec::new(),
                success: false,
                detail: format!("Failed to initialize provider: {err:#}"),
            };
        }
    };

    let models = match provider.refresh_models().await {
        Ok(models) => models,
        Err(err) => {
            return AuthTestContextAuditReport {
                provider: provider_id,
                display_name,
                checked_models: 0,
                skipped_models_without_context: 0,
                mismatches: Vec::new(),
                success: false,
                detail: format!("Failed to fetch live model catalog: {err:#}"),
            };
        }
    };

    let mut checked_models = 0usize;
    let mut skipped_models_without_context = 0usize;
    let mut mismatches = Vec::new();

    for model in models {
        let Some(catalog_context_window) = model.context_length.map(|value| value as usize) else {
            skipped_models_without_context += 1;
            continue;
        };
        checked_models += 1;

        if let Err(err) = provider.set_model(&model.id) {
            mismatches.push(AuthTestContextModelReport {
                model: model.id,
                catalog_context_window,
                resolved_context_window: 0,
                ok: false,
            });
            crate::logging::info(&format!(
                "live context audit could not switch model for {}: {err:#}",
                provider_id
            ));
            continue;
        }

        let resolved_context_window = provider.context_window();
        if resolved_context_window != catalog_context_window {
            mismatches.push(AuthTestContextModelReport {
                model: model.id,
                catalog_context_window,
                resolved_context_window,
                ok: false,
            });
        }
    }

    let success = mismatches.is_empty();
    let detail = if success {
        format!(
            "Checked {checked_models} live catalog models with context metadata; skipped {skipped_models_without_context} without context metadata."
        )
    } else {
        format!(
            "Found {} context-window mismatches across {checked_models} live catalog models with context metadata; skipped {skipped_models_without_context} without context metadata.",
            mismatches.len()
        )
    };

    AuthTestContextAuditReport {
        provider: provider_id,
        display_name,
        checked_models,
        skipped_models_without_context,
        mismatches,
        success,
        detail,
    }
}

fn print_context_audit_reports(reports: &[AuthTestContextAuditReport]) {
    for report in reports {
        println!("{} ({})", report.display_name, report.provider);
        println!("  success: {}", report.success);
        println!("  {}", report.detail);
        for mismatch in report.mismatches.iter().take(20) {
            println!(
                "  mismatch: {} catalog={} resolved={}",
                mismatch.model, mismatch.catalog_context_window, mismatch.resolved_context_window
            );
        }
        if report.mismatches.len() > 20 {
            println!("  ... {} more mismatches", report.mismatches.len() - 20);
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "CLI auth-test entrypoint maps directly from command-line flags"
)]
pub async fn run_auth_test_command(
    choice: &super::provider_init::ProviderChoice,
    model: Option<&str>,
    login: bool,
    all_configured: bool,
    no_smoke: bool,
    no_tool_smoke: bool,
    prompt: Option<&str>,
    emit_json: bool,
    output_path: Option<&str>,
) -> Result<()> {
    let targets = resolve_auth_test_targets(choice, all_configured)?;
    let provider_smoke_prompt = prompt.unwrap_or(DEFAULT_AUTH_TEST_PROVIDER_PROMPT);
    let tool_smoke_prompt = prompt.unwrap_or(DEFAULT_AUTH_TEST_TOOL_PROMPT);

    let mut reports = Vec::new();
    for target in targets {
        let report = match target {
            ResolvedAuthTestTarget::Detailed(target) => {
                run_auth_test_target(
                    target,
                    model,
                    login,
                    !no_smoke,
                    !no_tool_smoke,
                    provider_smoke_prompt,
                    tool_smoke_prompt,
                )
                .await
            }
            ResolvedAuthTestTarget::Generic { provider, choice } => {
                let mut report = AuthTestProviderReport::new_generic(
                    choice.as_arg_value().to_string(),
                    generic_credential_paths_for_provider(provider),
                );
                if login {
                    match super::login::run_login(
                        &choice,
                        None,
                        super::login::LoginOptions::default(),
                    )
                    .await
                    {
                        Ok(()) => report.push_step("login", true, "Login flow completed."),
                        Err(err) => report.push_step("login", false, err.to_string()),
                    }
                }
                populate_generic_auth_test_report(
                    provider,
                    choice,
                    model,
                    !no_smoke,
                    !no_tool_smoke,
                    provider_smoke_prompt,
                    tool_smoke_prompt,
                    report,
                )
                .await
            }
        };
        persist_auth_test_report(&report, model);
        reports.push(report);
    }

    let report_json = (emit_json || output_path.is_some())
        .then(|| serde_json::to_string_pretty(&reports))
        .transpose()?;

    if let Some(path) = output_path {
        std::fs::write(path, report_json.as_deref().unwrap_or("[]"))
            .with_context(|| format!("failed to write auth-test report to {}", path))?;
    }

    if emit_json {
        println!("{}", report_json.as_deref().unwrap_or("[]"));
    } else {
        print_auth_test_reports(&reports);
    }

    if reports.iter().all(|report| report.success) {
        Ok(())
    } else {
        anyhow::bail!("One or more auth tests failed")
    }
}

pub(crate) fn resolve_auth_test_targets(
    choice: &super::provider_init::ProviderChoice,
    all_configured: bool,
) -> Result<Vec<ResolvedAuthTestTarget>> {
    if all_configured || matches!(choice, super::provider_init::ProviderChoice::Auto) {
        // Auth-test discovery must not run slow or blocking provider-global probes.
        // Generic OpenAI-compatible providers only need local env/config detection,
        // and detailed providers perform their own provider-specific checks later.
        let status = crate::auth::AuthStatus::check_fast();
        let targets = configured_auth_test_targets(&status);
        if targets.is_empty() {
            anyhow::bail!(
                "No configured supported auth providers found. Run `jcode login --provider <provider>` first, or choose an explicit --provider."
            );
        }
        return Ok(targets);
    }

    ResolvedAuthTestTarget::from_choice(choice)
        .map(|target| vec![target])
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Provider '{}' is not yet supported by `jcode auth-test`.",
                choice.as_arg_value()
            )
        })
}

pub(crate) fn configured_auth_test_targets(
    status: &crate::auth::AuthStatus,
) -> Vec<ResolvedAuthTestTarget> {
    crate::provider_catalog::auth_status_login_providers()
        .into_iter()
        .filter(|provider| status.assessment_for_provider(*provider).is_configured())
        .filter_map(ResolvedAuthTestTarget::from_provider)
        .collect()
}

async fn run_auth_test_target(
    target: AuthTestTarget,
    model: Option<&str>,
    login: bool,
    run_smoke: bool,
    run_tool_smoke: bool,
    provider_smoke_prompt: &str,
    tool_smoke_prompt: &str,
) -> AuthTestProviderReport {
    let mut report = AuthTestProviderReport::new(target);

    if login {
        match super::login::run_login(
            &target.provider_choice(),
            None,
            super::login::LoginOptions::default(),
        )
        .await
        {
            Ok(()) => report.push_step("login", true, "Login flow completed."),
            Err(err) => report.push_step("login", false, err.to_string()),
        }
    }

    populate_auth_test_target_report(
        target,
        model,
        run_smoke,
        run_tool_smoke,
        provider_smoke_prompt,
        tool_smoke_prompt,
        report,
    )
    .await
}

async fn populate_auth_test_target_report(
    target: AuthTestTarget,
    model: Option<&str>,
    run_smoke: bool,
    run_tool_smoke: bool,
    provider_smoke_prompt: &str,
    tool_smoke_prompt: &str,
    report: AuthTestProviderReport,
) -> AuthTestProviderReport {
    populate_auth_test_target_report_with_cancellation(
        target,
        model,
        run_smoke,
        run_tool_smoke,
        provider_smoke_prompt,
        tool_smoke_prompt,
        report,
        None,
    )
    .await
    .expect("validation without cancellation cannot fail")
}

#[expect(
    clippy::too_many_arguments,
    reason = "Auth-test validation carries explicit smoke, prompt, report, and cancellation controls"
)]
async fn populate_auth_test_target_report_with_cancellation(
    target: AuthTestTarget,
    model: Option<&str>,
    run_smoke: bool,
    run_tool_smoke: bool,
    provider_smoke_prompt: &str,
    tool_smoke_prompt: &str,
    mut report: AuthTestProviderReport,
    cancellation: Option<&ValidationCancellation>,
) -> Result<AuthTestProviderReport> {
    run_validation_step_if_guarded(
        "credential_probe",
        async {
            match target {
                AuthTestTarget::Claude => probe_claude_auth(&mut report).await,
                AuthTestTarget::Openai => probe_openai_auth(&mut report).await,
                AuthTestTarget::Gemini => probe_gemini_auth(&mut report).await,
                AuthTestTarget::Antigravity => probe_antigravity_auth(&mut report).await,
                AuthTestTarget::Google => probe_google_auth(&mut report).await,
                AuthTestTarget::Copilot => probe_copilot_auth(&mut report).await,
                AuthTestTarget::Cursor => probe_cursor_auth(&mut report).await,
            }
            Ok(())
        },
        cancellation,
    )
    .await?;
    maybe_run_auth_test_smoke_with_cancellation(
        &mut report,
        AuthTestSmokeKind::Provider,
        target,
        model,
        run_smoke,
        provider_smoke_prompt,
        cancellation,
    )
    .await?;
    maybe_run_auth_test_smoke_with_cancellation(
        &mut report,
        AuthTestSmokeKind::Tool,
        target,
        model,
        run_tool_smoke,
        tool_smoke_prompt,
        cancellation,
    )
    .await?;
    Ok(report)
}

#[expect(
    clippy::too_many_arguments,
    reason = "Auth-test helper carries explicit smoke and prompt controls until structured options land"
)]
async fn populate_generic_auth_test_report(
    provider: crate::provider_catalog::LoginProviderDescriptor,
    choice: super::provider_init::ProviderChoice,
    model: Option<&str>,
    run_smoke: bool,
    run_tool_smoke: bool,
    provider_smoke_prompt: &str,
    tool_smoke_prompt: &str,
    report: AuthTestProviderReport,
) -> AuthTestProviderReport {
    populate_generic_auth_test_report_with_cancellation(
        provider,
        choice,
        model,
        run_smoke,
        run_tool_smoke,
        provider_smoke_prompt,
        tool_smoke_prompt,
        report,
        None,
    )
    .await
    .expect("validation without cancellation cannot fail")
}

#[expect(
    clippy::too_many_arguments,
    reason = "Post-login helper mirrors existing auth-test controls while adding cancellation"
)]
async fn populate_generic_auth_test_report_with_cancellation(
    provider: crate::provider_catalog::LoginProviderDescriptor,
    choice: super::provider_init::ProviderChoice,
    model: Option<&str>,
    run_smoke: bool,
    run_tool_smoke: bool,
    provider_smoke_prompt: &str,
    tool_smoke_prompt: &str,
    mut report: AuthTestProviderReport,
    cancellation: Option<&ValidationCancellation>,
) -> Result<AuthTestProviderReport> {
    super::provider_init::apply_login_provider_profile_env(provider);
    probe_generic_provider_auth(provider, &mut report);
    maybe_run_auth_test_smoke_for_choice_with_cancellation(
        &mut report,
        AuthTestSmokeKind::Provider,
        &choice,
        model,
        run_smoke,
        provider_smoke_prompt,
        cancellation,
    )
    .await?;
    maybe_run_auth_test_smoke_for_choice_with_cancellation(
        &mut report,
        AuthTestSmokeKind::Tool,
        &choice,
        model,
        run_tool_smoke,
        tool_smoke_prompt,
        cancellation,
    )
    .await?;
    Ok(report)
}

fn persist_auth_test_report(report: &AuthTestProviderReport, model: Option<&str>) {
    let step_map = report
        .steps
        .iter()
        .map(|step| (step.name.as_str(), step.ok))
        .collect::<HashMap<_, _>>();
    let summary = report
        .steps
        .iter()
        .find(|step| !step.ok)
        .map(|step| format!("{}: {}", step.name, step.detail))
        .or_else(|| {
            report
                .steps
                .last()
                .map(|step| format!("{}: {}", step.name, step.detail))
        })
        .unwrap_or_else(|| "No validation steps recorded.".to_string());

    let record = crate::auth::validation::ProviderValidationRecord {
        checked_at_ms: chrono::Utc::now().timestamp_millis(),
        success: report.success,
        provider_smoke_ok: step_map.get("provider_smoke").copied(),
        tool_smoke_ok: step_map.get("tool_smoke").copied(),
        summary,
    };

    if let Err(err) = crate::auth::validation::save(&report.provider, record) {
        crate::logging::warn(&format!(
            "failed to persist auth validation result for {}: {}",
            report.provider, err
        ));
    }

    if let Err(err) = persist_auth_test_live_verification_event(report, model) {
        crate::logging::warn(&format!(
            "failed to persist auth-test live verification event for {}: {}",
            report.provider, err
        ));
    }
}

fn persist_auth_test_live_verification_event(
    report: &AuthTestProviderReport,
    model: Option<&str>,
) -> Result<()> {
    let mut stages = Vec::new();
    let mut expected = Vec::new();
    let mut capabilities = Vec::new();

    for step in &report.steps {
        match step.name.as_str() {
            "credential_probe" => {
                expected.push(crate::live_tests::checkpoints::AUTH_CREDENTIAL_LOADED);
                stages.push(auth_test_step_stage(
                    crate::live_tests::checkpoints::AUTH_CREDENTIAL_LOADED,
                    step,
                ));
            }
            "provider_smoke" => {
                capabilities.push("provider_smoke");
                capabilities.push("non_streaming_chat_completion");
                expected.push(crate::live_tests::checkpoints::NON_STREAMING_CHAT_COMPLETION);
                stages.push(auth_test_step_stage(
                    crate::live_tests::checkpoints::NON_STREAMING_CHAT_COMPLETION,
                    step,
                ));
            }
            "tool_smoke" => {
                let tool_smoke_skipped = auth_test_step_is_skipped(step);
                if !tool_smoke_skipped {
                    capabilities.push("real_jcode_tool_smoke");
                }
                expected.push(crate::live_tests::checkpoints::TOOL_CALL_PARSE);
                expected.push(crate::live_tests::checkpoints::TOOL_EXECUTION_LOOP);
                expected.push(crate::live_tests::checkpoints::TOOL_RESULT_FOLLOWUP);
                expected.push(crate::live_tests::checkpoints::REAL_JCODE_TOOL_SMOKE);
                let stage = auth_test_step_stage(
                    crate::live_tests::checkpoints::REAL_JCODE_TOOL_SMOKE,
                    step,
                )
                .with_evidence("tool_name", serde_json::json!(AUTH_TEST_TOOL_NAME))
                .with_evidence("tool_command", serde_json::json!(AUTH_TEST_TOOL_COMMAND));
                stages.push(stage.clone());
                stages.push(auth_test_tool_derived_stage(
                    crate::live_tests::checkpoints::TOOL_CALL_PARSE,
                    step,
                ));
                stages.push(auth_test_tool_derived_stage(
                    crate::live_tests::checkpoints::TOOL_EXECUTION_LOOP,
                    step,
                ));
                stages.push(auth_test_tool_derived_stage(
                    crate::live_tests::checkpoints::TOOL_RESULT_FOLLOWUP,
                    step,
                ));
            }
            _ => {}
        }
    }

    if expected.is_empty() {
        return Ok(());
    }

    let result = if report.success {
        crate::live_tests::LiveVerificationResult::Passed
    } else {
        crate::live_tests::LiveVerificationResult::Failed
    };
    let (coverage_provider_id, coverage_provider_label) =
        auth_test_coverage_provider_identity(report);
    let mut event = crate::live_tests::LiveVerificationEvent::new(
        "auth_test_real_jcode_runtime",
        coverage_provider_id,
        coverage_provider_label,
        crate::live_tests::LiveVerificationAuth::non_secret("auth-test", None::<String>),
        result,
    )
    .with_expected_checkpoints(expected)
    .with_capabilities(capabilities)
    .with_stages(stages)
    .with_metadata(
        "checkpoint_taxonomy_version",
        serde_json::json!(crate::live_tests::CHECKPOINT_TAXONOMY_VERSION),
    )
    .with_metadata("auth_test_steps", serde_json::json!(report.steps));
    if let Some(model) = model.map(str::trim).filter(|model| !model.is_empty()) {
        event = event.with_model(model.to_string());
    }
    crate::live_tests::append_event(&event)?;
    Ok(())
}

fn auth_test_coverage_provider_identity(report: &AuthTestProviderReport) -> (String, String) {
    if report.provider == "openai-compatible"
        && let Ok(profile_name) = std::env::var("JCODE_NAMED_PROVIDER_PROFILE")
    {
        let profile_name = profile_name.trim();
        if !profile_name.is_empty() {
            let label = crate::config::config()
                .providers
                .get(profile_name)
                .map(|profile| {
                    format!(
                        "{} (custom OpenAI-compatible: {})",
                        profile_name, profile.base_url
                    )
                })
                .unwrap_or_else(|| format!("{} (custom OpenAI-compatible)", profile_name));
            return (profile_name.to_string(), label);
        }
    }

    (report.provider.clone(), report.provider.clone())
}

fn auth_test_step_stage(
    checkpoint: &'static str,
    step: &AuthTestStepReport,
) -> crate::live_tests::LiveVerificationStage {
    let status = if auth_test_step_is_skipped(step) {
        crate::live_tests::LiveVerificationStageStatus::Skipped
    } else if step.ok {
        crate::live_tests::LiveVerificationStageStatus::Passed
    } else {
        crate::live_tests::LiveVerificationStageStatus::Failed
    };
    crate::live_tests::LiveVerificationStage::new(checkpoint, status)
        .with_evidence("auth_test_step", serde_json::json!(step.name))
        .with_evidence("detail", serde_json::json!(step.detail))
}

fn auth_test_step_is_skipped(step: &AuthTestStepReport) -> bool {
    step.detail.trim_start().starts_with("Skipped:")
}

#[cfg(test)]
mod validation_guard_tests {
    use super::*;
    use std::future;
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn cancellation_returns_an_actionable_error_for_the_current_step() {
        let (cancel_tx, cancel_rx) = oneshot::channel();
        let task = tokio::spawn(run_validation_step(
            "provider_smoke",
            Duration::from_secs(1),
            future::pending::<Result<()>>(),
            async move { cancel_rx.await.expect("cancellation sender dropped") },
        ));

        cancel_tx.send(()).expect("cancellation receiver is alive");
        let error = task
            .await
            .expect("validation task should finish")
            .expect_err("cancellation should fail validation");
        let message = format!("{error:#}");
        assert!(message.contains("provider_smoke"), "missing step: {message}");
        assert!(message.contains("Ctrl+C"), "missing cancellation hint: {message}");
        assert!(
            message.to_ascii_lowercase().contains("credentials were saved"),
            "missing persistence hint: {message}"
        );
    }

    #[tokio::test]
    async fn timeout_returns_an_actionable_error_for_the_current_step() {
        let error = run_validation_step(
            "tool_smoke",
            Duration::from_millis(5),
            future::pending::<Result<()>>(),
            future::pending::<()>(),
        )
        .await
        .expect_err("hung validation should time out");
        let message = format!("{error:#}");
        assert!(message.contains("tool_smoke"), "missing step: {message}");
        assert!(message.contains("timed out"), "missing timeout detail: {message}");
        assert!(
            message.to_ascii_lowercase().contains("credentials were saved"),
            "missing persistence hint: {message}"
        );
    }
}

fn auth_test_tool_derived_stage(
    checkpoint: &'static str,
    step: &AuthTestStepReport,
) -> crate::live_tests::LiveVerificationStage {
    auth_test_step_stage(checkpoint, step).with_evidence(
        "derived_from",
        serde_json::json!(crate::live_tests::checkpoints::REAL_JCODE_TOOL_SMOKE),
    )
}
