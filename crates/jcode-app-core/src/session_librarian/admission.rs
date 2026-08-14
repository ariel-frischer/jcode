use super::{AdmittedSessionContent, LibrarianFailure, LibrarianFailureStage};
use jcode_base::{
    config::{LibrarianAdmissionCaps, LibrarianBudgets},
    message::{ContentBlock, Role, redact_secrets},
    session::Session,
};
use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

const ADMISSION_FORMAT_VERSION: u32 = 1;
const ERROR_EXCERPT_BYTES: usize = 256;

#[derive(Clone, Debug)]
struct PendingToolUse {
    operation: String,
    path: Option<String>,
    intent: Option<String>,
    canonical_input: Vec<u8>,
}

#[derive(Clone, Debug, Serialize)]
struct AdmissionItem {
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    operation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    counts: Option<BTreeMap<String, u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    intent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    input_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    estimated_tokens: u32,
}

impl AdmissionItem {
    fn text(role: &'static str, text: String) -> Self {
        Self {
            kind: "text",
            role: Some(role),
            text: Some(text),
            operation: None,
            path: None,
            status: None,
            counts: None,
            intent: None,
            input_sha256: None,
            result_sha256: None,
            error: None,
            estimated_tokens: 0,
        }
    }

    fn receipt(tool_use: PendingToolUse, result: &str, is_error: bool) -> Self {
        let redacted_result = redact_secrets(result);
        let result_sha256 = sha256(redacted_result.as_bytes());
        let counts = extract_counts(&redacted_result);
        let error = is_error.then(|| truncate_utf8(&redacted_result, ERROR_EXCERPT_BYTES));

        Self {
            kind: "receipt",
            role: None,
            text: None,
            operation: Some(tool_use.operation),
            path: tool_use.path,
            status: Some(if is_error { "error" } else { "success" }),
            counts: Some(counts),
            intent: tool_use.intent,
            input_sha256: Some(sha256(&tool_use.canonical_input)),
            result_sha256: Some(result_sha256),
            error,
            estimated_tokens: 0,
        }
    }

    fn refresh_estimate(&mut self) {
        self.estimated_tokens = 0;
        self.estimated_tokens = serialized_len(self) as u32;
        self.estimated_tokens = serialized_len(self) as u32;
    }

    fn fit_item_cap(&mut self, cap: u32) -> bool {
        self.refresh_estimate();
        while self.estimated_tokens > cap {
            let excess = (self.estimated_tokens - cap) as usize + 8;
            let target = self
                .text
                .as_ref()
                .map(String::len)
                .or_else(|| self.error.as_ref().map(String::len))
                .or_else(|| self.intent.as_ref().map(String::len));
            let Some(current_len) = target else {
                return false;
            };
            if current_len == 0 {
                return false;
            }
            let new_len = current_len.saturating_sub(excess.max(1));
            if let Some(text) = self.text.as_mut() {
                *text = truncate_utf8(text, new_len);
            } else if let Some(error) = self.error.as_mut() {
                *error = truncate_utf8(error, new_len);
            } else if let Some(intent) = self.intent.as_mut() {
                *intent = truncate_utf8(intent, new_len);
            }
            self.refresh_estimate();
        }
        true
    }
}

#[derive(Serialize)]
struct CanonicalAdmission<'a> {
    version: u32,
    session_id: &'a str,
    items: &'a [AdmissionItem],
}

