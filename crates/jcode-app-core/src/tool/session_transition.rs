use super::{Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

pub(crate) const MAX_PROMPT_CHARS: usize = 32 * 1024;
pub(crate) const MAX_RELEVANT_FILES: usize = 32;

pub(crate) fn prepare_handoff_prompt(
    prompt: Option<String>,
    bead_id: Option<&str>,
    relevant_files: &[String],
) -> Result<Option<String>> {
    if relevant_files.len() > MAX_RELEVANT_FILES {
        anyhow::bail!("Handoff relevant_files exceeds the 32-path limit");
    }

    let mut prompt = prompt.and_then(|prompt| {
        let prompt = prompt.trim().to_owned();
        (!prompt.is_empty()).then_some(prompt)
    });
    if bead_id.is_some() || !relevant_files.is_empty() {
        let mut durable = Vec::new();
        if let Some(bead_id) = bead_id.map(str::trim).filter(|value| !value.is_empty()) {
            durable.push(format!(
                "Durable tracker: inspect Bead `{bead_id}` and its comments."
            ));
        }
        if !relevant_files.is_empty() {
            durable.push(format!(
                "Relevant files:\n{}",
                relevant_files
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
    Ok(prompt)
}

#[derive(Debug, Clone)]
pub(crate) struct PendingSessionTransition {
    pub prompt: Option<String>,
    pub auto_start: bool,
    pub max_chain_transitions: usize,
    pub copy_todos: bool,
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
    copy_todos: Option<bool>,
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
                "copy_todos": {"type": "boolean", "description": "Carry the current todo list into the fresh session. Defaults to true. Set false only when the next task is unrelated and the existing todos would be noise."},
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
        let prompt = prepare_handoff_prompt(
            input.prompt.or(input.goal),
            input.bead_id.as_deref(),
            &input.relevant_files,
        )?;
        let auto_start = input.auto_start.unwrap_or(policy.auto_start) && prompt.is_some();
        let copy_todos = input.copy_todos.unwrap_or(policy.copy_todos);
        PENDING
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(
                ctx.session_id,
                PendingSessionTransition {
                    prompt,
                    auto_start,
                    max_chain_transitions: policy.max_chain_transitions,
                    copy_todos,
                },
            );
        Ok(ToolOutput::new(
            "Fresh-session handoff staged. Finish this turn with a concise completion summary; Jcode will switch sessions after the turn is persisted.",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct JcodeHomeGuard {
        previous: Option<std::ffi::OsString>,
    }

    impl Drop for JcodeHomeGuard {
        fn drop(&mut self) {
            if let Some(previous) = self.previous.take() {
                crate::env::set_var("JCODE_HOME", previous);
            } else {
                crate::env::remove_var("JCODE_HOME");
            }
            crate::config::Config::invalidate_cache();
        }
    }

    fn isolated_config(contents: &str) -> (tempfile::TempDir, JcodeHomeGuard) {
        let directory = tempfile::TempDir::new().expect("temp jcode home");
        let guard = JcodeHomeGuard {
            previous: std::env::var_os("JCODE_HOME"),
        };
        crate::env::set_var("JCODE_HOME", directory.path());
        let path = directory.path().join("config.toml");
        std::fs::write(path, contents).expect("write config");
        crate::config::Config::invalidate_cache();
        (directory, guard)
    }

    fn context(session_id: String) -> ToolContext {
        ToolContext {
            session_id,
            message_id: "message".to_string(),
            tool_call_id: "tool-call".to_string(),
            working_dir: None,
            stdin_request_tx: None,
            graceful_shutdown_signal: None,
            execution_mode: super::super::ToolExecutionMode::Direct,
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn confirmation_gate_and_durable_context_are_applied_before_staging() {
        let _environment = crate::storage::lock_test_env();
        let (_directory, _home) = isolated_config(
            "[handoff]\nagent_requires_confirmation = true\nauto_start = false\nmax_chain_transitions = 3\n",
        );
        let mut session = crate::session::Session::create(None, None);
        session.save().expect("save source session");
        let session_id = session.id.clone();
        let tool = SessionTransitionTool::new();

        let error = tool
            .execute(
                json!({"goal": "Continue validation"}),
                context(session_id.clone()),
            )
            .await
            .expect_err("confirmation must be required");
        assert!(error.to_string().contains("confirmed=true"));
        assert!(take_pending(&session_id).is_none());

        tool.execute(
            json!({
                "goal": "Continue validation",
                "bead_id": "jcode-ggj",
                "relevant_files": ["docs/proposals/FRESH_SESSION_HANDOFF.md"],
                "auto_start": true,
                "confirmed": true
            }),
            context(session_id.clone()),
        )
        .await
        .expect("confirmed transition should stage");

        let pending = take_pending(&session_id).expect("staged transition");
        let prompt = pending.prompt.expect("durable continuation prompt");
        assert!(prompt.starts_with("Continue validation"));
        assert!(prompt.contains("Bead `jcode-ggj`"));
        assert!(prompt.contains("docs/proposals/FRESH_SESSION_HANDOFF.md"));
        assert!(pending.auto_start);
        assert_eq!(pending.max_chain_transitions, 3);
        assert!(
            pending.copy_todos,
            "todos must carry by default when copy_todos is omitted"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn copy_todos_can_be_opted_out_per_handoff() {
        let _environment = crate::storage::lock_test_env();
        let (_directory, _home) = isolated_config("[handoff]\n");
        let mut session = crate::session::Session::create(None, None);
        session.save().expect("save source session");
        let session_id = session.id.clone();

        SessionTransitionTool::new()
            .execute(
                json!({"prompt": "unrelated next task", "copy_todos": false}),
                context(session_id.clone()),
            )
            .await
            .expect("transition should stage");

        let pending = take_pending(&session_id).expect("staged transition");
        assert!(!pending.copy_todos);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn disabled_agent_policy_rejects_without_staging() {
        let _environment = crate::storage::lock_test_env();
        let (_directory, _home) =
            isolated_config("[handoff]\nenabled = true\nagent_enabled = false\n");
        let mut session = crate::session::Session::create(None, None);
        session.save().expect("save source session");
        let session_id = session.id.clone();

        let error = SessionTransitionTool::new()
            .execute(json!({"prompt": "next"}), context(session_id.clone()))
            .await
            .expect_err("agent-disabled policy must reject");

        assert!(error.to_string().contains("disabled"));
        assert!(take_pending(&session_id).is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn oversized_handoff_inputs_are_rejected_without_staging() {
        let _environment = crate::storage::lock_test_env();
        let (_directory, _home) = isolated_config("[handoff]\n");
        let mut session = crate::session::Session::create(None, None);
        session.save().expect("save source session");
        let session_id = session.id.clone();
        let tool = SessionTransitionTool::new();

        let paths = (0..33)
            .map(|index| format!("path-{index}"))
            .collect::<Vec<_>>();
        let error = tool
            .execute(
                json!({"prompt": "next", "relevant_files": paths}),
                context(session_id.clone()),
            )
            .await
            .expect_err("too many relevant files must be rejected");
        assert!(error.to_string().contains("32-path limit"));
        assert!(take_pending(&session_id).is_none());

        let error = tool
            .execute(
                json!({"prompt": "x".repeat(MAX_PROMPT_CHARS + 1)}),
                context(session_id.clone()),
            )
            .await
            .expect_err("oversized prompt must be rejected");
        assert!(error.to_string().contains("32 KiB limit"));
        assert!(take_pending(&session_id).is_none());
    }
}
