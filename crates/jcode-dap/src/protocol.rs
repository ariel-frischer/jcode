use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DapRequestMessage {
    pub seq: u64,
    #[serde(rename = "type")]
    pub message_type: String,
    pub command: String,
    #[serde(default)]
    pub arguments: Value,
}

impl DapRequestMessage {
    pub fn new(seq: u64, command: impl Into<String>, arguments: Value) -> Self {
        Self {
            seq,
            message_type: "request".to_owned(),
            command: command.into(),
            arguments,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DapResponseMessage {
    pub seq: u64,
    #[serde(rename = "type")]
    pub message_type: String,
    pub request_seq: u64,
    pub success: bool,
    pub command: String,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub body: Option<Value>,
}

impl DapResponseMessage {
    pub fn success(seq: u64, request_seq: u64, command: impl Into<String>, body: Value) -> Self {
        Self {
            seq,
            message_type: "response".to_owned(),
            request_seq,
            success: true,
            command: command.into(),
            message: None,
            body: Some(body),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DapEventMessage {
    pub seq: u64,
    #[serde(rename = "type")]
    pub message_type: String,
    pub event: String,
    #[serde(default)]
    pub body: Option<Value>,
}

impl DapEventMessage {
    pub fn new(seq: u64, event: impl Into<String>, body: Value) -> Self {
        Self {
            seq,
            message_type: "event".to_owned(),
            event: event.into(),
            body: Some(body),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct DapCapabilities {
    #[serde(flatten)]
    pub values: serde_json::Map<String, Value>,
}

impl DapCapabilities {
    pub fn supports(&self, name: &str) -> bool {
        self.values
            .get(name)
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }
}
