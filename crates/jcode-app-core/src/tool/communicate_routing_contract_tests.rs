//! Downstream contract: upstream operator-only routing must not remove explicit selection.
use super::{CommunicateTool, format_swarm_model_list};
use crate::protocol::Request;
use crate::tool::Tool;
use serde_json::json;

#[test]
fn exposed_and_openai_projected_schema_preserve_optional_model() {
    let schema = CommunicateTool::new().parameters_schema();
    for schema in [
        schema.clone(),
        jcode_provider_core::openai_schema::openai_compatible_schema(&schema),
    ] {
        assert_eq!(schema["properties"]["model"]["type"], json!("string"));
        assert!(
            schema["properties"]["effort"]["enum"]
                .as_array()
                .expect("effort choices")
                .contains(&json!("low"))
        );
        assert!(
            !schema["required"]
                .as_array()
                .expect("required fields")
                .contains(&json!("model"))
        );
    }
}

#[test]
fn spawn_and_assignment_wire_preserve_explicit_native_route() {
    for action in ["comm_spawn", "comm_assign_next"] {
        let input = json!({
            "type": action, "id": 7, "session_id": "coordinator",
            "model": "openai:gpt-6-astra", "effort": "low"
        });
        let request: Request = serde_json::from_value(input).expect("valid request");
        let wire = serde_json::to_value(request).expect("serialized request");
        assert_eq!(wire["model"], "openai:gpt-6-astra", "{action}");
        assert_eq!(wire["effort"], "low", "{action}");
    }
}

#[test]
fn oversized_route_catalog_is_bounded() {
    let routes = (0..10_000)
        .map(|i| jcode_provider_core::ModelRoute {
            model: format!("model-{i}"),
            provider: "OpenAI".into(),
            api_method: "openai-oauth".into(),
            available: true,
            detail: "OAuth".into(),
            cheapness: None,
        })
        .collect::<Vec<_>>();
    let output = format_swarm_model_list(Some("gpt-6-astra"), None, &routes);
    assert!(
        output.len() <= 12 * 1024,
        "catalog produced {} bytes",
        output.len()
    );
    assert!(output.contains("gpt-6-astra"));
    assert!(output.contains("omitted"));
}

#[test]
fn legacy_wire_omits_unset_model() {
    for action in ["comm_spawn", "comm_assign_next"] {
        let request: Request = serde_json::from_value(json!({
            "type": action, "id": 7, "session_id": "coordinator"
        }))
        .expect("legacy request");
        let wire = serde_json::to_value(request).expect("serialized request");
        assert!(wire.get("model").is_none());
    }
}

#[test]
fn catalog_prioritizes_current_model_and_bounds_unicode_fields() {
    let mut routes = (0..100)
        .map(|i| jcode_provider_core::ModelRoute {
            model: format!("other-{i}"),
            provider: "OpenAI".into(),
            api_method: "openai-oauth".into(),
            available: true,
            detail: "🦀".repeat(10_000),
            cheapness: None,
        })
        .collect::<Vec<_>>();
    routes.push(jcode_provider_core::ModelRoute {
        model: "gpt-6-astra".into(),
        provider: "OpenAI".into(),
        api_method: "openai-oauth".into(),
        available: true,
        detail: "OAuth".into(),
        cheapness: None,
    });
    let output = format_swarm_model_list(
        Some("gpt-6-astra"),
        Some("🦀".repeat(10_000).as_str()),
        &routes,
    );
    assert!(output.len() <= 12 * 1024);
    assert!(output.contains("- gpt-6-astra via OpenAI [openai-oauth]"));
    assert!(output.contains("omitted"));
}

#[test]
fn catalog_deduplicates_routes_without_hiding_auth_choices() {
    let route = jcode_provider_core::ModelRoute {
        model: "gpt-6-astra".into(),
        provider: "OpenAI".into(),
        api_method: "openai-oauth".into(),
        available: true,
        detail: "OAuth".into(),
        cheapness: None,
    };
    let mut api = route.clone();
    api.api_method = "openai-api-key".into();
    let output = format_swarm_model_list(None, None, &[route.clone(), route, api]);
    assert_eq!(output.matches("- gpt-6-astra via").count(), 2);
    assert!(output.contains("1 catalog entries omitted"));
}
