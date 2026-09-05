use jcode_provider_core::ModelRoute;
use std::collections::HashSet;

const MAX_ROUTES: usize = 60;
const MAX_BODY_BYTES: usize = 11 * 1024;

// Catalog metadata is provider supplied. Bound individual fields as well as the
// number of rows, including Unicode, so even a single route cannot flood context.
fn field(value: &str) -> String {
    let mut chars = value.chars();
    let mut result: String = chars.by_ref().take(160).collect();
    if chars.next().is_some() {
        result.push('…');
    }
    result
}

pub(super) fn format_swarm_model_list(
    current_model: Option<&str>,
    configured_swarm_model: Option<&str>,
    model_routes: &[ModelRoute],
) -> String {
    let mut out = format!(
        "Current coordinator model: {}\n",
        field(current_model.unwrap_or("unknown"))
    );
    match configured_swarm_model.filter(|pin| !pin.trim().is_empty()) {
        Some(pin) => out.push_str(&format!("Configured agents.swarm_model pin: {}\n", field(pin))),
        None => out.push_str("No agents.swarm_model pin configured (workers inherit the coordinator unless model is supplied).\n"),
    }
    if model_routes.is_empty() {
        out.push_str("\nNo model routes reported. Pass model explicitly or omit it to inherit.");
        return out;
    }
    out.push_str("\nAvailable model routes (pass model explicitly, e.g. openai:gpt-6-astra):\n");
    let is_current = |route: &&ModelRoute| {
        [current_model, configured_swarm_model]
            .into_iter()
            .flatten()
            .any(|model| {
                let bare = model.rsplit_once(':').map_or(model, |(_, bare)| bare);
                route.model == bare
            })
    };
    let priority = model_routes.iter().filter(is_current);
    let available = model_routes.iter().filter(|route| route.available);
    let mut seen = HashSet::new();
    let mut shown = 0;
    for route in priority.chain(available).chain(model_routes.iter()) {
        if !seen.insert((&route.model, &route.provider, &route.api_method)) {
            continue;
        }
        let cost = match route.estimated_reference_cost_micros() {
            Some(micros) => format!(" ~${:.2}/ref-task", micros as f64 / 1_000_000.0),
            None => String::new(), // The catalog does not price every route.
        };
        let detail = if route.detail.is_empty() {
            String::new()
        } else {
            format!(" ({})", field(&route.detail))
        };
        let line = format!(
            "- {} via {} [{}]{}{}{}\n",
            field(&route.model),
            field(&route.provider),
            field(&route.api_method),
            if route.available {
                ""
            } else {
                " [unavailable]"
            },
            cost,
            detail
        );
        if shown == MAX_ROUTES || out.len() + line.len() > MAX_BODY_BYTES {
            break;
        }
        out.push_str(&line);
        shown += 1;
    }
    let omitted = model_routes.len().saturating_sub(shown);
    if omitted > 0 {
        out.push_str(&format!("\n{omitted} catalog entries omitted (including duplicates). Use the model picker for the full catalog.\n"));
    }
    out.push_str("\nAlso pass effort (none|minimal|low|medium|high|xhigh|max) to set the spawned agent's reasoning effort.");
    out
}
