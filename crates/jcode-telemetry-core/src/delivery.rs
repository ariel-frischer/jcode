use super::DeliveryMode;
use jcode_logging as logging;
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Duration;

pub(super) const TELEMETRY_ENDPOINT: &str = "https://telemetry.jcode.sh/v1/event";
pub(super) const TRANSCRIPT_ENDPOINT: &str = "https://telemetry.jcode.sh/v1/transcript";
const ASYNC_SEND_TIMEOUT: Duration = Duration::from_secs(5);
const BACKGROUND_QUEUE_CAPACITY: usize = 2048;
static TELEMETRY_PERMANENTLY_REJECTED: AtomicBool = AtomicBool::new(false);
static TELEMETRY_QUEUE_OVERFLOW_WARNED: AtomicBool = AtomicBool::new(false);
static TELEMETRY_BACKGROUND_SENDER: RetryCell<SyncSender<Value>> = RetryCell::new();
#[cfg(not(test))]
static TRANSCRIPT_BACKGROUND_SENDER: RetryCell<SyncSender<Value>> = RetryCell::new();
static TELEMETRY_HTTP_CLIENT: RetryCell<reqwest::blocking::Client> = RetryCell::new();

pub(super) struct RetryCell<T> {
    value: OnceLock<T>,
    initializer: Mutex<()>,
}

impl<T> RetryCell<T> {
    pub(super) const fn new() -> Self {
        Self {
            value: OnceLock::new(),
            initializer: Mutex::new(()),
        }
    }
}

pub(super) fn cached_optional<T, E>(
    slot: &RetryCell<T>,
    initialize: impl FnOnce() -> Result<T, E>,
    report: impl FnOnce(E),
) -> Option<&T> {
    if let Some(value) = slot.value.get() {
        return Some(value);
    }
    let _guard = slot
        .initializer
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if let Some(value) = slot.value.get() {
        return Some(value);
    }
    match initialize() {
        Ok(value) => {
            let _already_initialized = slot.value.set(value);
            slot.value.get()
        }
        Err(err) => {
            report(err);
            None
        }
    }
}

fn telemetry_http_client() -> Option<&'static reqwest::blocking::Client> {
    cached_optional(
        &TELEMETRY_HTTP_CLIENT,
        || {
            reqwest::blocking::Client::builder()
                .user_agent(jcode_provider_core::JCODE_USER_AGENT)
                .build()
        },
        |err| {
            logging::warn(&format!(
                "telemetry HTTP client initialization failed: {err}"
            ))
        },
    )
}

fn post_payload(payload: Value, timeout: Duration) -> bool {
    if TELEMETRY_PERMANENTLY_REJECTED.load(Ordering::Relaxed) {
        return false;
    }
    let Some(client) = telemetry_http_client() else {
        return false;
    };
    match client
        .post(TELEMETRY_ENDPOINT)
        .timeout(timeout)
        .json(&payload)
        .send()
    {
        Ok(response) if response.status().is_success() => true,
        Ok(response) => {
            let status = response.status();
            if telemetry_status_is_permanent(status.as_u16()) {
                TELEMETRY_PERMANENTLY_REJECTED.store(true, Ordering::Relaxed);
                logging::warn(&format!(
                    "telemetry endpoint permanently rejected payload with HTTP {status}; suppressing telemetry delivery for this process"
                ));
            } else {
                logging::warn(&format!(
                    "telemetry endpoint temporarily rejected payload with HTTP {status}"
                ));
            }
            false
        }
        Err(err) => {
            logging::warn(&format!("telemetry payload send failed: {err}"));
            false
        }
    }
}

#[cfg(not(test))]
fn post_transcript_payload(payload: Value, timeout: Duration) -> bool {
    let Some(client) = telemetry_http_client() else {
        return false;
    };
    match client
        .post(TRANSCRIPT_ENDPOINT)
        .timeout(timeout)
        .json(&payload)
        .send()
    {
        Ok(response) if response.status().is_success() => true,
        Ok(response) => {
            logging::warn(&format!(
                "transcript endpoint rejected upload with HTTP {}",
                response.status()
            ));
            false
        }
        Err(err) => {
            logging::warn(&format!("transcript upload failed: {err}"));
            false
        }
    }
}

pub(super) fn telemetry_status_is_permanent(status: u16) -> bool {
    (400..500).contains(&status) && !matches!(status, 408 | 425 | 429)
}