pub(crate) fn admit_session(
    session: &Session,
    budgets: &LibrarianBudgets,
    caps: &LibrarianAdmissionCaps,
) -> Result<AdmittedSessionContent, LibrarianFailure> {
    let working_directory = session.working_dir.as_deref().map(Path::new);
    let mut pending_tools = HashMap::<String, PendingToolUse>::new();
    let mut receipt_identities = HashSet::<String>::new();
    let mut candidates = Vec::<AdmissionItem>::new();

    for message in &session.messages {
        let role = match message.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        };
        for block in &message.content {
            match block {
                ContentBlock::Text { text, .. } => {
                    if let Some(redacted) = eligible_text(text) {
                        candidates.push(AdmissionItem::text(role, redacted));
                    }
                }
                ContentBlock::ToolUse {
                    id, name, input, ..
                } => {
                    pending_tools.insert(id.clone(), pending_tool(name, input, working_directory)?);
                }
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => {
                    let Some(tool_use) = pending_tools.remove(tool_use_id) else {
                        continue;
                    };
                    let receipt =
                        AdmissionItem::receipt(tool_use, content, is_error.unwrap_or(false));
                    let identity = receipt_identity(&receipt);
                    if receipt_identities.insert(identity) {
                        candidates.push(receipt);
                    }
                }
                ContentBlock::Reasoning { .. }
                | ContentBlock::ReasoningTrace { .. }
                | ContentBlock::AnthropicThinking { .. }
                | ContentBlock::OpenAIReasoning { .. }
                | ContentBlock::Image { .. }
                | ContentBlock::OpenAICompaction { .. } => {}
            }
        }
    }

    let mut bounded = Vec::with_capacity(candidates.len());
    for mut item in candidates {
        if item.fit_item_cap(caps.max_item_tokens)
            && (item.kind != "receipt" || serialized_len(&item) <= caps.max_receipt_bytes)
        {
            bounded.push(item);
        }
    }

    let bounded = apply_category_caps(bounded, caps);
    let admitted = apply_global_cap(&session.id, bounded, budgets.max_input_tokens)?;
    if admitted.is_empty() {
        return Err(LibrarianFailure {
            stage: LibrarianFailureStage::Admission,
            code: "librarian_empty_content",
            message:
                "Session librarian found no eligible content within the configured input budget."
                    .to_string(),
            usage: None,
        });
    }

    let canonical_payload = serialize_payload(&session.id, &admitted)?;
    let input_tokens = conservative_token_count(&canonical_payload);
    if input_tokens > budgets.max_input_tokens {
        return Err(LibrarianFailure {
            stage: LibrarianFailureStage::Admission,
            code: "librarian_input_budget_exceeded",
            message:
                "Session librarian input could not be reduced below the configured token budget."
                    .to_string(),
            usage: None,
        });
    }

    Ok(AdmittedSessionContent {
        session_id: session.id.clone(),
        canonical_payload,
        input_tokens,
    })
}

fn eligible_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() || is_structurally_excluded(trimmed) {
        return None;
    }
    let redacted = redact_secrets(trimmed);
    (!redacted.trim().is_empty()).then_some(redacted)
}

fn is_structurally_excluded(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("<system-reminder>")
        || lower.starts_with("# agents.md")
        || (lower.starts_with("## skill:") && lower.contains("**base directory**"))
        || lower.starts_with("data:")
        || lower.contains(";base64,")
        || looks_like_base64_blob(text)
}

fn looks_like_base64_blob(text: &str) -> bool {
    if text.len() < 512 {
        return false;
    }
    let compact = text.bytes().filter(|byte| !byte.is_ascii_whitespace());
    let (eligible, total) = compact.fold((0usize, 0usize), |(eligible, total), byte| {
        (
            eligible
                + usize::from(byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'=')),
            total + 1,
        )
    });
    total > 0 && eligible.saturating_mul(100) / total >= 98
}

fn pending_tool(
    operation: &str,
    input: &Value,
    working_directory: Option<&Path>,
) -> Result<PendingToolUse, LibrarianFailure> {
    let redacted_input = redact_json(input);
    let canonical_input = serde_json::to_vec(&canonical_json(&redacted_input))
        .map_err(admission_serialization_failure)?;
    let path = first_string(input, &["file_path", "path", "file"])
        .and_then(|path| normalize_recorded_path(path, working_directory));
    let intent = first_string(input, &["intent"])
        .map(redact_secrets)
        .map(|value| truncate_utf8(&value, ERROR_EXCERPT_BYTES));

    Ok(PendingToolUse {
        operation: operation.to_string(),
        path,
        intent,
        canonical_input,
    })
}

fn first_string<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn redact_json(value: &Value) -> Value {
    match value {
        Value::String(value) => Value::String(redact_secrets(value)),
        Value::Array(values) => Value::Array(values.iter().map(redact_json).collect()),
        Value::Object(values) => Value::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), redact_json(value)))
                .collect(),
        ),
        value => value.clone(),
    }
}

fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(canonical_json).collect()),
        Value::Object(values) => {
            let ordered = values
                .iter()
                .map(|(key, value)| (key.clone(), canonical_json(value)))
                .collect::<BTreeMap<_, _>>();
            Value::Object(ordered.into_iter().collect::<Map<_, _>>())
        }
        value => value.clone(),
    }
}

