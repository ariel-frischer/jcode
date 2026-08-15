use crate::config::{AdapterRegistry, ResolvedAdapter, TransportMode};
use crate::error::{DapError, Result};
use crate::policy::{Action, DapPolicy};
use crate::protocol::DapCapabilities;
use crate::transport::DapClient;
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub type SessionId = String;

#[derive(Debug, Clone)]
pub struct LaunchRequest {
    pub adapter: Option<String>,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub parent_session_id: Option<SessionId>,
}

#[derive(Debug, Clone)]
pub struct AttachRequest {
    pub adapter: Option<String>,
    pub cwd: PathBuf,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub pid: Option<u32>,
    pub parent_session_id: Option<SessionId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Launching,
    Configuring,
    Stopped,
    Running,
    Terminated,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionSnapshot {
    pub id: SessionId,
    pub adapter: String,
    pub cwd: PathBuf,
    pub program: Option<PathBuf>,
    pub status: SessionStatus,
    pub stop_reason: Option<String>,
    pub output: String,
    pub output_truncated: bool,
    pub capabilities: BTreeMap<String, bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<SessionId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub child_session_ids: Vec<SessionId>,
}

struct LiveSession {
    client: Arc<DapClient>,
    snapshot: SessionSnapshot,
    capabilities: DapCapabilities,
    policy: DapPolicy,
    last_used: Instant,
}

#[derive(Clone)]
pub struct DapSessionManager {
    sessions: Arc<Mutex<BTreeMap<SessionId, LiveSession>>>,
    policy: DapPolicy,
    registry: Option<AdapterRegistry>,
    load_policy_from_config: bool,
}

impl Default for DapSessionManager {
    fn default() -> Self {
        Self::with_policy(DapPolicy::default())
    }
}

impl DapSessionManager {
    pub fn new() -> Self {
        Self {
            load_policy_from_config: true,
            ..Self::default()
        }
    }

