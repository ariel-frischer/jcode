//! Nonblocking local accounting. One fixed worker owns all filesystem/config/log work.
//! No per-session map, provider call, pricing lookup, network client or raw error log.
use jcode_session_types::{
    lifecycle::LifecycleObservabilityStatus, memory_usage::MemoryRequestObservation,
};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    time::Duration,
};

pub use jcode_session_types::memory_usage as types;

mod pricing;
pub mod summary;

pub const QUEUE_CAPACITY: usize = 256;
pub const MAX_FLUSH_WAIT: Duration = Duration::from_millis(250);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitOutcome {
    Enqueued,
    Dropped,
    Invalid,
}
#[derive(Debug, Default)]
struct Counters {
    accepted: AtomicU64,
    dropped: AtomicU64,
    invalid: AtomicU64,
    persisted: AtomicU64,
    logged: AtomicU64,
    suppressed: AtomicU64,
    persistence_failures: AtomicU64,
    logging_failures: AtomicU64,
    flush_failures: AtomicU64,
    worker_running: AtomicBool,
}
/// Process-local counters only. Historical loss remains unknown after restart.
#[derive(Debug, Clone, Default, Serialize)]
pub struct RecorderSnapshot {
    pub accepted: u64,
    pub dropped: u64,
    pub invalid: u64,
    pub persisted: u64,
    pub logged: u64,
    pub suppressed: u64,
    pub persistence_failures: u64,
    pub logging_failures: u64,
    pub flush_failures: u64,
    pub worker_running: bool,
}
#[derive(Debug)]
enum Command {
    Record(Box<MemoryRequestObservation>),
    Flush(SyncSender<()>),
    Shutdown(SyncSender<()>),
}
#[derive(Debug, Clone)]
pub struct Recorder {
    tx: SyncSender<Command>,
    counters: Arc<Counters>,
}
impl Recorder {
    fn channel() -> (Self, Receiver<Command>) {
        let (tx, rx) = mpsc::sync_channel(QUEUE_CAPACITY);
        (
            Self {
                tx,
                counters: Arc::default(),
            },
            rx,
        )
    }
    /// Explicit private-root recorder for callers and deterministic offline tests.
    pub fn new(base: PathBuf, controls: LifecycleObservabilityStatus) -> Self {
        Self::start(base, Some(controls))
    }
    fn start(base: PathBuf, controls: Option<LifecycleObservabilityStatus>) -> Self {
        let (recorder, receiver) = Self::channel();
        let counters = Arc::clone(&recorder.counters);
        counters.worker_running.store(true, Ordering::Relaxed);
        let result = std::thread::Builder::new()
            .name("memory-usage".into())
            .stack_size(2 * 1024 * 1024)
            .spawn(move || run_worker(receiver, base, controls, counters));
        if result.is_err() {
            recorder
                .counters
                .worker_running
                .store(false, Ordering::Relaxed);
        }
        // Dropping the JoinHandle detaches. Never join a potentially stalled disk
        // worker on an inference thread. Barrier waits have a hard 250ms ceiling.
        recorder
    }
    pub fn submit(&self, record: MemoryRequestObservation) -> SubmitOutcome {
        if record.validate().is_err() {
            increment(&self.counters.invalid);
            return SubmitOutcome::Invalid;
        }
        match self.tx.try_send(Command::Record(Box::new(record))) {
            Ok(()) => {
                increment(&self.counters.accepted);
                SubmitOutcome::Enqueued
            }
            Err(_) => {
                increment(&self.counters.dropped);
                SubmitOutcome::Dropped
            }
        }
    }
    pub fn snapshot(&self) -> RecorderSnapshot {
        let c = &self.counters;
        RecorderSnapshot {
            accepted: c.accepted.load(Ordering::Relaxed),
            dropped: c.dropped.load(Ordering::Relaxed),
            invalid: c.invalid.load(Ordering::Relaxed),
            persisted: c.persisted.load(Ordering::Relaxed),
            logged: c.logged.load(Ordering::Relaxed),
            suppressed: c.suppressed.load(Ordering::Relaxed),
            persistence_failures: c.persistence_failures.load(Ordering::Relaxed),
            logging_failures: c.logging_failures.load(Ordering::Relaxed),
            flush_failures: c.flush_failures.load(Ordering::Relaxed),
            worker_running: c.worker_running.load(Ordering::Relaxed),
        }
    }
    /// Shutdown/reporting boundary only, never called from submission. Acknowledges
    /// prior queued writes, not fsync or lifetime completeness. False on any loss.
    pub fn flush(&self, timeout: Duration) -> bool {
        self.barrier(timeout, false)
    }
    pub fn shutdown(&self, timeout: Duration) -> bool {
        self.barrier(timeout, true)
    }
    fn barrier(&self, timeout: Duration, shutdown: bool) -> bool {
        let (tx, rx) = mpsc::sync_channel(1);
        let command = if shutdown {
            Command::Shutdown(tx)
        } else {
            Command::Flush(tx)
        };
        if self.tx.try_send(command).is_err()
            || rx.recv_timeout(timeout.min(MAX_FLUSH_WAIT)).is_err()
        {
            increment(&self.counters.flush_failures);
            return false;
        }
        let snapshot = self.snapshot();
        snapshot.persistence_failures == 0
            && snapshot.logging_failures == 0
            && snapshot.dropped == 0
            && snapshot.invalid == 0
    }
}

