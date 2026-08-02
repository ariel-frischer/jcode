use anyhow::{Result, bail};
use jcode_provider_core::canonical_reasoning_effort;
use std::collections::HashMap;

/// The wire-level effort vocabulary accepted by the memory sidecar.
pub(crate) const SUPPORTED_MEMORY_REASONING_EFFORTS: &[&str] =
    &["none", "minimal", "low", "medium", "high", "xhigh", "max"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MemoryReasoningResolution {
    pub(crate) requested_model: String,
    pub(crate) request_model: String,
    pub(crate) configured_effort: Option<String>,
    pub(crate) explicit_override: Option<String>,
    pub(crate) effective_effort: Option<String>,
    pub(crate) effective_description: String,
}

/// Resolve the memory effort without reading process state or performing I/O.
/// The caller supplies the actual request model and any cached catalog data so
/// request construction and diagnostics share one deterministic contract.
pub(crate) fn resolve_memory_reasoning(
    requested_model: &str,
    request_model: &str,
    configured_effort: Option<&str>,
    oauth_fallback: bool,
    catalog_efforts: Option<&HashMap<String, Vec<String>>>,
) -> Result<MemoryReasoningResolution> {
    let requested_model = requested_model.trim().to_string();
    let request_model = request_model.trim().to_string();
    let explicit_override = configured_effort
        .map(|effort| normalize_memory_effort(effort, &request_model, catalog_efforts))
        .transpose()?;

    let (effective_effort, effective_description) = if let Some(effort) = &explicit_override {
        (
            Some(effort.clone()),
            format!("configured {effort} for model {request_model}"),
        )
    } else if request_model == super::SIDECAR_OPENAI_MODEL {
        (
            Some(super::SIDECAR_OPENAI_REASONING.to_string()),
            format!(
                "model default {} for {request_model}",
                super::SIDECAR_OPENAI_REASONING
            ),
        )
    } else if oauth_fallback && request_model == super::SIDECAR_OPENAI_OAUTH_FALLBACK_MODEL {
        (
            Some(super::SIDECAR_OPENAI_OAUTH_FALLBACK_REASONING.to_string()),
            format!(
                "OAuth fallback default {} for {request_model}",
                super::SIDECAR_OPENAI_OAUTH_FALLBACK_REASONING
            ),
        )
    } else if is_openai_model(&request_model) {
        (
            None,
            format!("reasoning omitted for {request_model}; provider/model default"),
        )
    } else {
        (
            None,
            format!("reasoning not applicable for resolved model {request_model}"),
        )
    };

    Ok(MemoryReasoningResolution {
        requested_model,
        request_model,
        configured_effort: explicit_override.clone(),
        explicit_override,
        effective_effort,
        effective_description,
    })
}

pub(crate) fn normalize_memory_effort(
    raw_effort: &str,
    model: &str,
    catalog_efforts: Option<&HashMap<String, Vec<String>>>,
) -> Result<String> {
    let canonical = canonical_reasoning_effort(raw_effort).ok_or_else(|| {
        anyhow::anyhow!(
            "Invalid memory reasoning effort '{}' for model '{}'. Supported alternatives: {}",
            raw_effort.trim(),
            model,
            supported_efforts(model, catalog_efforts)
        )
    })?;

    if !is_openai_model(model) {
        bail!(
            "Memory reasoning effort '{}' is incompatible with resolved model '{}'; use an OpenAI memory model or remove the setting",
            raw_effort.trim(),
            model
        );
    }

    if let Some(model_efforts) = catalog_efforts.and_then(|catalog| catalog.get(model)) {
        let supported = model_efforts
            .iter()
            .filter_map(|effort| canonical_reasoning_effort(effort))
            .collect::<Vec<_>>();
        if !supported.contains(&canonical) {
            bail!(
                "Memory reasoning effort '{}' is incompatible with resolved model '{}'. Supported alternatives: {}",
                raw_effort.trim(),
                model,
                supported_efforts(model, catalog_efforts)
            );
        }
    }

    Ok(canonical.to_string())
}

fn is_openai_model(model: &str) -> bool {
    crate::provider::provider_for_model(model) == Some("openai") || model.starts_with("gpt-")
}

fn supported_efforts(
    model: &str,
    catalog_efforts: Option<&HashMap<String, Vec<String>>>,
) -> String {
    catalog_efforts
        .and_then(|catalog| catalog.get(model))
        .map(|efforts| {
            efforts
                .iter()
                .filter_map(|effort| canonical_reasoning_effort(effort))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|efforts| !efforts.is_empty())
        .unwrap_or_else(|| SUPPORTED_MEMORY_REASONING_EFFORTS.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn normalizes_supported_efforts_and_rejects_swarm_sentinels() {
        assert_eq!(
            normalize_memory_effort(" XHIGH ", "gpt-5.6-luna", None).unwrap(),
            "xhigh"
        );
        assert_eq!(
            normalize_memory_effort("LOW", "gpt-5.6-luna", None).unwrap(),
            "low"
        );

        let error = normalize_memory_effort("swarm-deep", "gpt-5.6-luna", None)
            .expect_err("swarm sentinels are not wire-level memory efforts");
        let message = error.to_string();
        assert!(message.contains("swarm-deep"));
        assert!(message.contains("gpt-5.6-luna"));
        assert!(message.contains("none, minimal, low, medium, high, xhigh, max"));
    }

    #[test]
    fn rejects_unknown_effort_with_actionable_context() {
        let error = normalize_memory_effort("turbo", "gpt-5.6-luna", None)
            .expect_err("unknown effort should be rejected");
        let message = error.to_string();
        assert!(message.contains("turbo"));
        assert!(message.contains("gpt-5.6-luna"));
        assert!(message.contains("Supported alternatives"));
    }

    #[test]
    fn rejects_effort_for_non_openai_resolved_model() {
        let error = resolve_memory_reasoning(
            "claude-haiku-4-5-20241022",
            "claude-haiku-4-5-20241022",
            Some("high"),
            false,
            None,
        )
        .expect_err("OpenAI effort cannot be sent to Claude");
        let message = error.to_string();
        assert!(message.contains("high"));
        assert!(message.contains("claude-haiku-4-5-20241022"));
        assert!(message.contains("OpenAI memory model"));
    }

    #[test]
    fn resolves_explicit_luna_effort() {
        let resolution =
            resolve_memory_reasoning("gpt-5.6-luna", "gpt-5.6-luna", Some("xhigh"), false, None)
                .expect("xhigh is supported by Luna");

        assert_eq!(resolution.requested_model, "gpt-5.6-luna");
        assert_eq!(resolution.request_model, "gpt-5.6-luna");
        assert_eq!(resolution.configured_effort.as_deref(), Some("xhigh"));
        assert_eq!(resolution.explicit_override.as_deref(), Some("xhigh"));
        assert_eq!(resolution.effective_effort.as_deref(), Some("xhigh"));
        assert!(resolution.effective_description.contains("configured"));
    }

    #[test]
    fn resolves_unset_luna_default_and_oauth_fallback() {
        let luna = resolve_memory_reasoning("gpt-5.6-luna", "gpt-5.6-luna", None, false, None)
            .expect("Luna default should resolve");
        assert_eq!(luna.configured_effort, None);
        assert_eq!(luna.effective_effort.as_deref(), Some("none"));
        assert!(luna.effective_description.contains("model default"));

        let fallback = resolve_memory_reasoning("gpt-5.6-luna", "gpt-5.4", None, true, None)
            .expect("OAuth fallback default should resolve");
        assert_eq!(fallback.effective_effort.as_deref(), Some("low"));
        assert!(fallback.effective_description.contains("OAuth fallback"));
    }

    #[test]
    fn resolves_unset_other_openai_model_as_omitted() {
        let resolution =
            resolve_memory_reasoning("gpt-4.1-mini", "gpt-4.1-mini", None, false, None)
                .expect("other OpenAI models should preserve omission");
        assert_eq!(resolution.effective_effort, None);
        assert!(resolution.effective_description.contains("omitted"));
    }

    #[test]
    fn uses_catalog_efforts_for_model_compatibility() {
        let mut catalog = HashMap::new();
        catalog.insert(
            "gpt-5.6-luna".to_string(),
            vec!["none".to_string(), "low".to_string()],
        );

        let error = resolve_memory_reasoning(
            "gpt-5.6-luna",
            "gpt-5.6-luna",
            Some("xhigh"),
            false,
            Some(&catalog),
        )
        .expect_err("catalog-known incompatible effort should fail");
        let message = error.to_string();
        assert!(message.contains("xhigh"));
        assert!(message.contains("gpt-5.6-luna"));
        assert!(message.contains("none, low"));
    }
}
