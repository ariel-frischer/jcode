use super::{Tool, ToolContext, ToolOutput};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use jcode_lsp::protocol::file_uri;
use jcode_lsp::{LspAction, LspSessionManager};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const LSP_ACTIONS: &[&str] = &[
    "sessions",
    "status",
    "start",
    "feedback",
    "diagnostics",
    "capabilities",
    "hover",
    "definition",
    "references",
    "symbols",
    "formatting",
    "rename",
    "code_actions",
    "disconnect",
];

pub struct LspTool {
    manager: Arc<LspSessionManager>,
}

impl LspTool {
    pub fn new(manager: Arc<LspSessionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for LspTool {
    fn name(&self) -> &str {
        "lsp"
    }

    fn description(&self) -> &str {
        "Use configured language servers for bounded diagnostics, navigation, capabilities, and edit feedback. LSP is optional: missing or unavailable servers never make ordinary edits fail."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {"type": "string", "enum": LSP_ACTIONS},
                "session_id": {"type": "string"},
                "server": {"type": "string"},
                "file": {"type": "string"},
                "cwd": {"type": "string"},
                "text": {"type": "string"},
                "version": {"type": "integer"},
                "line": {"type": "integer"},
                "character": {"type": "integer"},
                "new_name": {"type": "string"},
                "uri": {"type": "string"}
            }
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let action = input
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("lsp action is required"))?;
        match action {
            "sessions" | "status" => {
                let sessions = self.manager.list().await;
                return render(action, json!({"sessions": sessions}));
            }
            "disconnect" => {
                let session_id = required_string(&input, "session_id")?;
                self.manager.disconnect(session_id).await?;
                return render(action, json!({"disconnected": session_id}));
            }
            _ => {}
        }

        let cwd = resolve_cwd(&input, &ctx)?;
        let file = input
            .get("file")
            .and_then(Value::as_str)
            .map(|file| ctx.resolve_path(Path::new(file)));
        let session_id = match input.get("session_id").and_then(Value::as_str) {
            Some(session_id) => session_id.to_owned(),
            None => {
                let file = file
                    .as_deref()
                    .ok_or_else(|| anyhow!("lsp action '{action}' requires file or session_id"))?;
                let started = self
                    .manager
                    .start_for_file(&cwd, file, input.get("server").and_then(Value::as_str))
                    .await?;
                match started {
                    Some(session_id) => session_id,
                    None => {
                        return render(
                            action,
                            json!({"active": false, "reason": "no enabled language server matches this file and project"}),
                        );
                    }
                }
            }
        };

        match action {
            "start" => render(action, json!({"session_id": session_id, "active": true})),
            "feedback" => {
                let file = file.ok_or_else(|| anyhow!("lsp feedback requires file"))?;
                let text = input
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow!("lsp feedback requires text"))?;
                let version = input.get("version").and_then(Value::as_i64).unwrap_or(1);
                let feedback = self
                    .manager
                    .feedback_after_edit(&cwd, &file, text, version)
                    .await?;
                render(action, json!(feedback))
            }
            "diagnostics" => {
                let file_uri = file.as_deref().map(file_uri);
                let uri = input
                    .get("uri")
                    .and_then(Value::as_str)
                    .or(file_uri.as_deref());
                let diagnostics = self.manager.diagnostics(&session_id, uri).await?;
                render(
                    action,
                    json!({"session_id": session_id, "diagnostics": diagnostics}),
                )
            }
            "capabilities" => {
                let capabilities = self
                    .manager
                    .get(&session_id)
                    .await
                    .map(|snapshot| snapshot.capabilities);
                render(
                    action,
                    json!({"session_id": session_id, "capabilities": capabilities}),
                )
            }
            "hover" | "definition" | "references" | "symbols" | "formatting" | "rename"
            | "code_actions" => {
                let lsp_action = parse_action(action)?;
                let params = request_params(action, &input, file.as_deref())?;
                let result = self
                    .manager
                    .execute(&session_id, lsp_action, params)
                    .await?;
                render(action, json!({"session_id": session_id, "result": result}))
            }
            other => Err(anyhow!("unknown lsp action '{other}'")),
        }
    }
}

fn parse_action(action: &str) -> Result<LspAction> {
    Ok(match action {
        "hover" => LspAction::Hover,
        "definition" => LspAction::Definition,
        "references" => LspAction::References,
        "symbols" => LspAction::Symbols,
        "formatting" => LspAction::Formatting,
        "rename" => LspAction::Rename,
        "code_actions" => LspAction::CodeActions,
        _ => return Err(anyhow!("unsupported lsp action '{action}'")),
    })
}

fn required_string<'a>(input: &'a Value, key: &str) -> Result<&'a str> {
    input
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("lsp {key} is required"))
}

fn resolve_cwd(input: &Value, ctx: &ToolContext) -> Result<PathBuf> {
    input
        .get("cwd")
        .and_then(Value::as_str)
        .map(|cwd| ctx.resolve_path(Path::new(cwd)))
        .or_else(|| ctx.working_dir.clone())
        .ok_or_else(|| anyhow!("lsp requires a session working directory"))
}

fn request_params(action: &str, input: &Value, file: Option<&Path>) -> Result<Value> {
    let file = file.ok_or_else(|| anyhow!("lsp action '{action}' requires file"))?;
    let uri = file_uri(file);
    let line = input.get("line").and_then(Value::as_u64).unwrap_or(0);
    let character = input.get("character").and_then(Value::as_u64).unwrap_or(0);
    let text_document = json!({"uri": uri});
    let position = json!({"line": line, "character": character});
    Ok(match action {
        "hover" | "definition" => json!({"textDocument": text_document, "position": position}),
        "references" => {
            json!({"textDocument": text_document, "position": position, "context": {"includeDeclaration": true}})
        }
        "symbols" => json!({"textDocument": text_document}),
        "formatting" => {
            json!({"textDocument": text_document, "options": {"tabSize": 4, "insertSpaces": true}})
        }
        "rename" => {
            json!({"textDocument": text_document, "position": position, "newName": required_string(input, "new_name")?})
        }
        "code_actions" => {
            json!({"textDocument": text_document, "range": {"start": position, "end": position}, "context": {"diagnostics": []}})
        }
        _ => return Err(anyhow!("unsupported lsp action '{action}'")),
    })
}

fn render(action: &str, value: Value) -> Result<ToolOutput> {
    let body = serde_json::to_string_pretty(&value)?;
    Ok(ToolOutput::new(body)
        .with_title(format!("LSP {action}"))
        .with_metadata(json!({"action": action, "lsp": true})))
}
