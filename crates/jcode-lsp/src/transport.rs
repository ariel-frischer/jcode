use crate::error::{LspError, Result};
use crate::protocol::{JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, RequestId};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{Mutex, Notify, broadcast, oneshot};
use tokio::time::timeout;

const DEFAULT_MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;
const DEFAULT_MAX_HEADER_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub method: String,
    pub params: Value,
}

#[derive(Debug, Default)]
pub struct FrameCodec {
    buffer: Vec<u8>,
    max_frame_bytes: usize,
}

impl FrameCodec {
    pub fn new(max_frame_bytes: usize) -> Self {
        Self {
            buffer: Vec::new(),
            max_frame_bytes: max_frame_bytes.max(1),
        }
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Vec<u8>>> {
        self.buffer.extend_from_slice(bytes);
        let mut messages = Vec::new();
        loop {
            let Some((header_end, delimiter_len)) = find_header_end(&self.buffer) else {
                if self.buffer.len() > DEFAULT_MAX_HEADER_BYTES {
                    return Err(LspError::Protocol(
                        "LSP header exceeded maximum size".into(),
                    ));
                }
                break;
            };
            let header = String::from_utf8_lossy(&self.buffer[..header_end]);
            let content_length = header
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    if name.trim().eq_ignore_ascii_case("content-length") {
                        parse_usize(value.trim())
                    } else {
                        None
                    }
                })
                .ok_or_else(|| LspError::Protocol("LSP frame is missing Content-Length".into()))?;
            if content_length > self.max_frame_bytes {
                return Err(LspError::Protocol(format!(
                    "LSP frame exceeds maximum of {} bytes",
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
                continue;
            }
            messages.push(body);
        }
        Ok(messages)
    }
}

#[allow(clippy::manual_ok_err)]
fn parse_usize(value: &str) -> Option<usize> {
    match value.parse::<usize>() {
        Ok(value) => Some(value),
        Err(_) => None,
    }
}

fn find_header_end(buffer: &[u8]) -> Option<(usize, usize)> {
    let crlf = buffer.windows(4).position(|window| window == b"\r\n\r\n");
    let lf = buffer.windows(2).position(|window| window == b"\n\n");
    match (crlf, lf) {
        (Some(crlf), Some(lf)) if crlf <= lf => Some((crlf, 4)),
        (Some(_), Some(lf)) => Some((lf, 2)),
        (Some(crlf), None) => Some((crlf, 4)),
        (None, Some(lf)) => Some((lf, 2)),
        (None, None) => None,
    }
}

type Writer = Box<dyn AsyncWrite + Send + Unpin>;
type Pending = HashMap<RequestId, oneshot::Sender<Result<Value>>>;

#[derive(Clone)]
pub struct LspClient {
    writer: Arc<Mutex<Writer>>,
    pending: Arc<Mutex<Pending>>,
    notifications: broadcast::Sender<Notification>,
    next_id: Arc<AtomicU64>,
    disposed: Arc<AtomicBool>,
    disposed_notify: Arc<Notify>,
}

impl LspClient {
    pub async fn from_stream<R, W>(reader: R, writer: W) -> Arc<Self>
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        let (notifications, _) = broadcast::channel(128);
        let client = Arc::new(Self {
            writer: Arc::new(Mutex::new(Box::new(writer))),
            pending: Arc::new(Mutex::new(HashMap::new())),
            notifications,
            next_id: Arc::new(AtomicU64::new(1)),
            disposed: Arc::new(AtomicBool::new(false)),
            disposed_notify: Arc::new(Notify::new()),
        });
        client.spawn_reader(reader);
        client
    }

    fn spawn_reader<R>(self: &Arc<Self>, mut reader: R)
    where
        R: AsyncRead + Send + Unpin + 'static,
    {
        let pending = self.pending.clone();
        let notifications = self.notifications.clone();
        let disposed = self.disposed.clone();
        let disposed_notify = self.disposed_notify.clone();
        tokio::spawn(async move {
            let mut codec = FrameCodec::new(DEFAULT_MAX_FRAME_BYTES);
            let mut buffer = [0_u8; 16 * 1024];
            let reason = loop {
                match reader.read(&mut buffer).await {
                    Ok(0) => break "server stream closed".to_owned(),
                    Ok(size) => match codec.push(&buffer[..size]) {
                        Ok(frames) => {
                            for frame in frames {
                                if let Ok(value) = serde_json::from_slice::<Value>(&frame) {
                                    if let Some(id) = value.get("id").and_then(Value::as_u64) {
                                        let result = match serde_json::from_value::<JsonRpcResponse>(
                                            value,
                                        ) {
                                            Ok(response) => response.error.map_or_else(
                                                || Ok(response.result.unwrap_or(Value::Null)),
                                                |error| Err(LspError::Server(error.message)),
                                            ),
                                            Err(error) => Err(LspError::Json(error.to_string())),
                                        };
                                        if let Some(sender) = pending.lock().await.remove(&id) {
                                            std::mem::drop(sender.send(result));
                                        }
                                    } else if let Ok(notification) =
                                        serde_json::from_value::<JsonRpcNotification>(value)
                                    {
                                        std::mem::drop(notifications.send(Notification {
                                            method: notification.method,
                                            params: notification.params.unwrap_or(Value::Null),
                                        }));
                                    }
                                }
                            }
                        }
                        Err(error) => break error.to_string(),
                    },
                    Err(error) => break error.to_string(),
                }
            };
            disposed.store(true, Ordering::Release);
            disposed_notify.notify_waiters();
            let mut pending = pending.lock().await;
            for (_, sender) in pending.drain() {
                std::mem::drop(sender.send(Err(LspError::Server(reason.clone()))));
            }
        });
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Notification> {
        self.notifications.subscribe()
    }

    pub fn is_disposed(&self) -> bool {
        self.disposed.load(Ordering::Acquire)
    }

    pub async fn request(
        &self,
        method: &str,
        params: Value,
        request_timeout: Duration,
    ) -> Result<Value> {
        if self.is_disposed() {
            return Err(LspError::Server("language server is not running".into()));
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().await.insert(id, sender);
        let request = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(id),
            method: method.into(),
            params: Some(params),
        };
        if let Err(error) = self.write_json(&request, request_timeout).await {
            self.pending.lock().await.remove(&id);
            return Err(error);
        }
        match timeout(request_timeout, receiver).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(LspError::Server("request channel closed".into())),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                Err(LspError::Timeout(method.into()))
            }
        }
    }

    pub async fn notify(
        &self,
        method: &str,
        params: Value,
        request_timeout: Duration,
    ) -> Result<()> {
        let notification = JsonRpcNotification {
            jsonrpc: "2.0".into(),
            method: method.into(),
            params: Some(params),
        };
        self.write_json(&notification, request_timeout).await
    }

    async fn write_json<T: serde::Serialize>(
        &self,
        message: &T,
        write_timeout: Duration,
    ) -> Result<()> {
        let body = serde_json::to_vec(message)?;
        let frame = format!("Content-Length: {}\r\n\r\n", body.len());
        let mut writer = self.writer.lock().await;
        timeout(write_timeout, async {
            writer.write_all(frame.as_bytes()).await?;
            writer.write_all(&body).await?;
            writer.flush().await
        })
        .await
        .map_err(|_| LspError::Timeout("writing request".into()))??;
        Ok(())
    }

    pub async fn wait_disposed(&self) {
        if !self.is_disposed() {
            self.disposed_notify.notified().await;
        }
    }
}
