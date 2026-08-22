use crate::error::{LspError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type RequestId = u64;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<RequestId>,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: RequestId,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(default)]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Information,
    Hint,
}

impl Severity {
    fn from_value(value: Option<&Value>) -> Option<Self> {
        match value.and_then(Value::as_u64) {
            Some(1) => Some(Self::Error),
            Some(2) => Some(Self::Warning),
            Some(3) => Some(Self::Information),
            Some(4) => Some(Self::Hint),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Diagnostic {
    pub uri: String,
    pub range: Range,
    pub severity: Option<Severity>,
    pub message: String,
    pub source: Option<String>,
    pub code: Option<String>,
    pub version: Option<i64>,
    pub stale: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ServerCapabilities {
    pub hover: bool,
    pub definition: bool,
    pub references: bool,
    pub document_symbol: bool,
    pub formatting: bool,
    pub rename: bool,
    pub code_action: bool,
}

impl ServerCapabilities {
    pub fn from_initialize(value: &Value) -> Self {
        let capabilities = value.get("capabilities").unwrap_or(value);
        Self {
            hover: capabilities
                .get("hoverProvider")
                .is_some_and(value_is_enabled),
            definition: capabilities
                .get("definitionProvider")
                .is_some_and(value_is_enabled),
            references: capabilities
                .get("referencesProvider")
                .is_some_and(value_is_enabled),
            document_symbol: capabilities
                .get("documentSymbolProvider")
                .is_some_and(value_is_enabled),
            formatting: capabilities
                .get("documentFormattingProvider")
                .is_some_and(value_is_enabled),
            rename: capabilities
                .get("renameProvider")
                .is_some_and(value_is_enabled),
            code_action: capabilities
                .get("codeActionProvider")
                .is_some_and(value_is_enabled),
        }
    }
}

fn value_is_enabled(value: &Value) -> bool {
    match value {
        Value::Bool(value) => *value,
        Value::Object(_) => true,
        _ => false,
    }
}

pub fn normalize_diagnostics(
    raw: &Value,
    current_version: i64,
    max_message_bytes: usize,
) -> Result<Vec<Diagnostic>> {
    let uri = raw
        .get("uri")
        .and_then(Value::as_str)
        .ok_or_else(|| LspError::Protocol("diagnostics notification has no uri".into()))?;
    let published_version = raw.get("version").and_then(Value::as_i64);
    let stale = published_version != Some(current_version);
    let diagnostics = raw
        .get("diagnostics")
        .and_then(Value::as_array)
        .ok_or_else(|| LspError::Protocol("diagnostics notification has no diagnostics".into()))?;

    diagnostics
        .iter()
        .map(|diagnostic| {
            let range = parse_range(diagnostic.get("range"))?;
            let message = diagnostic
                .get("message")
                .and_then(Value::as_str)
                .ok_or_else(|| LspError::Protocol("diagnostic has no message".into()))?;
            Ok(Diagnostic {
                uri: uri.to_owned(),
                range,
                severity: Severity::from_value(diagnostic.get("severity")),
                message: truncate_utf8(message, max_message_bytes),
                source: diagnostic
                    .get("source")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                code: diagnostic.get("code").map(value_to_string),
                version: published_version,
                stale,
            })
        })
        .collect()
}

fn parse_range(value: Option<&Value>) -> Result<Range> {
    let value = value.ok_or_else(|| LspError::Protocol("diagnostic has no range".into()))?;
    Ok(Range {
        start: parse_position(value.get("start"))?,
        end: parse_position(value.get("end"))?,
    })
}

fn parse_position(value: Option<&Value>) -> Result<Position> {
    let value = value.ok_or_else(|| LspError::Protocol("range has no position".into()))?;
    Ok(Position {
        line: value
            .get("line")
            .and_then(Value::as_u64)
            .and_then(u32_option)
            .ok_or_else(|| LspError::Protocol("position has invalid line".into()))?,
        character: value
            .get("character")
            .and_then(Value::as_u64)
            .and_then(u32_option)
            .ok_or_else(|| LspError::Protocol("position has invalid character".into()))?,
    })
}

#[allow(clippy::manual_ok_err)]
fn u32_option(value: u64) -> Option<u32> {
    match u32::try_from(value) {
        Ok(value) => Some(value),
        Err(_) => None,
    }
}

fn value_to_string(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

pub fn file_uri(path: &std::path::Path) -> String {
    let path = path.to_string_lossy();
    if path.starts_with('/') {
        format!("file://{path}")
    } else {
        format!("file:///{path}")
    }
}