    pub fn with_policy(policy: DapPolicy) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(BTreeMap::new())),
            policy,
            registry: None,
            load_policy_from_config: false,
        }
    }

    pub fn with_registry(policy: DapPolicy, registry: AdapterRegistry) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(BTreeMap::new())),
            policy,
            registry: Some(registry),
            load_policy_from_config: false,
        }
    }

    pub async fn list(&self) -> Vec<SessionSnapshot> {
        let sessions = self.sessions.lock().await;
        sessions.values().map(|session| session.snapshot.clone()).collect()
    }

    pub async fn get(&self, id: &str) -> Option<SessionSnapshot> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions.get_mut(id)?;
        session.last_used = Instant::now();
        Some(session.snapshot.clone())
    }

    /// Dispose terminated or idle sessions so adapter processes do not outlive
    /// their usefulness. Callers choose the cleanup cadence explicitly.
    pub async fn reap_idle(&self, idle_for: Duration) -> usize {
        let now = Instant::now();
        let ids = {
            let sessions = self.sessions.lock().await;
            sessions
                .iter()
                .filter(|(_, session)| {
                    matches!(session.snapshot.status, SessionStatus::Terminated)
                        || now.duration_since(session.last_used) >= idle_for
                })
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>()
        };
        let mut reaped = 0;
        for id in ids {
            if self.disconnect_internal(&id).await.is_ok() {
                reaped += 1;
            }
        }
        reaped
    }

    pub async fn launch(&self, request: LaunchRequest) -> Result<SessionSnapshot> {
        if request.program.as_os_str().is_empty() {
            return Err(DapError::Session("launch requires an explicit program path".into()));
        }
        let cwd = normalize_cwd(&request.cwd);
        let policy = self.policy_for(&cwd)?;
        policy.check(Action::Launch)?;
        let registry = match &self.registry {
            Some(registry) => registry.clone(),
            None => AdapterRegistry::load(&cwd)?,
        };
        let adapter = registry.select_launch(&request.program, &cwd, request.adapter.as_deref())?;
        let client = spawn_adapter(&adapter, &cwd, &policy).await?;
        let capabilities = match client.initialize(policy.request_timeout).await {
            Ok(capabilities) => capabilities,
            Err(error) => {
                client.dispose().await;
                return Err(error);
            }
        };
        let id = Uuid::new_v4().to_string();
        let parent_session_id = request.parent_session_id.clone();
        let session = LiveSession {
            client: client.clone(),
            snapshot: SessionSnapshot {
                id: id.clone(),
                adapter: adapter.name.clone(),
                cwd: cwd.clone(),
                program: Some(request.program.clone()),
                status: SessionStatus::Configuring,
                stop_reason: None,
                output: String::new(),
                output_truncated: false,
                capabilities: capability_map(&capabilities),
                parent_session_id: parent_session_id.clone(),
                child_session_ids: Vec::new(),
            },
            capabilities: capabilities.clone(),
            policy: policy.clone(),
            last_used: Instant::now(),
        };
        self.sessions.lock().await.insert(id.clone(), session);
        self.add_child_relation(parent_session_id.as_deref(), &id).await;
        self.spawn_event_loop(id.clone(), client.clone(), policy.max_output_bytes);
        let launch_args = merge_defaults(&adapter.config.launch_defaults, serde_json::json!({
            "request": "launch",
            "program": request.program,
            "cwd": cwd,
            "args": request.args,
        }));
        let response = client.request("launch", launch_args, policy.request_timeout, None).await;
        if let Err(error) = response {
            let _ = self.disconnect_internal(&id).await;
            return Err(error);
        }
        if capabilities.supports("supportsConfigurationDoneRequest")
            && let Err(error) = client
                .request("configurationDone", serde_json::json!({}), policy.request_timeout, None)
                .await
        {
            let _ = self.disconnect_internal(&id).await;
            return Err(error);
        }
        self.update_status_if_configuring(&id, SessionStatus::Running).await;
        self.get(&id).await.ok_or_else(|| DapError::Session("session disappeared after launch".into()))
    }

    pub async fn attach(&self, request: AttachRequest) -> Result<SessionSnapshot> {
        if request.pid.is_none() && request.port.is_none() {
            return Err(DapError::Session("attach requires a pid or host/port endpoint".into()));
        }
        let cwd = normalize_cwd(&request.cwd);
        let policy = self.policy_for(&cwd)?;
        policy.check(Action::Attach)?;
        let registry = match &self.registry {
            Some(registry) => registry.clone(),
            None => AdapterRegistry::load(&cwd)?,
        };
        let adapter_name = request.adapter.as_deref().ok_or_else(|| DapError::Config("attach requires an explicit adapter".into()))?;
        let adapter = registry.resolve(adapter_name, &cwd)?;
        let client = if let Some(port) = request.port {
            DapClient::connect_tcp(
                request.host.as_deref().unwrap_or("127.0.0.1"),
                port,
                policy.startup_timeout,
            )
            .await?
        } else {
            match adapter.config.transport {
                TransportMode::Stdio => spawn_adapter(&adapter, &cwd, &policy).await?,
                TransportMode::Tcp | TransportMode::Socket => {
                    return Err(DapError::Unsupported(
                        "attach requires a TCP port for socket transports".into(),
                    ));
                }
            }
        };
        let capabilities = match client.initialize(policy.request_timeout).await {
            Ok(capabilities) => capabilities,
            Err(error) => {
                client.dispose().await;
                return Err(error);
            }
        };
        let id = Uuid::new_v4().to_string();
        let parent_session_id = request.parent_session_id.clone();
        let session = LiveSession {
            client: client.clone(),
            snapshot: SessionSnapshot {
                id: id.clone(),
                adapter: adapter.name.clone(),
                cwd: cwd.clone(),
                program: None,
                status: SessionStatus::Configuring,
                stop_reason: None,
                output: String::new(),
                output_truncated: false,
                capabilities: capability_map(&capabilities),
                parent_session_id: parent_session_id.clone(),
                child_session_ids: Vec::new(),
            },
            capabilities: capabilities.clone(),
            policy: policy.clone(),
            last_used: Instant::now(),
        };
        self.sessions.lock().await.insert(id.clone(), session);
        self.add_child_relation(parent_session_id.as_deref(), &id).await;
        self.spawn_event_loop(id.clone(), client.clone(), policy.max_output_bytes);
        let mut attach_overrides = Map::new();
        attach_overrides.insert("request".into(), Value::from("attach"));
        attach_overrides.insert("cwd".into(), serde_json::to_value(&cwd)?);
        if let Some(pid) = request.pid {
            attach_overrides.insert("pid".into(), Value::from(pid));
        }
        if let Some(host) = request.host.as_deref() {
            attach_overrides.insert("host".into(), Value::from(host));
        }
        if let Some(port) = request.port {
            attach_overrides.insert("port".into(), Value::from(port));
        }
        let attach_args = merge_defaults(&adapter.config.attach_defaults, Value::Object(attach_overrides));
        if let Err(error) = client
            .request("attach", attach_args, policy.request_timeout, None)
            .await
        {
            let _ = self.disconnect_internal(&id).await;
            return Err(error);
        }
        if capabilities.supports("supportsConfigurationDoneRequest")
            && let Err(error) = client
                .request("configurationDone", serde_json::json!({}), policy.request_timeout, None)
                .await
        {
            let _ = self.disconnect_internal(&id).await;
            return Err(error);
        }
        self.update_status_if_configuring(&id, SessionStatus::Running).await;
        self.get(&id).await.ok_or_else(|| DapError::Session("session disappeared after attach".into()))
    }

    pub async fn execute(&self, id: &str, action: Action, arguments: Value, cancellation: Option<CancellationToken>) -> Result<Value> {
        let (client, capabilities, policy) = {
            let mut sessions = self.sessions.lock().await;
            let session = sessions.get_mut(id).ok_or_else(|| DapError::Session(format!("unknown session '{id}'")))?;
            session.last_used = Instant::now();
            (session.client.clone(), session.capabilities.clone(), session.policy.clone())
        };
        policy.check(action)?;
        if let Some(capability) = action.required_capability()
            && !capabilities.supports(capability)
        {
            return Err(DapError::Unsupported(format!("adapter did not advertise {capability}")));
        }
        let command = match action {
            Action::Continue => "continue",
            Action::Pause => "pause",
            Action::StepOver => "next",
            Action::StepIn => "stepIn",
            Action::StepOut => "stepOut",
            Action::Threads => "threads",
            Action::StackTrace => "stackTrace",
            Action::Scopes => "scopes",
            Action::Variables => "variables",
            Action::Evaluate => "evaluate",
            Action::Output => return self.get(id).await.map(|snapshot| serde_json::to_value(snapshot).unwrap_or(Value::Null)).ok_or_else(|| DapError::Session("session disappeared".into())),
            Action::ReadMemory => "readMemory",
            Action::WriteMemory => "writeMemory",
            Action::Modules => "modules",
            Action::Stop => "terminate",
            Action::Disconnect => "disconnect",
            Action::SetBreakpoint => "setBreakpoints",
            Action::RemoveBreakpoint => "setBreakpoints",
            Action::Status | Action::Sessions | Action::Launch | Action::Attach => return Err(DapError::Session("action requires a manager-level operation".into())),
        };
        let response = client.request(command, arguments, policy.request_timeout, cancellation).await?;
        if matches!(action, Action::Continue | Action::StepOver | Action::StepIn | Action::StepOut) {
            self.update_status(id, SessionStatus::Running).await;
        }
        if matches!(action, Action::Stop | Action::Disconnect) {
            let _ = self.disconnect_internal(id).await;
        }
        Ok(response.body.unwrap_or(Value::Null))
    }

    pub async fn disconnect(&self, id: &str) -> Result<()> {
        let policy = self
            .sessions
            .lock()
            .await
            .get(id)
            .map(|session| session.policy.clone())
            .ok_or_else(|| DapError::Session(format!("unknown session '{id}'")))?;
        policy.check(Action::Disconnect)?;
        self.disconnect_internal(id).await
    }

    async fn disconnect_internal(&self, id: &str) -> Result<()> {
        let mut ids = vec![id.to_owned()];
        let mut index = 0;
        while index < ids.len() {
            let child_ids = self
                .sessions
                .lock()
                .await
                .get(&ids[index])
                .map(|session| session.snapshot.child_session_ids.clone())
                .ok_or_else(|| DapError::Session(format!("unknown session '{id}'")))?;
            ids.extend(child_ids);
            index += 1;
        }

        for current_id in ids.into_iter().rev() {
            let Some(session) = self.sessions.lock().await.remove(&current_id) else {
                continue;
            };
            let parent_id = session.snapshot.parent_session_id.clone();
            session.client.dispose().await;
            if let Some(parent_id) = parent_id
                && let Some(parent) = self.sessions.lock().await.get_mut(&parent_id)
            {
                parent
                    .snapshot
                    .child_session_ids
                    .retain(|child_id| child_id != &current_id);
            }
        }
        Ok(())
    }

    async fn update_status(&self, id: &str, status: SessionStatus) {
        if let Some(session) = self.sessions.lock().await.get_mut(id) { session.snapshot.status = status; }
    }

    async fn add_child_relation(&self, parent_id: Option<&str>, child_id: &str) {
        let Some(parent_id) = parent_id else { return; };
        if let Some(parent) = self.sessions.lock().await.get_mut(parent_id)
            && !parent.snapshot.child_session_ids.iter().any(|id| id == child_id)
        {
            parent.snapshot.child_session_ids.push(child_id.to_owned());
        }
    }

    async fn update_status_if_configuring(&self, id: &str, status: SessionStatus) {
        if let Some(session) = self.sessions.lock().await.get_mut(id)
            && session.snapshot.status == SessionStatus::Configuring
        {
            session.snapshot.status = status;
        }
    }

    fn policy_for(&self, cwd: &Path) -> Result<DapPolicy> {
        if self.load_policy_from_config {
            DapPolicy::load(cwd)
        } else {
            Ok(self.policy.clone())
        }
    }

    fn spawn_event_loop(&self, id: SessionId, client: Arc<DapClient>, max_output: usize) {
        let sessions = self.sessions.clone();
        tokio::spawn(async move {
            let mut events = client.subscribe();
            loop {
                let event = match events.recv().await {
                    Ok(event) => event,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                };
                let mut sessions_guard = sessions.lock().await;
                let Some(session) = sessions_guard.get_mut(&id) else { break; };
                match event.event.as_str() {
                    "stopped" => {
                        session.snapshot.status = SessionStatus::Stopped;
                        session.snapshot.stop_reason = event.body.as_ref().and_then(|body| body.get("reason")).and_then(Value::as_str).map(str::to_owned);
                    }
                    "continued" => session.snapshot.status = SessionStatus::Running,
                    "terminated" | "exited" => session.snapshot.status = SessionStatus::Terminated,
                    "output" => {
                        if let Some(output) = event.body.as_ref().and_then(|body| body.get("output")).and_then(Value::as_str) {
                            append_output(&mut session.snapshot, output, max_output);
                        }
                    }
                    _ => {}
                }
            }
        });
    }
}

