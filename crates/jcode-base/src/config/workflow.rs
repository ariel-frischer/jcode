//! Canonical workflow environment overlay. Invalid overrides retain the TOML value.
use super::{Config, WorkflowConfig};

impl Config {
    pub(super) fn apply_workflow_env_overrides(&mut self) {
        for key in apply_overrides(&mut self.workflow, |key| std::env::var(key).ok()) {
            crate::logging::warn(&format!(
                "Ignoring invalid workflow environment override: {key}"
            ));
        }
        if let Err(error) = self.workflow.validate() {
            crate::logging::warn(error);
            self.workflow.enabled = false;
        }
    }
}

fn apply_overrides(
    config: &mut WorkflowConfig,
    lookup: impl Fn(&str) -> Option<String>,
) -> Vec<&'static str> {
    let mut invalid = Vec::new();
    for (key, target) in [
        ("JCODE_WORKFLOW_ENABLED", &mut config.enabled),
        (
            "JCODE_WORKFLOW_AUTOSPEC_ENABLED",
            &mut config.autospec_enabled,
        ),
        ("JCODE_WORKFLOW_SHOW_PANEL", &mut config.show_panel),
    ] {
        if let Some(raw) = lookup(key) {
            match raw.trim().to_ascii_lowercase().as_str() {
                "true" | "1" | "yes" | "on" => *target = true,
                "false" | "0" | "no" | "off" => *target = false,
                _ => invalid.push(key),
            }
        }
    }
    let mut visible = config.max_visible as u64;
    for (key, target, min, max) in [
        (
            "JCODE_WORKFLOW_POLL_SECONDS",
            &mut config.poll_seconds,
            1,
            3600,
        ),
        (
            "JCODE_WORKFLOW_QUIET_SECONDS",
            &mut config.quiet_seconds,
            30,
            86400,
        ),
        (
            "JCODE_WORKFLOW_TERMINAL_RETENTION_SECONDS",
            &mut config.terminal_retention_seconds,
            0,
            86400,
        ),
        ("JCODE_WORKFLOW_MAX_VISIBLE", &mut visible, 1, 8),
    ] {
        if let Some(raw) = lookup(key) {
            match raw.trim().parse::<u64>() {
                Ok(value) if (min..=max).contains(&value) => *target = value,
                _ => invalid.push(key),
            }
        }
    }
    config.max_visible = visible as usize;
    invalid
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_environment_overrides_toml_and_invalid_keeps_previous() {
        let mut config: WorkflowConfig = toml::from_str("enabled=true\npoll_seconds=7").unwrap();
        let invalid = apply_overrides(&mut config, |key| match key {
            "JCODE_WORKFLOW_ENABLED" => Some("false".into()),
            "JCODE_WORKFLOW_POLL_SECONDS" => Some("0".into()),
            "JCODE_WORKFLOW_MAX_VISIBLE" => Some("2".into()),
            _ => None,
        });
        assert!(!config.enabled);
        assert_eq!(config.poll_seconds, 7);
        assert_eq!(config.max_visible, 2);
        assert_eq!(invalid, vec!["JCODE_WORKFLOW_POLL_SECONDS"]);
    }
}