fn normalize_recorded_path(raw: &str, working_directory: Option<&Path>) -> Option<String> {
    let path = Path::new(raw);
    let path = if path.is_absolute() {
        let root = working_directory.filter(|root| root.is_absolute())?;
        path.strip_prefix(root).ok()?
    } else {
        path
    };
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            Component::Normal(part) => normalized.push(part),
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (!normalized.as_os_str().is_empty()).then(|| normalized.to_string_lossy().replace('\\', "/"))
}

fn extract_counts(result: &str) -> BTreeMap<String, u64> {
    let mut counts = BTreeMap::new();
    for token in result.split_whitespace() {
        let token = token.trim_matches(|character: char| {
            !character.is_ascii_alphanumeric() && character != '_' && character != '='
        });
        let Some((key, value)) = token.split_once('=') else {
            continue;
        };
        if !key.is_empty()
            && key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            && let Ok(value) = value
                .trim_matches(|character: char| !character.is_ascii_digit())
                .parse()
        {
            counts.insert(key.to_string(), value);
        }
    }
    counts
}

fn receipt_identity(receipt: &AdmissionItem) -> String {
    format!(
        "{}\0{}\0{}\0{}",
        receipt.operation.as_deref().unwrap_or(""),
        receipt.path.as_deref().unwrap_or(""),
        receipt.input_sha256.as_deref().unwrap_or(""),
        receipt.result_sha256.as_deref().unwrap_or(""),
    )
}

fn apply_category_caps(
    items: Vec<AdmissionItem>,
    caps: &LibrarianAdmissionCaps,
) -> Vec<AdmissionItem> {
    let mut keep = vec![false; items.len()];
    let mut file_tokens = HashMap::<String, u32>::new();
    let mut tool_tokens = 0u32;

    for (index, item) in items.iter().enumerate().rev() {
        if item.kind != "receipt" {
            keep[index] = true;
            continue;
        }
        if tool_tokens.saturating_add(item.estimated_tokens) > caps.max_tool_category_tokens {
            continue;
        }
        if let Some(path) = item.path.as_ref() {
            let used = file_tokens.get(path).copied().unwrap_or(0);
            if used.saturating_add(item.estimated_tokens) > caps.max_normalized_file_tokens {
                continue;
            }
            file_tokens.insert(path.clone(), used + item.estimated_tokens);
        }
        tool_tokens += item.estimated_tokens;
        keep[index] = true;
    }

    items
        .into_iter()
        .zip(keep)
        .filter_map(|(item, keep)| keep.then_some(item))
        .collect()
}

fn apply_global_cap(
    session_id: &str,
    items: Vec<AdmissionItem>,
    max_input_tokens: u32,
) -> Result<Vec<AdmissionItem>, LibrarianFailure> {
    let mut selected = Vec::<AdmissionItem>::new();
    for item in items.into_iter().rev() {
        let mut candidate = Vec::with_capacity(selected.len() + 1);
        candidate.push(item.clone());
        candidate.extend(selected.iter().cloned());
        if conservative_token_count(&serialize_payload(session_id, &candidate)?) <= max_input_tokens
        {
            selected = candidate;
        }
    }
    Ok(selected)
}

fn serialize_payload(
    session_id: &str,
    items: &[AdmissionItem],
) -> Result<Vec<u8>, LibrarianFailure> {
    serde_json::to_vec(&CanonicalAdmission {
        version: ADMISSION_FORMAT_VERSION,
        session_id,
        items,
    })
    .map_err(admission_serialization_failure)
}

fn admission_serialization_failure(error: serde_json::Error) -> LibrarianFailure {
    LibrarianFailure {
        stage: LibrarianFailureStage::Admission,
        code: "librarian_admission_serialization_failed",
        message: format!("Session librarian could not serialize bounded admitted content: {error}"),
        usage: None,
    }
}

fn conservative_token_count(bytes: &[u8]) -> u32 {
    u32::try_from(bytes.len()).unwrap_or(u32::MAX)
}

fn serialized_len(value: &impl Serialize) -> usize {
    serde_json::to_vec(value).map_or(usize::MAX, |bytes| bytes.len())
}

fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        encoded.push_str(&format!("{byte:02x}"));
    }
    encoded
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let mut boundary = max_bytes.min(value.len());
    while boundary > 0 && !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value[..boundary].to_string()
}
