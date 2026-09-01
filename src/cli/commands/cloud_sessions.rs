use super::CloudSessionsSubcommand;

pub(super) fn cloud_sessions_helper_override(action: &CloudSessionsSubcommand) -> Option<String> {
    match action {
        CloudSessionsSubcommand::Configure { .. }
        | CloudSessionsSubcommand::Status { .. }
        | CloudSessionsSubcommand::Sync { .. } => None,
        CloudSessionsSubcommand::Upload { helper, .. }
        | CloudSessionsSubcommand::UploadLatest { helper, .. }
        | CloudSessionsSubcommand::List { helper, .. }
        | CloudSessionsSubcommand::Verify { helper, .. }
        | CloudSessionsSubcommand::Dashboard { helper, .. }
        | CloudSessionsSubcommand::View { helper, .. } => helper.clone(),
    }
}

pub(super) fn append_common_jade_args(
    args: &mut Vec<String>,
    user_id: String,
    profile: Option<String>,
    region: Option<String>,
) {
    args.extend(["--user-id".to_string(), user_id]);
    if let Some(profile) = profile {
        args.extend(["--profile".to_string(), profile]);
    }
    if let Some(region) = region {
        args.extend(["--region".to_string(), region]);
    }
}
