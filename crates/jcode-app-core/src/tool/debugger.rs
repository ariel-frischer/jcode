use super::{Tool, ToolContext, ToolOutput};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use jcode_dap::{Action, AttachRequest, DapSessionManager, LaunchRequest};
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::sync::Arc;

const DEBUGGER_ACTIONS: &[&str] = &[
    "sessions", "status", "launch", "attach", "set_breakpoint", "remove_breakpoint",
    "continue", "pause", "step_over", "step_in", "step_out", "threads", "stack_trace",
    "scopes", "variables", "evaluate", "output", "stop", "disconnect", "read_memory",
    "write_memory", "modules",
];

pub struct DebuggerTool {
    manager: Arc<DapSessionManager>,
}

impl DebuggerTool {
    pub fn new(manager: Arc<DapSessionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for DebuggerTool {
    fn name(&self) -> &str {
        "debugger"
    }

    fn description(&self) -> &str {
        "Run bounded, capability-aware DAP debugger operations against configured adapters."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {"type": "string", "enum": DEBUGGER_ACTIONS},
                "session_id": {"type": "string"},
                "parent_session_id": {"type": "string"},
                "adapter": {"type": "string"},
                "program": {"type": "string"},
                "args": {"type": "array", "items": {"type": "string"}},
                "cwd": {"type": "string"},
                "file": {"type": "string"},
                "line": {"type": "integer"},
                "condition": {"type": "string"},
                "expression": {"type": "string"},
                "context": {"type": "string"},
                "thread_id": {"type": "integer"},
                "start_frame": {"type": "integer"},
                "levels": {"type": "integer"},
                "frame_id": {"type": "integer"},
                "variable_ref": {"type": "integer"},
                "pid": {"type": "integer"},
                "host": {"type": "string"},
                "port": {"type": "integer"},
                "memory_reference": {"type": "string"},
                "data": {"type": "string"},
                "offset": {"type": "integer"},
                "count": {"type": "integer"},
                "timeout": {"type": "integer"}
            }
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let action_name = input
            .get("action")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| anyhow!("debugger action is required"))?;
        match action_name.as_str() {
            "sessions" | "status" => {
                let sessions = self.manager.list().await;
                let metadata = json!({"action": action_name, "sessions": sessions});
                let output = serde_json::to_string_pretty(&json!({"sessions": sessions}))?;
                Ok(ToolOutput::new(output).with_title("Debugger sessions").with_metadata(metadata))
            }
            "launch" => {
                let cwd = resolve_cwd(&input, &ctx)?;
                let program = input
                    .get("program")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow!("debugger launch requires program"))?;
                let program = ctx.resolve_path(std::path::Path::new(program));
                let args = input
                    .get("args")
                    .and_then(Value::as_array)
                    .map(|args| args.iter().filter_map(Value::as_str).map(str::to_owned).collect())
                    .unwrap_or_default();
                let snapshot = self
                    .manager
                    .launch(LaunchRequest {
                        adapter: string_field(&input, "adapter"),
                        program,
                        args,
                        cwd,
                        parent_session_id: string_field(&input, "parent_session_id"),
                    })
                    .await?;
                render_result(&action_name, json!(snapshot))
            }
            "attach" => {
                let cwd = resolve_cwd(&input, &ctx)?;
                let snapshot = self
                    .manager
                    .attach(AttachRequest {
                        adapter: string_field(&input, "adapter"),
                        cwd,
                        host: string_field(&input, "host"),
                        port: input.get("port").and_then(Value::as_u64).map(|value| value as u16),
                        pid: input.get("pid").and_then(Value::as_u64).map(|value| value as u32),
                        parent_session_id: string_field(&input, "parent_session_id"),
                    })
                    .await?;
                render_result(&action_name, json!(snapshot))
            }
            _ => {
                let action = parse_action(&action_name)?;
                let session_id = resolve_session_id(&input, &self.manager).await?;
                let arguments = dap_arguments(&action_name, &input);
                let result = self.manager.execute(&session_id, action, arguments, None).await?;
                render_result(&action_name, result)
            }
        }
    }
}

