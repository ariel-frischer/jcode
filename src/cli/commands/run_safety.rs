use anyhow::{Result, bail};
use serde::Serialize;
use std::num::NonZeroU64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RunStopReason {
    MaxTurnsReached,
}

impl RunStopReason {
    pub(super) fn code(self) -> &'static str {
        match self {
            Self::MaxTurnsReached => "max_turns_reached",
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::MaxTurnsReached => "maximum turns reached",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub(super) struct RunSafetyBound {
    bound: &'static str,
    source: &'static str,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub(super) struct RunStopMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety_bound: Option<RunSafetyBound>,
}

impl RunStopMetadata {
    pub(super) fn from_reason(reason: Option<RunStopReason>) -> Self {
        match reason {
            Some(reason) => Self {
                stop_reason: Some(reason.code()),
                outcome: Some("bounded_stop"),
                safety_bound: Some(RunSafetyBound {
                    bound: "max_turns",
                    source: "invocation",
                }),
            },
            None => Self::default(),
        }
    }
}

#[derive(Debug)]
pub(super) struct RunTurnLimit {
    max_turns: Option<NonZeroU64>,
    completed_turns: u64,
    stop_reason: Option<RunStopReason>,
}

impl RunTurnLimit {
    pub(super) fn parse(raw: Option<&str>) -> Result<Self> {
        let max_turns = match raw {
            Some(raw) => {
                let value = raw.trim().parse::<u64>().ok().and_then(NonZeroU64::new);
                let Some(value) = value else {
                    bail!("--max-turns must be a positive decimal whole number");
                };
                Some(value)
            }
            None => None,
        };
        Ok(Self {
            max_turns,
            completed_turns: 0,
            stop_reason: None,
        })
    }

    #[cfg(test)]
    fn max_turns(&self) -> Option<NonZeroU64> {
        self.max_turns
    }

    #[cfg(test)]
    fn completed_turns(&self) -> u64 {
        self.completed_turns
    }

    pub(super) fn complete_turn_and_should_stop(&mut self) -> bool {
        self.completed_turns = self.completed_turns.saturating_add(1);
        if self
            .max_turns
            .is_some_and(|max_turns| self.completed_turns >= max_turns.get())
        {
            self.stop_reason = Some(RunStopReason::MaxTurnsReached);
            true
        } else {
            false
        }
    }

    pub(super) fn stop_reason(&self) -> Option<RunStopReason> {
        self.stop_reason
    }
}

pub(super) fn print_plain_stop(reason: Option<RunStopReason>) {
    if let Some(message) = plain_stop_message(reason) {
        println!("{message}");
    }
}

fn plain_stop_message(reason: Option<RunStopReason>) -> Option<String> {
    reason.map(|reason| format!("Run stopped: {} ({})", reason.label(), reason.code()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_turns_requires_a_positive_decimal_whole_number() {
        for value in ["", " ", "0", "-1", "1.5", "many"] {
            let error = RunTurnLimit::parse(Some(value)).expect_err("invalid limit must fail");
            assert!(error.to_string().contains("positive decimal whole number"));
            assert!(error.to_string().contains("--max-turns"));
        }

        let limit = RunTurnLimit::parse(Some(" 3 ")).expect("valid limit should parse");
        assert_eq!(limit.max_turns().map(std::num::NonZeroU64::get), Some(3));
    }

    #[test]
    fn max_turns_stops_after_the_configured_completed_turn() {
        let mut limit = RunTurnLimit::parse(Some("2")).expect("limit should parse");

        assert!(!limit.complete_turn_and_should_stop());
        assert_eq!(limit.stop_reason(), None);
        assert!(limit.complete_turn_and_should_stop());
        assert_eq!(limit.stop_reason(), Some(RunStopReason::MaxTurnsReached));
        assert_eq!(limit.completed_turns(), 2);
    }

    #[test]
    fn unset_limit_preserves_legacy_unbounded_behavior() {
        let mut limit = RunTurnLimit::parse(None).expect("unset limit should parse");
        for _ in 0..10 {
            assert!(!limit.complete_turn_and_should_stop());
        }
        assert_eq!(limit.stop_reason(), None);
    }

    #[test]
    fn max_turns_reason_has_stable_plain_and_structured_contracts() {
        let reason = RunStopReason::MaxTurnsReached;
        assert_eq!(reason.code(), "max_turns_reached");
        assert_eq!(reason.label(), "maximum turns reached");
        assert_eq!(
            plain_stop_message(Some(reason)).as_deref(),
            Some("Run stopped: maximum turns reached (max_turns_reached)")
        );
        assert_eq!(plain_stop_message(None), None);

        let encoded = serde_json::to_value(RunStopMetadata::from_reason(Some(reason))).unwrap();
        assert_eq!(encoded["stop_reason"], "max_turns_reached");
        assert_eq!(encoded["outcome"], "bounded_stop");
        assert_eq!(encoded["safety_bound"]["bound"], "max_turns");
        assert_eq!(encoded["safety_bound"]["source"], "invocation");

        let legacy = serde_json::to_value(RunStopMetadata::from_reason(None)).unwrap();
        assert!(legacy.get("stop_reason").is_none());
        assert!(legacy.get("outcome").is_none());
        assert!(legacy.get("safety_bound").is_none());
    }
}
