use super::{Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

const MAX_PROMPT_CHARS: usize = 32 * 1024;

#[derive(Debug, Clone)]
pub(crate) struct PendingSessionTransition {
    pub prompt: Option<String>,
    pub auto_start: bool,
    pub max_chain_transitions: usize,
}

static PENDING: LazyLock<Mutex<HashMap<String, PendingSessionTransition>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(crate) fn take_pending(session_id: &str) -> Option<PendingSessionTransition> {
    PENDING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(session_id)
}

#[derive(Debug, Deserialize)]
struct Input {
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    goal: Option<String>,
    #[serde(default)]
    bead_id: Option<String>,
    #[serde(default)]
    relevant_files: Vec<String>,
    #[serde(default)]
    auto_start: Option<bool>,
    #[serde(default)]
    confirmed: bool,
}

pub struct SessionTransitionTool;

impl SessionTransitionTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for SessionTransitionTool {
    fn name(&self) -> &str {
        "session_transition"
    }

    fn description(&self) -> &str {
        "Finish the current task by staging a clean child session. Use only after the current task is complete and durable state is saved. The current turn finishes normally, then Jcode switches this client to the fresh session and optionally starts the supplied prompt."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "prompt": {"type": "string", "description": "Prompt for the fresh session. Omit to open a blank session."},
                "goal": {"type": "string", "description": "Next-task goal used when prompt is omitted."},
                "bead_id": {"type": "string", "description": "Optional durable Bead identifier for the next session to inspect."},
                "relevant_files": {"type": "array", "items": {"type": "string"}, "maxItems": 32, "description": "Optional bounded list of paths relevant to the next task."},
                "auto_start": {"type": "boolean", "description": "Submit the prompt after switching. Defaults to the effective handoff policy."},
                "confirmed": {"type": "boolean", "description": "Required only when agent_requires_confirmation is enabled."}
            }
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let input: Input = serde_json::from_value(input)?;
        let session = crate::session::Session::load(&ctx.session_id)?;
        let config = crate::config::config();
        let config_dir = crate::storage::jcode_dir().ok();
        let policy = config
            .resolve_handoff_policy(session.profile_name.as_deref(), config_dir.as_deref())?;
        if !policy.enabled || !policy.agent_enabled {
            anyhow::bail!("Agent self-handoff is disabled for this session");
        }
        if policy.agent_requires_confirmation && !input.confirmed {
            anyhow::bail!("Agent self-handoff requires confirmed=true for this session");
        }
        if input.relevant_files.len() > 32 {
            anyhow::bail!("Handoff relevant_files exceeds the 32-path limit");
        }
        let mut prompt = input.prompt.or(input.goal).and_then(|prompt| {
            let prompt = prompt.trim().to_owned();
            (!prompt.is_empty()).then_some(prompt)
        });
        if input.bead_id.is_some() || !input.relevant_files.is_empty() {
            let mut durable = Vec::new();
            if let Some(bead_id) = input
                .bead_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                durable.push(format!(
                    "Durable tracker: inspect Bead `{bead_id}` and its comments."
                ));
            }
            if !input.relevant_files.is_empty() {
                durable.push(format!(
                    "Relevant files:\n{}",
                    input
                        .relevant_files
                        .iter()
                        .map(|path| format!("- {path}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                ));
            }
            let durable = durable.join("\n\n");
            prompt = Some(match prompt {
                Some(prompt) => format!("{prompt}\n\n{durable}"),
                None => durable,
            });
        }
        if prompt
            .as_ref()
            .is_some_and(|prompt| prompt.len() > MAX_PROMPT_CHARS)
        {
            anyhow::bail!("Handoff prompt exceeds the 32 KiB limit");
        }
        let auto_start = input.auto_start.unwrap_or(policy.auto_start) && prompt.is_some();
        PENDING
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(
                ctx.session_id,
                PendingSessionTransition {
                    prompt,
                    auto_start,
                    max_chain_transitions: policy.max_chain_transitions,
                },
            );
        Ok(ToolOutput::new(
            "Fresh-session handoff staged. Finish this turn with a concise completion summary; Jcode will switch sessions after the turn is persisted.",
        ))
    }
}
