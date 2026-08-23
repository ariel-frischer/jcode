#[expect(
    clippy::too_many_arguments,
    reason = "Auth-test validation carries explicit smoke, prompt, report, and cancellation controls"
)]
pub(super) async fn populate_auth_test_target_report_with_cancellation(
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

