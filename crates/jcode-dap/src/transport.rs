use crate::error::{DapError, Result};
use crate::protocol::{DapCapabilities, DapEventMessage, DapResponseMessage};
use serde_json::Value;
use std::collections::HashMap;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, split};
use tokio::net::{TcpListener, TcpStream};
#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, broadcast, oneshot};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

const DEFAULT_MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Default)]
pub struct FrameCodec {
    buffer: Vec<u8>,
    max_frame_bytes: usize,
    malformed_messages: usize,
}

impl FrameCodec {
    pub fn new(max_frame_bytes: usize) -> Self {
        Self {
            buffer: Vec::new(),
            max_frame_bytes: max_frame_bytes.max(1),
            malformed_messages: 0,
        }
    }

    pub fn malformed_messages(&self) -> usize {
        self.malformed_messages
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Vec<u8>>> {
        if self.max_frame_bytes == 0 {
            self.max_frame_bytes = DEFAULT_MAX_FRAME_BYTES;
        }
        self.buffer.extend_from_slice(bytes);
        let mut messages = Vec::new();
        loop {
            let Some(header_end) = find_header_end(&self.buffer) else {
                if self.buffer.len() > 64 * 1024 {
                    return Err(DapError::Protocol(
                        "DAP header exceeded maximum size".into(),
                    ));
                }
                break;
            };
            let delimiter_len = header_delimiter_len(&self.buffer, header_end);
            let header = String::from_utf8_lossy(&self.buffer[..header_end]);
            let content_length = header.lines().find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.trim()
                    .eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            });
            let Some(content_length) = content_length else {
                return Err(DapError::Protocol(
                    "DAP frame is missing Content-Length".into(),
                ));
            };
            if content_length > self.max_frame_bytes {
                return Err(DapError::Protocol(format!(
                    "DAP frame exceeds maximum of {} bytes",
                    self.max_frame_bytes
                )));
            }
            let body_start = header_end + delimiter_len;
            if self.buffer.len() < body_start + content_length {
                break;
            }
            let body = self.buffer[body_start..body_start + content_length].to_vec();
            self.buffer.drain(..body_start + content_length);
            if serde_json::from_slice::<Value>(&body).is_err() {
                self.malformed_messages += 1;
                continue;
            }
            messages.push(body);
        }
        Ok(messages)
    }
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    let crlf = buffer.windows(4).position(|window| window == b"\r\n\r\n");
    let lf = buffer.windows(2).position(|window| window == b"\n\n");
    match (crlf, lf) {
        (Some(crlf), Some(lf)) => Some(crlf.min(lf)),
        (value, None) | (None, value) => value,
    }
}

fn header_delimiter_len(buffer: &[u8], header_end: usize) -> usize {
    if buffer.get(header_end..header_end + 4) == Some(b"\r\n\r\n") {
        4
    } else {
        2
    }
}

#[derive(Clone)]
pub struct DapClient {
    writer: Arc<Mutex<Box<dyn AsyncWrite + Send + Unpin>>>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Result<DapResponseMessage>>>>>,
    events: broadcast::Sender<DapEventMessage>,
    next_seq: Arc<AtomicU64>,
    disposed: Arc<AtomicBool>,
    kill_tx: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    capabilities: Arc<Mutex<DapCapabilities>>,
}

