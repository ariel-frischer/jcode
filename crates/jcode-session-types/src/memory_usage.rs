//! Local accounting contracts. No runtime, provider, storage, or content payloads.
//!
//! Consumers MUST validate records before persistence and after deserialization.
//! Identifier syntax is a bound, not a content sanitizer: IDs must originate from
//! trusted session/operation/request identity, model names from resolved routing.
//! Never pass prompts, provider errors, credentials, or account IDs into them.

use crate::lifecycle::LifecycleObservabilityStatus;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const MEMORY_USAGE_SCHEMA_VERSION: u16 = 1;
pub const MAX_ACCOUNTING_IDENTIFIER_BYTES: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryCallContext {
    pub session_id: Option<String>,
    pub operation_id: String,
    pub operation_kind: MemoryOperationKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryOperationKind {
    Relevance,
    Rerank,
    IncrementalExtraction,
    FinalExtraction,
    ContradictionCheck,
    ClusterNaming,
    /// Compatibility callers without an explicit memory operation.
    Unattributed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestOutcome {
    Success,
    Incomplete,
    Error,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthClass {
    Oauth,
    ApiKey,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffort {
    None,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptCoverage {
    PhysicalAttempt,
    /// Generic adapter cannot observe transport retries hidden by its provider.
    ProviderCallOnly,
}

/// Normalized input includes cache reads and creations; output includes reasoning.
/// None is unknown, never a reported zero. Invalid provider fields must be left
/// unknown by collectors, not coerced into valid-looking zero or capped values.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenUsage {
    pub input_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub cache_creation_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
}

impl TokenUsage {
    pub fn validate(&self) -> Result<(), ValidationError> {
        for (subset, total) in [
            (self.cached_input_tokens, self.input_tokens),
            (self.cache_creation_tokens, self.input_tokens),
            (self.reasoning_tokens, self.output_tokens),
        ] {
            if let (Some(subset), Some(total)) = (subset, total)
                && subset > total
            {
                return Err(ValidationError::InvalidSubset);
            }
        }
        if let (Some(read), Some(created)) = (self.cached_input_tokens, self.cache_creation_tokens)
        {
            let cached = read
                .checked_add(created)
                .ok_or(ValidationError::TokenOverflow)?;
            if self.input_tokens.is_some_and(|input| cached > input) {
                return Err(ValidationError::InvalidSubset);
            }
        }
        if let (Some(input), Some(output)) = (self.input_tokens, self.output_tokens) {
            input
                .checked_add(output)
                .ok_or(ValidationError::TokenOverflow)?;
        }
        Ok(())
    }

    /// Cache and reasoning subsets are deliberately not added again. Invalid
    /// values return a safe error, while absent totals remain explicitly unknown.
    pub fn total_tokens(&self) -> Result<Option<u64>, ValidationError> {
        self.validate()?;
        match (self.input_tokens, self.output_tokens) {
            (Some(input), Some(output)) => input
                .checked_add(output)
                .map(Some)
                .ok_or(ValidationError::TokenOverflow),
            _ => Ok(None),
        }
    }
}

/// USD in billionths (10^-9), never floating-point or subscription allocations.
/// Pricing arithmetic and provenance resolution belong to the runtime owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CostEstimate {
    pub basis: PricingBasis,
    pub estimate_nano_usd: Option<u64>,
    pub known_subtotal_nano_usd: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PricingBasis {
    /// Known model-specific public API rates. Not an actual billed charge,
    /// including for native OAuth requests.
    PublicApiEquivalent,
    Unknown,
}

/// One actual send, finalized once. Retry/fallback/vote sends get distinct IDs.
/// A preflight rejection or skipped operation must not construct an observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryRequestObservation {
    pub schema_version: u16,
    pub request_id: String,
    pub context: MemoryCallContext,
    pub recorded_at: DateTime<Utc>,
    pub provider: String,
    pub model: String,
    pub effort: Option<ReasoningEffort>,
    pub auth_class: AuthClass,
    pub outcome: RequestOutcome,
    pub usage: TokenUsage,
    pub attempt_coverage: AttemptCoverage,
    pub pricing: CostEstimate,
}

impl MemoryRequestObservation {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.schema_version != MEMORY_USAGE_SCHEMA_VERSION {
            return Err(ValidationError::UnsupportedSchema);
        }
        for id in [
            Some(self.request_id.as_str()),
            Some(self.context.operation_id.as_str()),
            self.context.session_id.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            validate_accounting_identifier(id)?;
        }
        validate_accounting_identifier(&self.provider)?;
        if !valid_identifier(&self.model, true) {
            return Err(ValidationError::InvalidIdentifier);
        }
        self.usage.validate()?;
        if self.pricing.basis == PricingBasis::Unknown
            && (self.pricing.estimate_nano_usd.is_some()
                || self.pricing.known_subtotal_nano_usd != 0)
        {
            return Err(ValidationError::InvalidEstimate);
        }
        if self
            .pricing
            .estimate_nano_usd
            .is_some_and(|estimate| estimate != self.pricing.known_subtotal_nano_usd)
        {
            return Err(ValidationError::InvalidEstimate);
        }
        Ok(())
    }
}

/// Safe opaque selector grammar shared with future CLI filtering. This does not
/// authenticate ownership. Callers must supply the actual originating session.
pub fn validate_accounting_identifier(value: &str) -> Result<(), ValidationError> {
    if valid_identifier(value, false) {
        Ok(())
    } else {
        Err(ValidationError::InvalidIdentifier)
    }
}

fn valid_identifier(value: &str, model: bool) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ACCOUNTING_IDENTIFIER_BYTES
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && !value.contains("..")
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'-' | b'_' | b'.')
                || (model && matches!(byte, b'/' | b':'))
        })
}

/// Static categories only. Display never interpolates untrusted field values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationError {
    UnsupportedSchema,
    InvalidIdentifier,
    InvalidSubset,
    TokenOverflow,
    InvalidEstimate,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::UnsupportedSchema => "unsupported accounting schema",
            Self::InvalidIdentifier => "invalid accounting identifier",
            Self::InvalidSubset => "invalid accounting token subset",
            Self::TokenOverflow => "accounting token overflow",
            Self::InvalidEstimate => "invalid accounting estimate",
        })
    }
}

impl std::error::Error for ValidationError {}

/// Known subtotal is meaningful only alongside its count of unknown calls.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenSubtotal {
    pub known_subtotal: u64,
    pub unknown_calls: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenSubtotals {
    pub input_tokens: TokenSubtotal,
    pub cached_input_tokens: TokenSubtotal,
    pub cache_creation_tokens: TokenSubtotal,
    pub output_tokens: TokenSubtotal,
    pub reasoning_tokens: TokenSubtotal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetainedWindow {
    pub first_recorded_at: Option<DateTime<Utc>>,
    pub last_recorded_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SummaryCoverage {
    /// Only retained observations, never complete lifetime accounting.
    RetainedWindow,
    Partial,
    Unavailable,
}

/// Output contract, not a durable lifetime ledger. Aggregators must use checked
/// arithmetic and mark partial/unavailable coverage on loss, corruption or expiry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionUsageSummary {
    pub session_id: Option<String>,
    pub calls: u64,
    pub tokens: TokenSubtotals,
    pub known_cost_subtotal_nano_usd: u64,
    pub unknown_cost_calls: u64,
    pub window: RetainedWindow,
    pub coverage: SummaryCoverage,
    pub controls: LifecycleObservabilityStatus,
}

#[cfg(test)]
#[path = "memory_usage_tests.rs"]
mod tests;