fn parse_action(action: &str) -> Result<Action> {
    let action = match action {
        "sessions" => Action::Sessions,
        "status" => Action::Status,
        "launch" => Action::Launch,
        "attach" => Action::Attach,
        "set_breakpoint" => Action::SetBreakpoint,
        "remove_breakpoint" => Action::RemoveBreakpoint,
        "continue" => Action::Continue,
        "pause" => Action::Pause,
        "step_over" => Action::StepOver,
        "step_in" => Action::StepIn,
        "step_out" => Action::StepOut,
        "threads" => Action::Threads,
        "stack_trace" => Action::StackTrace,
        "scopes" => Action::Scopes,
        "variables" => Action::Variables,
        "evaluate" => Action::Evaluate,
        "output" => Action::Output,
        "stop" => Action::Stop,
        "disconnect" => Action::Disconnect,
        "read_memory" => Action::ReadMemory,
        "write_memory" => Action::WriteMemory,
        "modules" => Action::Modules,
        other => return Err(anyhow!("unknown debugger action '{other}'")),
    };
    Ok(action)
}

fn resolve_cwd(input: &Value, ctx: &ToolContext) -> Result<PathBuf> {
    if let Some(cwd) = input.get("cwd").and_then(Value::as_str) {
        return Ok(ctx.resolve_path(std::path::Path::new(cwd)));
    }
    ctx.working_dir
        .clone()
        .ok_or_else(|| anyhow!("debugger requires a session working directory"))
}

fn string_field(input: &Value, key: &str) -> Option<String> {
    input.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn dap_arguments(action: &str, input: &Value) -> Value {
    let mut arguments = Map::new();
    let copy_string = |source: &str, target: &str, arguments: &mut Map<String, Value>| {
        if let Some(value) = input.get(source).and_then(Value::as_str) {
            arguments.insert(target.to_owned(), Value::String(value.to_owned()));
        }
    };
    let copy_number = |source: &str, target: &str, arguments: &mut Map<String, Value>| {
        if let Some(value) = input.get(source).and_then(Value::as_i64) {
            arguments.insert(target.to_owned(), Value::from(value));
        }
    };

    match action {
        "set_breakpoint" => {
            if let Some(file) = input.get("file").and_then(Value::as_str) {
                arguments.insert("source".into(), json!({"path": file}));
            }
            let mut breakpoint = Map::new();
            copy_number("line", "line", &mut breakpoint);
            copy_string("condition", "condition", &mut breakpoint);
            arguments.insert("breakpoints".into(), Value::Array(vec![Value::Object(breakpoint)]));
        }
        "remove_breakpoint" => {
            if let Some(file) = input.get("file").and_then(Value::as_str) {
                arguments.insert("source".into(), json!({"path": file}));
            }
            arguments.insert("breakpoints".into(), Value::Array(Vec::new()));
        }
        "continue" | "pause" | "step_over" | "step_in" | "step_out" => {
            copy_number("thread_id", "threadId", &mut arguments);
        }
        "stack_trace" => {
            copy_number("thread_id", "threadId", &mut arguments);
            copy_number("start_frame", "startFrame", &mut arguments);
            copy_number("levels", "levels", &mut arguments);
        }
        "scopes" => copy_number("frame_id", "frameId", &mut arguments),
        "variables" => copy_number("variable_ref", "variablesReference", &mut arguments),
        "evaluate" => {
            copy_string("expression", "expression", &mut arguments);
            copy_number("frame_id", "frameId", &mut arguments);
            copy_string("context", "context", &mut arguments);
        }
        "read_memory" | "write_memory" => {
            copy_string("memory_reference", "memoryReference", &mut arguments);
            copy_string("data", "data", &mut arguments);
            copy_number("offset", "offset", &mut arguments);
            copy_number("count", "count", &mut arguments);
        }
        _ => {}
    }
    Value::Object(arguments)
}

async fn resolve_session_id(input: &Value, manager: &DapSessionManager) -> Result<String> {
    if let Some(id) = input.get("session_id").and_then(Value::as_str) {
        return Ok(id.to_owned());
    }
    let sessions = manager.list().await;
    sessions
        .last()
        .map(|session| session.id.clone())
        .ok_or_else(|| anyhow!("debugger action requires session_id because no debugger session exists"))
}

fn render_result(action: &str, result: Value) -> Result<ToolOutput> {
    let metadata = json!({"action": action, "result": result});
    let output = serde_json::to_string_pretty(&metadata)?;
    Ok(ToolOutput::new(output).with_title(format!("Debugger {action}")).with_metadata(metadata))
}