impl DapClient {
    pub async fn from_stream<R, W>(reader: R, writer: W) -> Arc<Self>
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        let (events, _) = broadcast::channel(64);
        let client = Arc::new(Self {
            writer: Arc::new(Mutex::new(Box::new(writer))),
            pending: Arc::new(Mutex::new(HashMap::new())),
            events,
            next_seq: Arc::new(AtomicU64::new(1)),
            disposed: Arc::new(AtomicBool::new(false)),
            kill_tx: Arc::new(Mutex::new(None)),
            capabilities: Arc::new(Mutex::new(DapCapabilities::default())),
        });
        client.spawn_reader(reader);
        client
    }

    pub async fn connect_tcp(
        host: &str,
        port: u16,
        connect_timeout: Duration,
    ) -> Result<Arc<Self>> {
        let stream = timeout(connect_timeout, TcpStream::connect((host, port)))
            .await
            .map_err(|_| {
                DapError::Timeout(format!("connecting to DAP adapter at {host}:{port}"))
            })??;
        let (reader, writer) = split(stream);
        Ok(Self::from_stream(reader, writer).await)
    }

    #[cfg(unix)]
    pub async fn connect_unix(
        path: &std::path::Path,
        connect_timeout: Duration,
    ) -> Result<Arc<Self>> {
        let stream = timeout(connect_timeout, UnixStream::connect(path))
            .await
            .map_err(|_| {
                DapError::Timeout(format!(
                    "connecting to DAP adapter socket {}",
                    path.display()
                ))
            })??;
        let (reader, writer) = split(stream);
        Ok(Self::from_stream(reader, writer).await)
    }

    pub async fn spawn_stdio(
        command: &str,
        args: &[String],
        cwd: &std::path::Path,
    ) -> Result<Arc<Self>> {
        let mut process = Command::new(command);
        process
            .args(args)
            .current_dir(cwd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        #[cfg(unix)]
        process.process_group(0);
        let mut child = process.spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| DapError::AdapterExited("adapter stdout was not piped".into()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| DapError::AdapterExited("adapter stdin was not piped".into()))?;
        let client = Self::from_stream(stdout, stdin).await;
        Self::track_child(&client, child).await;
        Ok(client)
    }

    pub async fn spawn_tcp(
        command: &str,
        args: &[String],
        cwd: &std::path::Path,
        startup_timeout: Duration,
    ) -> Result<Arc<Self>> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let port = listener.local_addr()?.port();
        let args = args
            .iter()
            .map(|arg| arg.replace("${port}", &port.to_string()))
            .collect::<Vec<_>>();
        let mut process = Command::new(command);
        process
            .args(args)
            .current_dir(cwd)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        #[cfg(unix)]
        process.process_group(0);
        let mut child = process.spawn()?;
        let accepted = timeout(startup_timeout, listener.accept()).await;
        let (stream, _) = match accepted {
            Ok(Ok(stream)) => stream,
            Ok(Err(error)) => {
                let _ = child.kill().await;
                return Err(error.into());
            }
            Err(_) => {
                let _ = child.kill().await;
                return Err(DapError::Timeout(format!(
                    "DAP TCP adapter did not connect on port {port}"
                )));
            }
        };
        let (reader, writer) = split(stream);
        let client = Self::from_stream(reader, writer).await;
        Self::track_child(&client, child).await;
        Ok(client)
    }

    #[cfg(unix)]
    pub async fn spawn_unix(
        command: &str,
        args: &[String],
        cwd: &std::path::Path,
        startup_timeout: Duration,
    ) -> Result<Arc<Self>> {
        let socket_dir = std::env::temp_dir().join(format!("jcode-dap-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&socket_dir)?;
        std::fs::set_permissions(&socket_dir, std::fs::Permissions::from_mode(0o700))?;
        let socket_path = socket_dir.join("adapter.sock");
        let listener = match UnixListener::bind(&socket_path) {
            Ok(listener) => listener,
            Err(error) => {
                let _ = std::fs::remove_dir(&socket_dir);
                return Err(error.into());
            }
        };
        let args = args
            .iter()
            .map(|arg| arg.replace("${socket}", &socket_path.to_string_lossy()))
            .collect::<Vec<_>>();
        let mut process = Command::new(command);
        process
            .args(args)
            .current_dir(cwd)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        process.process_group(0);
        let mut child = match process.spawn() {
            Ok(child) => child,
            Err(error) => {
                let _ = std::fs::remove_file(&socket_path);
                let _ = std::fs::remove_dir(&socket_dir);
                return Err(error.into());
            }
        };
        let accepted = timeout(startup_timeout, listener.accept()).await;
        let (stream, _) = match accepted {
            Ok(Ok(stream)) => stream,
            Ok(Err(error)) => {
                let _ = child.kill().await;
                let _ = std::fs::remove_file(&socket_path);
                let _ = std::fs::remove_dir(&socket_dir);
                return Err(error.into());
            }
            Err(_) => {
                let _ = child.kill().await;
                let _ = std::fs::remove_file(&socket_path);
                let _ = std::fs::remove_dir(&socket_dir);
                return Err(DapError::Timeout(format!(
                    "DAP Unix adapter did not connect to {}",
                    socket_path.display()
                )));
            }
        };
        let _ = std::fs::remove_file(&socket_path);
        let _ = std::fs::remove_dir(&socket_dir);
        let (reader, writer) = split(stream);
        let client = Self::from_stream(reader, writer).await;
        Self::track_child(&client, child).await;
        Ok(client)
    }

    async fn track_child(client: &Arc<Self>, mut child: Child) {
        let (kill_tx, mut kill_rx) = oneshot::channel();
        *client.kill_tx.lock().await = Some(kill_tx);
        let disposed = client.disposed.clone();
        let pending = client.pending.clone();
        let events = client.events.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = child.wait() => {}
                _ = &mut kill_rx => {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                }
            }
            disposed.store(true, Ordering::SeqCst);
            let _ = events.send(crate::protocol::DapEventMessage::new(
                0,
                "exited",
                serde_json::json!({"reason": "adapter_process_exited"}),
            ));
            let mut pending = pending.lock().await;
            for (_, sender) in pending.drain() {
                let _ = sender.send(Err(DapError::AdapterExited(
                    "adapter process exited".into(),
                )));
            }
        });
    }

    fn spawn_reader<R>(&self, mut reader: R)
    where
        R: AsyncRead + Send + Unpin + 'static,
    {
        let pending = self.pending.clone();
        let events = self.events.clone();
        let disposed = self.disposed.clone();
        tokio::spawn(async move {
            let mut codec = FrameCodec::default();
            let mut bytes = [0_u8; 16 * 1024];
            loop {
                let count = match reader.read(&mut bytes).await {
                    Ok(0) => break,
                    Ok(count) => count,
                    Err(error) => {
                        let mut pending = pending.lock().await;
                        let error =
                            DapError::Io(std::io::Error::new(error.kind(), error.to_string()));
                        for (_, sender) in pending.drain() {
                            let _ = sender.send(Err(DapError::Protocol(error.to_string())));
                        }
                        break;
                    }
                };
                let frames = match codec.push(&bytes[..count]) {
                    Ok(frames) => frames,
                    Err(error) => {
                        let mut pending = pending.lock().await;
                        for (_, sender) in pending.drain() {
                            let _ = sender.send(Err(DapError::Protocol(error.to_string())));
                        }
                        break;
                    }
                };
                for frame in frames {
                    let value: Value = match serde_json::from_slice(&frame) {
                        Ok(value) => value,
                        Err(_) => continue,
                    };
                    match value.get("type").and_then(Value::as_str) {
                        Some("response") => {
                            if let Ok(response) =
                                serde_json::from_value::<DapResponseMessage>(value)
                                && let Some(sender) =
                                    pending.lock().await.remove(&response.request_seq)
                            {
                                let _ = sender.send(Ok(response));
                            }
                        }
                        Some("event") => {
                            if let Ok(event) = serde_json::from_value::<DapEventMessage>(value) {
                                let _ = events.send(event);
                            }
                        }
                        _ => {}
                    }
                }
            }
            disposed.store(true, Ordering::SeqCst);
            let _ = events.send(crate::protocol::DapEventMessage::new(
                0,
                "exited",
                serde_json::json!({"reason": "dap_transport_ended"}),
            ));
            let mut pending = pending.lock().await;
            for (_, sender) in pending.drain() {
                let _ = sender.send(Err(DapError::AdapterExited("DAP transport ended".into())));
            }
        });
    }

    pub fn subscribe(&self) -> broadcast::Receiver<DapEventMessage> {
        self.events.subscribe()
    }

    pub async fn request(
        &self,
        command: &str,
        arguments: Value,
        request_timeout: Duration,
        cancellation: Option<CancellationToken>,
    ) -> Result<DapResponseMessage> {
        if self.disposed.load(Ordering::SeqCst) {
            return Err(DapError::AdapterExited("transport is closed".into()));
        }
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        let message = crate::protocol::DapRequestMessage::new(seq, command, arguments);
        let payload = serde_json::to_vec(&message)?;
        let mut framed = format!("Content-Length: {}\r\n\r\n", payload.len()).into_bytes();
        framed.extend_from_slice(&payload);
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().await.insert(seq, sender);
        let write_result = timeout(WRITE_TIMEOUT, async {
            let mut writer = self.writer.lock().await;
            writer.write_all(&framed).await?;
            writer.flush().await
        })
        .await;
        let write_result = match write_result {
            Ok(result) => result,
            Err(_) => {
                self.pending.lock().await.remove(&seq);
                return Err(DapError::Timeout(format!("writing DAP request {command}")));
            }
        };
        if let Err(error) = write_result {
            self.pending.lock().await.remove(&seq);
            return Err(DapError::Io(error));
        }
        let wait = async {
            match cancellation {
                Some(token) => tokio::select! {
                    _ = token.cancelled() => Err(DapError::Cancelled),
                    result = receiver => result.map_err(|_| DapError::AdapterExited("response channel closed".into()))?,
                },
                None => receiver
                    .await
                    .map_err(|_| DapError::AdapterExited("response channel closed".into()))?,
            }
        };
        match timeout(request_timeout, wait).await {
            Ok(result) => {
                if matches!(&result, Err(DapError::Cancelled)) {
                    self.pending.lock().await.remove(&seq);
                }
                match result {
                    Ok(response) if !response.success => Err(DapError::Protocol(
                        response
                            .message
                            .unwrap_or_else(|| format!("DAP request {command} failed")),
                    )),
                    other => other,
                }
            }
            Err(_) => {
                self.pending.lock().await.remove(&seq);
                Err(DapError::Timeout(format!("DAP request {command}")))
            }
        }
    }

    pub async fn initialize(&self, request_timeout: Duration) -> Result<DapCapabilities> {
        let response = self
            .request(
                "initialize",
                serde_json::json!({
                    "clientID": "jcode",
                    "clientName": "jcode",
                    "supportsRunInTerminalRequest": false
                }),
                request_timeout,
                None,
            )
            .await?;
        let capabilities: DapCapabilities = response
            .body
            .and_then(|body| serde_json::from_value(body).ok())
            .unwrap_or_default();
        *self.capabilities.lock().await = capabilities.clone();
        Ok(capabilities)
    }

    pub async fn capabilities(&self) -> DapCapabilities {
        self.capabilities.lock().await.clone()
    }

    pub async fn dispose(&self) {
        if self.disposed.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Some(kill) = self.kill_tx.lock().await.take() {
            let _ = kill.send(());
        }
        let mut pending = self.pending.lock().await;
        for (_, sender) in pending.drain() {
            let _ = sender.send(Err(DapError::Cancelled));
        }
    }
}
