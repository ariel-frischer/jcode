//! Optional workflow observation. This module has no adapter or runtime dependencies.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkflowConfig {
    pub enabled: bool,
    pub autospec_enabled: bool,
    pub show_panel: bool,
    pub poll_seconds: u64,
    pub quiet_seconds: u64,
    pub terminal_retention_seconds: u64,
    pub max_visible: usize,
}

impl Default for WorkflowConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            autospec_enabled: false,
            show_panel: true,
            poll_seconds: 2,
            quiet_seconds: 300,
            terminal_retention_seconds: 300,
            max_visible: 3,
        }
    }
}

impl WorkflowConfig {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !(1..=3600).contains(&self.poll_seconds) {
            return Err("workflow.poll_seconds must be between 1 and 3600");
        }
        if !(30..=86400).contains(&self.quiet_seconds) {
            return Err("workflow.quiet_seconds must be between 30 and 86400");
        }
        if self.terminal_retention_seconds > 86400 {
            return Err("workflow.terminal_retention_seconds must be at most 86400");
        }
        if !(1..=8).contains(&self.max_visible) {
            return Err("workflow.max_visible must be between 1 and 8");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_defaults_are_opt_in_and_bounded() {
        let config: WorkflowConfig = toml::from_str("").unwrap();
        assert!(!config.enabled);
        assert!(!config.autospec_enabled);
        assert!(config.show_panel);
        assert_eq!(config.poll_seconds, 2);
        assert_eq!(config.quiet_seconds, 300);
        assert_eq!(config.max_visible, 3);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn workflow_config_rejects_unbounded_monitoring() {
        let mut config = WorkflowConfig {
            poll_seconds: 0,
            ..WorkflowConfig::default()
        };
        assert!(config.validate().is_err());
        config.poll_seconds = 3601;
        assert!(config.validate().is_err());
        config.poll_seconds = 2;
        config.max_visible = 1000;
        assert!(config.validate().is_err());
    }

    #[test]
    fn workflow_config_round_trips_custom_settings() {
        let config: WorkflowConfig = toml::from_str(
            "enabled=true\nautospec_enabled=true\nshow_panel=false\npoll_seconds=5\nquiet_seconds=600\nterminal_retention_seconds=60\nmax_visible=2",
        ).unwrap();
        assert!(config.enabled && config.autospec_enabled);
        assert!(!config.show_panel);
        assert!(config.validate().is_ok());
        assert_eq!(
            toml::from_str::<WorkflowConfig>(&toml::to_string(&config).unwrap()).unwrap(),
            config
        );
    }
}
