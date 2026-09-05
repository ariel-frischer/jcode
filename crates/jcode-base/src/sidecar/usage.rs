//! Provider snapshots replace fields, never accumulate repeated cumulative events.
use jcode_session_types::memory_usage::{RequestOutcome, TokenUsage};
use serde_json::Value;

pub(super) struct UsageAccumulator {
    pub usage: TokenUsage,
    pub outcome: RequestOutcome,
    claude_uncached: Option<u64>,
    claude_limited: bool,
}

impl Default for UsageAccumulator {
    fn default() -> Self {
        Self {
            usage: TokenUsage::default(),
            outcome: RequestOutcome::Incomplete,
            claude_uncached: None,
            claude_limited: false,
        }
    }
}

impl UsageAccumulator {
    pub fn generic(
        &mut self,
        input: Option<u64>,
        output: Option<u64>,
        read: Option<u64>,
        write: Option<u64>,
        provider: &str,
    ) {
        self.usage = TokenUsage {
            input_tokens: if matches!(provider, "claude" | "anthropic") {
                match (input, read, write) {
                    (Some(input), Some(read), Some(write)) => {
                        input.checked_add(read).and_then(|n| n.checked_add(write))
                    }
                    _ => None,
                }
            } else {
                input
            },
            output_tokens: output,
            cached_input_tokens: read,
            cache_creation_tokens: write,
            reasoning_tokens: None,
        };
        self.sanitize();
    }

    pub fn openai(&mut self, value: &Value) {
        let response = value.get("response").unwrap_or(value);
        if let Some(usage) = response.get("usage").filter(|u| u.is_object()) {
            self.usage = TokenUsage {
                input_tokens: usage["input_tokens"].as_u64(),
                output_tokens: usage["output_tokens"].as_u64(),
                cached_input_tokens: usage["input_tokens_details"]["cached_tokens"].as_u64(),
                reasoning_tokens: usage["output_tokens_details"]["reasoning_tokens"].as_u64(),
                // Responses API does not expose a separately priced cache-write component.
                cache_creation_tokens: None,
            };
            self.sanitize();
        }
        match value["type"].as_str().or(response["status"].as_str()) {
            Some("response.completed" | "completed") => self.outcome = RequestOutcome::Success,
            Some("response.failed" | "failed" | "error") => self.outcome = RequestOutcome::Error,
            Some("response.incomplete" | "incomplete") => self.outcome = RequestOutcome::Incomplete,
            _ => {}
        }
    }

    pub fn claude(&mut self, value: &Value) {
        let message = value.get("message").unwrap_or(value);
        if let Some(usage) = message.get("usage").filter(|u| u.is_object()) {
            // message_delta reports only output; preserve message_start input components.
            for (field, target) in [
                ("input_tokens", &mut self.claude_uncached),
                (
                    "cache_read_input_tokens",
                    &mut self.usage.cached_input_tokens,
                ),
                (
                    "cache_creation_input_tokens",
                    &mut self.usage.cache_creation_tokens,
                ),
                ("output_tokens", &mut self.usage.output_tokens),
            ] {
                if let Some(value) = usage.get(field) {
                    *target = value.as_u64();
                }
            }
            self.usage.input_tokens = match (
                self.claude_uncached,
                self.usage.cached_input_tokens,
                self.usage.cache_creation_tokens,
            ) {
                (Some(input), Some(read), Some(write)) => input
                    .checked_add(read)
                    .and_then(|total| total.checked_add(write)),
                _ => None,
            };
            self.sanitize();
        }
        match value["type"].as_str() {
            Some("error") => self.outcome = RequestOutcome::Error,
            Some("message_stop") => self.outcome = RequestOutcome::Success,
            _ if message["stop_reason"].is_string() => self.outcome = RequestOutcome::Success,
            _ => {}
        }
        self.claude_limited |= message["stop_reason"].as_str() == Some("max_tokens")
            || value["delta"]["stop_reason"].as_str() == Some("max_tokens");
        if self.claude_limited && self.outcome != RequestOutcome::Error {
            self.outcome = RequestOutcome::Incomplete;
        }
    }

    fn sanitize(&mut self) {
        let usage = &mut self.usage;
        if let (Some(cached), Some(input)) = (usage.cached_input_tokens, usage.input_tokens)
            && cached > input
        {
            usage.cached_input_tokens = None;
        }
        if let (Some(created), Some(input)) = (usage.cache_creation_tokens, usage.input_tokens)
            && created > input
        {
            usage.cache_creation_tokens = None;
        }
        if let (Some(reasoning), Some(output)) = (usage.reasoning_tokens, usage.output_tokens)
            && reasoning > output
        {
            usage.reasoning_tokens = None;
        }
        if let (Some(read), Some(write)) = (usage.cached_input_tokens, usage.cache_creation_tokens)
            && read
                .checked_add(write)
                .is_none_or(|sum| usage.input_tokens.is_some_and(|input| sum > input))
        {
            usage.cached_input_tokens = None;
            usage.cache_creation_tokens = None;
        }
        if let (Some(input), Some(output)) = (usage.input_tokens, usage.output_tokens)
            && input.checked_add(output).is_none()
        {
            usage.output_tokens = None;
        }
    }
}
