use super::*;

#[test]
fn weekly_only_subscription_credit_uses_matching_bar_and_label() {
    let usage = UsageInfo {
        provider: UsageProvider::OpenAI,
        secondary_limit_label: Some("7-day".to_string()),
        seven_day: 0.3,
        available: true,
        ..Default::default()
    };
    let credit = ProviderCreditState::from_usage_info("OpenAI", Some(&usage), false, false, false);
    assert_eq!(credit.summary().as_deref(), Some("7-day 70% left"));
    assert_eq!(credit.primary_remaining_ratio, Some(0.7));
    assert_eq!(credit.primary_window_label.as_deref(), Some("7-day"));
    let field = TopBarContext::new("dolphin", credit)
        .fields()
        .into_iter()
        .find(|field| field.kind == TopBarFieldKind::Credit)
        .expect("credit field");
    assert_eq!(line_text(&field.full), "OpenAI 7-day ▰▰▰▰▰▰▰▱▱▱ 70% left");
    assert_eq!(line_text(field.compact.as_ref().unwrap()), "▰▰▰▰▱▱");
}

#[test]
fn subscription_credit_keeps_primary_precedence_and_rejects_invalid_windows() {
    for provider in [UsageProvider::OpenAI, UsageProvider::Anthropic] {
        for (primary, expected_ratio, expected_label) in [
            (0.2, 0.8, "5-hour"),
            (0.0, 1.0, "5-hour"),
            (f32::NAN, 0.7, "7-day"),
            (f32::INFINITY, 0.7, "7-day"),
            (-0.1, 0.7, "7-day"),
            (1.1, 0.7, "7-day"),
        ] {
            let usage = UsageInfo {
                provider,
                primary_limit_label: Some("5-hour".to_string()),
                secondary_limit_label: Some("7-day".to_string()),
                five_hour: primary,
                seven_day: 0.3,
                available: true,
                ..Default::default()
            };
            let credit =
                ProviderCreditState::from_usage_info("provider", Some(&usage), true, false, false);
            assert_eq!(credit.status, ProviderCreditStatus::Stale);
            assert_eq!(credit.primary_remaining_ratio, Some(expected_ratio));
            assert_eq!(credit.primary_window_label.as_deref(), Some(expected_label));
        }
    }
}

#[test]
fn subscription_credit_does_not_invent_missing_quota() {
    let mut usage = UsageInfo {
        provider: UsageProvider::OpenAI,
        available: true,
        ..Default::default()
    };
    let credit = ProviderCreditState::from_usage_info("OpenAI", Some(&usage), false, false, false);
    assert_eq!(credit.status, ProviderCreditStatus::Unavailable);
    assert_eq!(credit.primary_remaining_ratio, None);

    usage.secondary_limit_label = Some("7-day".to_string());
    let credit = ProviderCreditState::from_usage_info("OpenAI", Some(&usage), false, false, false);
    assert_eq!(credit.primary_remaining_ratio, Some(1.0));
    assert_eq!(credit.primary_window_label.as_deref(), Some("7-day"));
}