pub(super) fn spawn_background_worker<F>(
    capacity: usize,
    mut deliver: F,
) -> std::io::Result<SyncSender<Value>>
where
    F: FnMut(Value) + Send + 'static,
{
    let (sender, receiver) = sync_channel(capacity);
    std::thread::Builder::new()
        .name("jcode-telemetry".to_string())
        .spawn(move || {
            while let Ok(payload) = receiver.recv() {
                deliver(payload);
            }
        })?;
    Ok(sender)
}

fn background_sender() -> Option<&'static SyncSender<Value>> {
    cached_optional(
        &TELEMETRY_BACKGROUND_SENDER,
        || {
            spawn_background_worker(BACKGROUND_QUEUE_CAPACITY, |payload| {
                let _delivered = post_payload(payload, ASYNC_SEND_TIMEOUT);
            })
        },
        |err| {
            logging::warn(&format!(
                "telemetry background worker failed to start: {err}"
            ))
        },
    )
}

#[cfg(not(test))]
fn transcript_background_sender() -> Option<&'static SyncSender<Value>> {
    cached_optional(
        &TRANSCRIPT_BACKGROUND_SENDER,
        || {
            spawn_background_worker(64, |payload| {
                let _delivered = post_transcript_payload(payload, ASYNC_SEND_TIMEOUT);
            })
        },
        |err| {
            logging::warn(&format!(
                "transcript telemetry background worker failed to start: {err}"
            ))
        },
    )
}

pub(super) fn send_transcript_payload(payload: Value) -> bool {
    #[cfg(test)]
    {
        if let Ok(mut emitted) = super::TEST_EMITTED_PAYLOADS.lock() {
            emitted.push(payload);
        }
        true
    }
    #[cfg(not(test))]
    {
        let Some(sender) = transcript_background_sender() else {
            return false;
        };
        match sender.try_send(payload) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                logging::warn("transcript upload queue is full; dropping transcript");
                false
            }
            Err(TrySendError::Disconnected(_)) => {
                logging::warn("transcript upload worker stopped; dropping transcript");
                false
            }
        }
    }
}

#[cfg(test)]
fn record_test_payload(payload: &Value) -> bool {
    if let Ok(mut emitted) = super::TEST_EMITTED_PAYLOADS.lock() {
        emitted.push(payload.clone());
    }
    true
}

#[cfg(not(test))]
fn record_test_payload(_payload: &Value) -> bool {
    false
}

pub(super) fn send_payload(mut payload: Value, mode: DeliveryMode) -> bool {
    super::concurrency::mark_legacy_concurrency_unavailable(&mut payload);
    if record_test_payload(&payload) {
        return true;
    }
    match mode {
        DeliveryMode::Background => {
            if TELEMETRY_PERMANENTLY_REJECTED.load(Ordering::Relaxed) {
                return false;
            }
            logging::debug("queueing telemetry payload for background delivery");
            let Some(sender) = background_sender() else {
                return false;
            };
            match sender.try_send(payload) {
                Ok(()) => {
                    TELEMETRY_QUEUE_OVERFLOW_WARNED.store(false, Ordering::Relaxed);
                    true
                }
                Err(TrySendError::Full(_)) => {
                    if !TELEMETRY_QUEUE_OVERFLOW_WARNED.swap(true, Ordering::Relaxed) {
                        logging::warn(&format!(
                            "telemetry background queue is full (capacity={BACKGROUND_QUEUE_CAPACITY}); dropping events until delivery catches up"
                        ));
                    }
                    false
                }
                Err(TrySendError::Disconnected(_)) => {
                    logging::warn("telemetry background worker stopped; dropping payload");
                    false
                }
            }
        }
        DeliveryMode::Blocking(timeout) => {
            logging::debug(&format!(
                "sending telemetry payload with blocking timeout={}ms",
                timeout.as_millis()
            ));
            if tokio::runtime::Handle::try_current().is_ok() {
                let (tx, rx) = sync_channel(1);
                match std::thread::Builder::new()
                    .name("jcode-telemetry-blocking".to_string())
                    .spawn(move || {
                        let _delivered = tx.send(post_payload(payload, timeout));
                    }) {
                    Ok(_worker) => rx.recv_timeout(timeout).unwrap_or(false),
                    Err(err) => {
                        logging::warn(&format!("telemetry blocking worker failed to start: {err}"));
                        false
                    }
                }
            } else {
                post_payload(payload, timeout)
            }
        }
    }
}
