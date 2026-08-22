use crate::config::{ResolvedServer, ServerConfig, ServerRegistry};
use crate::diagnostics::DiagnosticStore;
use crate::error::{LspError, Result};
use crate::protocol::{Diagnostic, ServerCapabilities, file_uri, normalize_diagnostics};
use crate::transport::{LspClient, Notification};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, broadcast};
use tokio::time::sleep;

pub type SessionId = String;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Starting,
    Ready,
    Degraded,
    Terminated,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionSnapshot {
    pub id: SessionId,
    pub server: String,
    pub root: PathBuf,
    pub status: SessionStatus,
    pub capabilities: ServerCapabilities,
    pub document_count: usize,
    pub diagnostic_count: usize,
    pub deferred_feedback: bool,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EditFeedback {
    pub session_id: SessionId,
    pub status: String,
    pub diagnostics: Vec<Diagnostic>,
    pub formatted_text: Option<String>,
    pub deferred: bool,
}

struct LiveSession {
    client: Arc<LspClient>,
    notifications: broadcast::Receiver<Notification>,
    child: Option<Child>,
    snapshot: SessionSnapshot,
    config: ServerConfig,
    documents: BTreeMap<String, i64>,
    diagnostics: DiagnosticStore,
    last_used: Instant,
}

#[derive(Clone)]
pub struct LspSessionManager {
    sessions: Arc<Mutex<BTreeMap<SessionId, LiveSession>>>,
    registry: Option<ServerRegistry>,
}

impl Default for LspSessionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl LspSessionManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(BTreeMap::new())),
            registry: None,
        }
    }

    pub fn with_registry(registry: ServerRegistry) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(BTreeMap::new())),
            registry: Some(registry),
        }
    }

    pub fn shared() -> Arc<Self> {
        static SHARED: OnceLock<Arc<LspSessionManager>> = OnceLock::new();
        SHARED.get_or_init(|| Arc::new(Self::new())).clone()
    }

    pub async fn list(&self) -> Vec<SessionSnapshot> {
        self.sessions
            .lock()
            .await
            .values()
            .map(|session| session.snapshot.clone())
            .collect()
    }

    pub async fn get(&self, session_id: &str) -> Option<SessionSnapshot> {
        self.sessions
            .lock()
            .await
            .get(session_id)
            .map(|session| session.snapshot.clone())
    }

    pub async fn start_for_file(
        &self,
        cwd: &Path,
        file: &Path,
        explicit: Option<&str>,
    ) -> Result<Option<SessionId>> {
        let registry = match &self.registry {
            Some(registry) => registry.clone(),
            None => ServerRegistry::load(cwd)?,
        };
        let Some(resolved) = registry.select(cwd, file, explicit)? else {
            return Ok(None);
        };
        {
            let sessions = self.sessions.lock().await;
            if let Some((id, _)) = sessions.iter().find(|(_, session)| {
                session.snapshot.root == resolved.root
                    && session.snapshot.server == resolved.name
                    && session.snapshot.status != SessionStatus::Terminated
            }) {
                return Ok(Some(id.clone()));
            }
        }
        let session = spawn_session(&resolved).await?;
        let id = session.snapshot.id.clone();
        self.sessions.lock().await.insert(id.clone(), session);
        Ok(Some(id))
    }

    pub async fn sync_document(
        &self,
        session_id: &str,
        file: &Path,
        text: &str,
        version: i64,
    ) -> Result<()> {
        let (client, request_timeout, language_id, uri, first_open, version) = {
            let mut sessions = self.sessions.lock().await;
            let session = sessions
                .get_mut(session_id)
                .ok_or_else(|| LspError::Server("unknown LSP session".into()))?;
            session.last_used = Instant::now();
            drain_notifications(session);
            let uri = file_uri(file);
            let first_open = !session.documents.contains_key(&uri);
            let version = if version > 0 {
                version
            } else {
                session.documents.get(&uri).copied().unwrap_or(0) + 1
            };
            session.documents.insert(uri.clone(), version);
            session.snapshot.document_count = session.documents.len();
            (
                session.client.clone(),
                Duration::from_millis(session.config.request_timeout_ms),
                session.config.language_id.clone(),
                uri,
                first_open,
                version,
            )
        };
        if first_open {
            client.notify("textDocument/didOpen", json!({"textDocument": {"uri": uri, "languageId": language_id, "version": version, "text": text}}), request_timeout).await?;
        } else {
            client.notify("textDocument/didChange", json!({"textDocument": {"uri": uri, "version": version}, "contentChanges": [{"text": text}]}), request_timeout).await?;
        }
        Ok(())
    }

    pub async fn feedback_after_edit(
        &self,
        cwd: &Path,
        file: &Path,
        text: &str,
        version: i64,
    ) -> Result<Option<EditFeedback>> {
        let Some(session_id) = self.start_for_file(cwd, file, None).await? else {
            return Ok(None);
        };
        self.sync_document(&session_id, file, text, version).await?;
        let request_timeout = self.request_timeout(&session_id).await?;
        let deferred = if request_timeout <= Duration::from_millis(50) {
            sleep(request_timeout).await;
            true
        } else {
            sleep(Duration::from_millis(25)).await;
            false
        };
        let uri = file_uri(file);
        let diagnostics = self.diagnostics(&session_id, Some(&uri)).await?;
        Ok(Some(EditFeedback {
            session_id,
            status: if deferred {
                "deferred".into()
            } else {
                "ready".into()
            },
            diagnostics,
            formatted_text: None,
            deferred,
        }))
    }

    pub async fn diagnostics(
        &self,
        session_id: &str,
        uri: Option<&str>,
    ) -> Result<Vec<Diagnostic>> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| LspError::Server("unknown LSP session".into()))?;
        drain_notifications(session);
        session.snapshot.diagnostic_count = session.diagnostics.count();
        Ok(uri.map_or_else(
            || session.diagnostics.all(),
            |uri| session.diagnostics.get(uri),
        ))
    }

    pub async fn execute(
        &self,
        session_id: &str,
        action: LspAction,
        params: Value,
    ) -> Result<Value> {
        match action {
            LspAction::Status => {
                return Ok(serde_json::to_value(self.get(session_id).await).unwrap_or(Value::Null));
            }
            LspAction::Diagnostics => {
                return Ok(serde_json::to_value(
                    self.diagnostics(session_id, params.get("uri").and_then(Value::as_str))
                        .await?,
                )
                .unwrap_or(Value::Null));
            }
            LspAction::Capabilities => {
                return Ok(serde_json::to_value(
                    self.get(session_id)
                        .await
                        .map(|snapshot| snapshot.capabilities),
                )
                .unwrap_or(Value::Null));
            }
            _ => {}
        }
        let (client, timeout_duration, capabilities) = {
            let sessions = self.sessions.lock().await;
            let session = sessions
                .get(session_id)
                .ok_or_else(|| LspError::Server("unknown LSP session".into()))?;
            (
                session.client.clone(),
                Duration::from_millis(session.config.request_timeout_ms),
                session.snapshot.capabilities.clone(),
            )
        };
        let (method, required) = action.method_and_capability();
        if !required(capabilities) {
            return Err(LspError::Unsupported(method.into()));
        }
        let result = client.request(method, params, timeout_duration).await?;
        Ok(result)
    }

    async fn request_timeout(&self, session_id: &str) -> Result<Duration> {
        let sessions = self.sessions.lock().await;
        let session = sessions
            .get(session_id)
            .ok_or_else(|| LspError::Server("unknown LSP session".into()))?;
        Ok(Duration::from_millis(session.config.request_timeout_ms))
    }

    pub async fn disconnect(&self, session_id: &str) -> Result<()> {
        let mut session = self
            .sessions
            .lock()
            .await
            .remove(session_id)
            .ok_or_else(|| LspError::Server("unknown LSP session".into()))?;
        let timeout_duration = Duration::from_millis(session.config.request_timeout_ms);
        if session
            .client
            .request("shutdown", Value::Null, timeout_duration)
            .await
            .is_err()
        {
            session.snapshot.last_error = Some("shutdown request failed during disconnect".into());
        }
        if session
            .client
            .notify("exit", Value::Null, timeout_duration)
            .await
            .is_err()
        {
            session.snapshot.last_error = Some("exit notification failed during disconnect".into());
        }
        if let Some(mut child) = session.child.take() {
            if child.kill().await.is_err() {
                session.snapshot.last_error =
                    Some("adapter process kill failed during disconnect".into());
            }
            if child.wait().await.is_err() {
                session.snapshot.last_error =
                    Some("adapter process wait failed during disconnect".into());
            }
        }
        Ok(())
    }

    pub async fn reap_idle(&self, idle_for: Duration) -> usize {
        let now = Instant::now();
        let ids: Vec<String> = self
            .sessions
            .lock()
            .await
            .iter()
            .filter(|(_, session)| now.duration_since(session.last_used) >= idle_for)
            .map(|(id, _)| id.clone())
            .collect();
        let mut reaped = 0;
        for id in ids {
            if self.disconnect(&id).await.is_ok() {
                reaped += 1;
            }
        }
        reaped
    }
}