async fn spawn_adapter(
    adapter: &ResolvedAdapter,
    cwd: &Path,
    policy: &DapPolicy,
) -> Result<Arc<DapClient>> {
    let command = adapter.resolved_command.to_string_lossy();
    match adapter.config.transport {
        TransportMode::Stdio => DapClient::spawn_stdio(&command, &adapter.config.args, cwd).await,
        TransportMode::Tcp => {
            DapClient::spawn_tcp(&command, &adapter.config.args, cwd, policy.startup_timeout).await
        }
        TransportMode::Socket => {
            #[cfg(unix)]
            {
                DapClient::spawn_unix(
                    &command,
                    &adapter.config.args,
                    cwd,
                    policy.startup_timeout,
                )
                .await
            }
            #[cfg(not(unix))]
            {
                Err(DapError::Unsupported(
                    "Unix-socket DAP adapters are unavailable on this platform".into(),
                ))
            }
        }
    }
}

fn normalize_cwd(cwd: &Path) -> PathBuf { cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf()) }

fn capability_map(capabilities: &DapCapabilities) -> BTreeMap<String, bool> { capabilities.values.iter().filter_map(|(key, value)| value.as_bool().map(|value| (key.clone(), value))).collect() }

fn merge_defaults(defaults: &Value, overrides: Value) -> Value {
    let mut result = defaults.clone();
    merge_json(&mut result, overrides);
    result
}

fn merge_json(base: &mut Value, overlay: Value) {
    match base {
        Value::Object(base_object) => match overlay {
            Value::Object(overlay_object) => {
                for (key, value) in overlay_object {
                    merge_json(base_object.entry(key).or_insert(Value::Null), value);
                }
            }
            value => *base = value,
        },
        value => *value = overlay,
    }
}

pub(crate) fn append_output(snapshot: &mut SessionSnapshot, output: &str, max_bytes: usize) {
    if max_bytes == 0 {
        snapshot.output.clear();
        snapshot.output_truncated = true;
        return;
    }
    snapshot.output.push_str(output);
    while snapshot.output.len() > max_bytes {
        let excess = snapshot.output.len() - max_bytes;
        let mut split = excess.min(snapshot.output.len());
        while split < snapshot.output.len() && !snapshot.output.is_char_boundary(split) {
            split += 1;
        }
        snapshot.output.drain(..split);
        snapshot.output_truncated = true;
    }
}
