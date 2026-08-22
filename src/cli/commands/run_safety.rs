use anyhow::{Result, bail};
use serde_json::Value;
use std::num::NonZeroU64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RunStopReason {
    MaxTurnsExceeded,
}

impl RunStopReason {
    pub(super) fn code(self) -> &'static str {
        match self {
            Self::MaxTurnsExceeded => "max_turns_exceeded",
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::MaxTurnsExceeded => "maximum turns exceeded",
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

    pub(super) fn complete_turn(&mut self) -> bool {
        self.completed_turns = self.completed_turns.saturating_add(1);
        if self
            .max_turns
            .is_some_and(|max_turns| self.completed_turns >= max_turns.get())
        {
            self.stop_reason = Some(RunStopReason::MaxTurnsExceeded);
            true
        } else {
            false
        }
    }

    pub(super) fn stop_reason(&self) -> Option<RunStopReason> {
        self.stop_reason
    }
}

pub(super) fn annotate_json(mut value: Value, reason: Option<RunStopReason>) -> Value {
    let (Some(reason), Some(object)) = (reason, value.as_object_mut()) else {
        return value;
    };
    object.insert("stop_reason".into(), Value::String(reason.code().into()));
    object.insert("outcome".into(), Value::String("bounded_stop".into()));
    object.insert(
        "safety_bound".into(),
        serde_json::json!({"bound": "max_turns", "source": "invocation"}),
    );
    value
}

pub(super) fn print_plain_stop(reason: Option<RunStopReason>) {
    if let Some(message) = plain_stop_message(reason) {
        eprintln!("{message}");
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

        assert!(!limit.complete_turn());
        assert_eq!(limit.stop_reason(), None);
        assert!(limit.complete_turn());
        assert_eq!(limit.stop_reason(), Some(RunStopReason::MaxTurnsExceeded));
        assert_eq!(limit.completed_turns(), 2);
    }

    #[test]
    fn unset_limit_preserves_legacy_unbounded_behavior() {
        let mut limit = RunTurnLimit::parse(None).expect("unset limit should parse");
        for _ in 0..10 {
            assert!(!limit.complete_turn());
        }
        assert_eq!(limit.stop_reason(), None);
    }

    #[test]
    fn max_turns_reason_has_stable_plain_and_structured_contracts() {
        let reason = RunStopReason::MaxTurnsExceeded;
        assert_eq!(reason.code(), "max_turns_exceeded");
        assert_eq!(reason.label(), "maximum turns exceeded");
        assert_eq!(
            plain_stop_message(Some(reason)).as_deref(),
            Some("Run stopped: maximum turns exceeded (max_turns_exceeded)")
        );
        assert_eq!(plain_stop_message(None), None);

        let encoded = annotate_json(
            serde_json::json!({"type": "done", "text": "partial"}),
            Some(reason),
        );
        assert_eq!(encoded["stop_reason"], "max_turns_exceeded");
        assert_eq!(encoded["outcome"], "bounded_stop");
        assert_eq!(encoded["safety_bound"]["bound"], "max_turns");
        assert_eq!(encoded["safety_bound"]["source"], "invocation");

        let legacy = annotate_json(serde_json::json!({"type": "done"}), None);
        assert!(legacy.get("stop_reason").is_none());
        assert!(legacy.get("outcome").is_none());
        assert!(legacy.get("safety_bound").is_none());
    }
}