#[derive(Debug, Clone, Copy)]
pub enum LspAction {
    Hover,
    Definition,
    References,
    Symbols,
    Formatting,
    Rename,
    CodeActions,
    Status,
    Diagnostics,
    Capabilities,
}

impl LspAction {
    fn method_and_capability(self) -> (&'static str, fn(ServerCapabilities) -> bool) {
        match self {
            Self::Hover => ("textDocument/hover", |capabilities| capabilities.hover),
            Self::Definition => ("textDocument/definition", |capabilities| {
                capabilities.definition
            }),
            Self::References => ("textDocument/references", |capabilities| {
                capabilities.references
            }),
            Self::Symbols => ("textDocument/documentSymbol", |capabilities| {
                capabilities.document_symbol
            }),
            Self::Formatting => ("textDocument/formatting", |capabilities| {
                capabilities.formatting
            }),
            Self::Rename => ("textDocument/rename", |capabilities| capabilities.rename),
            Self::CodeActions => ("textDocument/codeAction", |capabilities| {
                capabilities.code_action
            }),
            Self::Status | Self::Diagnostics | Self::Capabilities => ("", |_| true),
        }
    }
}

async fn spawn_session(resolved: &ResolvedServer) -> Result<LiveSession> {
    let mut command = Command::new(&resolved.config.command);
    command
        .args(&resolved.config.args)
        .current_dir(&resolved.root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| LspError::Server("LSP server did not expose stdout".into()))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| LspError::Server("LSP server did not expose stdin".into()))?;
    let client = LspClient::from_stream(stdout, stdin).await;
    let timeout_duration = Duration::from_millis(resolved.config.startup_timeout_ms);
    let initialize = client.request("initialize", json!({
        "processId": std::process::id(),
        "rootUri": file_uri(&resolved.root),
        "capabilities": {},
        "workspaceFolders": [{"uri": file_uri(&resolved.root), "name": resolved.root.file_name().and_then(|name| name.to_str()).unwrap_or("workspace")}]
    }), timeout_duration).await?;
    client
        .notify("initialized", json!({}), timeout_duration)
        .await?;
    let capabilities = ServerCapabilities::from_initialize(&initialize);
    let id = uuid::Uuid::new_v4().to_string();
    let snapshot = SessionSnapshot {
        id,
        server: resolved.name.clone(),
        root: resolved.root.clone(),
        status: SessionStatus::Ready,
        capabilities,
        document_count: 0,
        diagnostic_count: 0,
        deferred_feedback: false,
        last_error: None,
    };
    Ok(LiveSession {
        notifications: client.subscribe(),
        client,
        child: Some(child),
        snapshot,
        config: resolved.config.clone(),
        documents: BTreeMap::new(),
        diagnostics: DiagnosticStore::default(),
        last_used: Instant::now(),
    })
}

fn drain_notifications(session: &mut LiveSession) {
    loop {
        match session.notifications.try_recv() {
            Ok(notification) if notification.method == "textDocument/publishDiagnostics" => {
                if let Some(uri) = notification.params.get("uri").and_then(Value::as_str) {
                    let version = session.documents.get(uri).copied().unwrap_or(0);
                    if let Ok(diagnostics) =
                        normalize_diagnostics(&notification.params, version, 128 * 1024)
                    {
                        session.diagnostics.replace(uri, diagnostics);
                    }
                }
            }
            Ok(_) => {}
            Err(
                broadcast::error::TryRecvError::Empty | broadcast::error::TryRecvError::Lagged(_),
            ) => break,
            Err(broadcast::error::TryRecvError::Closed) => break,
        }
    }
    session.snapshot.diagnostic_count = session.diagnostics.count();
}