fn run_worker(
    receiver: Receiver<Command>,
    base: PathBuf,
    controls: Option<LifecycleObservabilityStatus>,
    counters: Arc<Counters>,
) {
    struct WorkerLife(Arc<Counters>);
    impl Drop for WorkerLife {
        fn drop(&mut self) {
            self.0.worker_running.store(false, Ordering::Relaxed);
        }
    }
    let _life = WorkerLife(Arc::clone(&counters));
    while let Ok(command) = receiver.recv() {
        match command {
            Command::Record(record) => {
                // Config reload stays on this worker, never in the request-local
                // finalizer. Current effective controls govern both output sinks.
                let status = controls.unwrap_or_else(|| {
                    crate::config::config()
                        .lifecycle_observability
                        .effective_status()
                });
                process_record(&base, status, &record, &counters, |serialized| {
                    crate::logging::event_info("memory_usage", [("accounting", serialized)]);
                });
            }
            Command::Flush(sender) => {
                if sender.try_send(()).is_err() {
                    increment(&counters.flush_failures);
                }
            }
            Command::Shutdown(sender) => {
                // Disconnect before acknowledging, so sends after shutdown cannot
                // be accepted into an orphaned receiver.
                drop(receiver);
                counters.worker_running.store(false, Ordering::Relaxed);
                if sender.try_send(()).is_err() {
                    increment(&counters.flush_failures);
                }
                return;
            }
        }
    }
}

fn process_record(
    base: &Path,
    status: LifecycleObservabilityStatus,
    record: &MemoryRequestObservation,
    counters: &Counters,
    emit: impl FnOnce(&str),
) {
    if record.validate().is_err() {
        increment(&counters.invalid);
        return;
    }
    if !status.enabled || (!status.persist_session_events && !status.emit_structured_logs) {
        increment(&counters.suppressed);
        return;
    }
    let persistence = if status.persist_session_events {
        if crate::session::memory_usage::append_in_dir(base, record).is_ok() {
            increment(&counters.persisted);
            "written"
        } else {
            increment(&counters.persistence_failures);
            "failed"
        }
    } else {
        "disabled"
    };
    if status.emit_structured_logs {
        // Only allowlisted validated metadata and a closed failure category. The
        // existing logger retains its redaction. Errors cannot bypass this switch.
        match serde_json::to_string(
            &serde_json::json!({"observation": record, "persistence": persistence}),
        ) {
            Ok(serialized) => {
                emit(&serialized);
                increment(&counters.logged);
            }
            Err(_) => increment(&counters.logging_failures),
        }
    }
}

pub(crate) fn increment(counter: &AtomicU64) {
    let mut value = counter.load(Ordering::Relaxed);
    while value != u64::MAX {
        match counter.compare_exchange_weak(value, value + 1, Ordering::Relaxed, Ordering::Relaxed)
        {
            Ok(_) => return,
            Err(current) => value = current,
        }
    }
}

static GLOBAL: OnceLock<Option<Recorder>> = OnceLock::new();
/// Initialization is outside attempt finalization. Tests explicitly attach a
/// private recorder, never accidentally write to the developer's real data root.
pub(crate) fn default_recorder() -> Option<Recorder> {
    #[cfg(test)]
    {
        None
    }
    #[cfg(not(test))]
    {
        GLOBAL
            .get_or_init(|| match crate::storage::jcode_dir() {
                Ok(base) => Some(Recorder::start(base, None)),
                Err(_) => None, // Exposed as unavailable, plus lost-observation counts.
            })
            .clone()
    }
}
/// Does not initialize a worker or touch disk. None is unavailable, not zero usage.
pub fn global_snapshot() -> Option<RecorderSnapshot> {
    GLOBAL
        .get()
        .and_then(|recorder| recorder.as_ref().map(Recorder::snapshot))
}
/// Bounded opt-in shutdown boundary for the owning process, not a daemon mutation.
pub fn flush_global(timeout: Duration) -> bool {
    GLOBAL
        .get()
        .and_then(Option::as_ref)
        .is_some_and(|recorder| recorder.flush(timeout))
}

#[cfg(test)]
#[path = "memory_usage_tests.rs"]
mod tests;
