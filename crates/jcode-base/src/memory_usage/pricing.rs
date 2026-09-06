//! Offline standard-tier API equivalents, not billing or subscription allocation.
//! The existing static rate table owns model support and may lag public prices.
use jcode_provider_core::{RouteCheapnessEstimate, pricing};
use jcode_session_types::memory_usage::{
    CostEstimate, MemoryRequestObservation, PricingBasis, TokenUsage, ValidationError,
};

pub(super) fn unknown() -> CostEstimate {
    CostEstimate {
        basis: PricingBasis::Unknown,
        estimate_nano_usd: None,
        known_subtotal_nano_usd: 0,
    }
}

pub(super) fn estimate(call: &MemoryRequestObservation) -> Result<CostEstimate, ValidationError> {
    // Exact resolved names only. Never strip a provider prefix, infer an alias,
    // use routing reference cost, or fetch live pricing. Luna remains unpriced.
    let rates = match call.provider.as_str() {
        "openai" => pricing::openai_api_pricing(&call.model),
        "claude" | "anthropic" => pricing::anthropic_api_pricing(&call.model),
        _ => None,
    };
    estimate_with_rates(&call.usage, rates.as_ref())
}

pub(super) fn estimate_with_rates(
    usage: &TokenUsage,
    rates: Option<&RouteCheapnessEstimate>,
) -> Result<CostEstimate, ValidationError> {
    usage.validate()?;
    let Some(rates) = rates else {
        return Ok(unknown());
    };
    let uncached = match (
        usage.input_tokens,
        usage.cached_input_tokens,
        usage.cache_creation_tokens,
    ) {
        (Some(input), Some(read), Some(created)) => {
            input.checked_sub(read).and_then(|n| n.checked_sub(created))
        }
        _ => None,
    };
    let components = [
        (uncached, rates.input_price_per_mtok_micros),
        (
            usage.cached_input_tokens,
            rates.cache_read_price_per_mtok_micros,
        ),
        // The canonical contract exposes no creation rate or cache TTL. Do not
        // invent an Anthropic multiplier or charge creations as ordinary input.
        (usage.cache_creation_tokens, None),
        (usage.output_tokens, rates.output_price_per_mtok_micros),
    ];
    let mut known = 0_u128;
    let mut complete = true;
    for (tokens, rate) in components {
        let term = match (tokens, rate) {
            (Some(0), _) => Some(0), // A known empty component requires no rate.
            (Some(tokens), Some(rate)) => Some(u128::from(tokens) * u128::from(rate)),
            _ => None,
        };
        if let Some(term) = term {
            known = known
                .checked_add(term)
                .ok_or(ValidationError::InvalidEstimate)?;
        } else {
            complete = false;
        }
    }
    // micros per million tokens * tokens / 1000 = nano-USD. Sum known
    // numerators first, round once per call to nearest nano-USD, half up.
    let rounded = known
        .checked_add(500)
        .ok_or(ValidationError::InvalidEstimate)?
        / 1000;
    let subtotal = u64::try_from(rounded).map_err(|_| ValidationError::InvalidEstimate)?;
    Ok(CostEstimate {
        basis: PricingBasis::PublicApiEquivalent,
        estimate_nano_usd: complete.then_some(subtotal),
        known_subtotal_nano_usd: subtotal,
    })
}
