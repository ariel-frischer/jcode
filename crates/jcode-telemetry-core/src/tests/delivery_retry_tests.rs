use super::*;
use crate::delivery::{RetryCell, cached_optional, spawn_background_worker};
use serde_json::Value;
use std::sync::{
    atomic::Ordering,
    mpsc::{SyncSender, sync_channel},
};

#[test]
fn test_delivery_records_payload_without_entering_runtime_delivery() {
    let before = TEST_EMITTED_PAYLOADS
        .lock()
        .expect("telemetry test payloads")
        .len();

    assert!(crate::delivery::send_payload(
        serde_json::json!({"event": "test-only"}),
        crate::DeliveryMode::Blocking(Duration::from_millis(1)),
    ));

    let emitted = TEST_EMITTED_PAYLOADS
        .lock()
        .expect("telemetry test payloads");
    assert_eq!(emitted.len(), before + 1);
    assert_eq!(
        emitted.last(),
        Some(&serde_json::json!({"event": "test-only"}))
    );
}

#[test]
fn background_delivery_queue_is_bounded() {
    let (started_tx, started_rx) = std::sync::mpsc::sync_channel(1);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
    let sender = spawn_background_worker(1, move |_| {
        let _ = started_tx.send(());
        let _ = release_rx.recv();
    })
    .expect("start test telemetry worker");

    sender
        .send(serde_json::json!({"event": "first"}))
        .expect("enqueue first payload");
    started_rx.recv().expect("worker started first payload");
    sender
        .try_send(serde_json::json!({"event": "second"}))
        .expect("bounded queue accepts one waiting payload");
    assert!(matches!(
        sender.try_send(serde_json::json!({"event": "third"})),
        Err(std::sync::mpsc::TrySendError::Full(_))
    ));

    release_tx.send(()).expect("release telemetry worker");
}

#[test]
fn successful_initialization_is_cached() {
    let slot = RetryCell::new();
    let attempts = std::sync::atomic::AtomicUsize::new(0);

    assert_eq!(
        cached_optional(
            &slot,
            || {
                attempts.fetch_add(1, Ordering::Relaxed);
                Ok::<_, std::io::Error>(7)
            },
            |_| unreachable!("successful initialization must not report an error"),
        ),
        Some(&7)
    );
    assert_eq!(
        cached_optional(&slot, || Ok::<_, std::io::Error>(9), |_| {}),
        Some(&7)
    );
    assert_eq!(attempts.load(Ordering::Relaxed), 1);
}

#[test]
fn successful_initialization_is_cached_across_concurrent_callers() {
    let slot = std::sync::Arc::new(RetryCell::new());
    let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    for value in call_initializer_concurrently(std::sync::Arc::clone(&slot), &attempts) {
        assert_eq!(value, Some(7));
    }
    assert_eq!(attempts.load(Ordering::Relaxed), 1);
}

fn call_initializer_concurrently(
    slot: std::sync::Arc<RetryCell<u8>>,
    attempts: &std::sync::Arc<std::sync::atomic::AtomicUsize>,
) -> Vec<Option<u8>> {
    const CALLERS: usize = 8;
    let start = std::sync::Arc::new(std::sync::Barrier::new(CALLERS));
    (0..CALLERS)
        .map(|_| {
            let slot = std::sync::Arc::clone(&slot);
            let attempts = std::sync::Arc::clone(attempts);
            let start = std::sync::Arc::clone(&start);
            std::thread::spawn(move || {
                start.wait();
                cached_optional(
                    &slot,
                    || {
                        attempts.fetch_add(1, Ordering::Relaxed);
                        std::thread::sleep(Duration::from_millis(20));
                        Ok::<_, std::io::Error>(7)
                    },
                    |_| unreachable!("successful initialization must not report an error"),
                )
                .copied()
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|thread| thread.join().expect("initializer caller must not panic"))
        .collect()
}

#[test]
fn transient_failure_retries_once_across_concurrent_callers() {
    let slot = std::sync::Arc::new(RetryCell::new());
    let reported = std::sync::atomic::AtomicUsize::new(0);
    assert!(
        cached_optional(
            &slot,
            || Err::<u8, _>("temporary"),
            |_| {
                reported.fetch_add(1, Ordering::Relaxed);
            }
        )
        .is_none()
    );
    assert_eq!(reported.load(Ordering::Relaxed), 1);

    let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    for value in call_initializer_concurrently(slot, &attempts) {
        assert_eq!(value, Some(7));
    }
    assert_eq!(attempts.load(Ordering::Relaxed), 1);
}

#[test]
fn transient_http_client_initialization_failure_can_recover() {
    let slot = RetryCell::new();
    let reported = std::sync::atomic::AtomicUsize::new(0);

    assert!(
        cached_optional(
            &slot,
            || Err::<u8, _>("temporary"),
            |_| {
                reported.fetch_add(1, Ordering::Relaxed);
            }
        )
        .is_none()
    );
    assert_eq!(reported.load(Ordering::Relaxed), 1);
    assert_eq!(
        cached_optional(&slot, || Ok::<_, &str>(7), |_| {}),
        Some(&7)
    );
}

#[test]
fn transient_background_worker_failure_is_observable_and_can_recover() {
    let slot = RetryCell::new();
    let reported = std::sync::atomic::AtomicUsize::new(0);

    let first_attempt = std::panic::catch_unwind(|| {
        cached_optional(
            &slot,
            || Err::<SyncSender<Value>, _>(std::io::Error::other("temporary spawn failure")),
            |_| {
                reported.fetch_add(1, Ordering::Relaxed);
            },
        )
    });
    assert!(
        first_attempt
            .expect("spawn failure must not panic")
            .is_none()
    );
    assert_eq!(reported.load(Ordering::Relaxed), 1);

    let (sender, _receiver) = sync_channel(1);
    assert!(cached_optional(&slot, || Ok::<_, std::io::Error>(sender), |_| {}).is_some());
}
