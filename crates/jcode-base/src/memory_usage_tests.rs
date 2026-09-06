use super::*;
use crate::session::memory_usage::{read_in_dir, tests::record};
use std::{fs, time::Instant};

fn controls(enabled: bool, persist: bool, logs: bool) -> LifecycleObservabilityStatus {
    crate::config::LifecycleObservabilityConfig {
        enabled,
        persist_session_events: persist,
        emit_structured_logs: logs,
    }
    .effective_status()
}

#[test]
fn every_control_combination_gates_records_and_error_logs_independently() {
    for enabled in [false, true] {
        for persist in [false, true] {
            for logs in [false, true] {
                let dir = tempfile::tempdir().unwrap();
                let status = controls(enabled, persist, logs);
                let counters = Counters::default();
                let mut emitted = Vec::new();
                process_record(dir.path(), status, &record("r"), &counters, |text| {
                    emitted.push(text.to_owned())
                });
                assert_eq!(
                    read_in_dir(dir.path(), None).unwrap().calls.len(),
                    usize::from(enabled && persist)
                );
                assert_eq!(emitted.len(), usize::from(enabled && logs));
                assert_eq!(
                    counters.persisted.load(Ordering::Relaxed),
                    u64::from(enabled && persist)
                );
                assert_eq!(
                    counters.logged.load(Ordering::Relaxed),
                    u64::from(enabled && logs)
                );
                assert!(!format!("{emitted:?}").contains("SENSITIVE_SENTINEL"));
                // A raw filesystem error must never leak or bypass the structured sink switch.
                let broken = dir.path().join("blocked");
                fs::write(&broken, b"SENSITIVE_SENTINEL").unwrap();
                emitted.clear();
                process_record(&broken, status, &record("r2"), &counters, |text| {
                    emitted.push(text.to_owned())
                });
                assert_eq!(
                    counters.persistence_failures.load(Ordering::Relaxed),
                    u64::from(enabled && persist)
                );
                assert_eq!(emitted.len(), usize::from(enabled && logs));
                assert!(!format!("{emitted:?}").contains("SENSITIVE_SENTINEL"));
            }
        }
    }
}

#[test]
fn fixed_queue_saturation_dead_worker_and_counter_overflow_are_safe() {
    let (recorder, receiver) = Recorder::channel();
    for n in 0..QUEUE_CAPACITY {
        assert_eq!(
            recorder.submit(record(&format!("r{n}"))),
            SubmitOutcome::Enqueued
        );
    }
    let start = Instant::now();
    for _ in 0..1000 {
        assert_eq!(recorder.submit(record("overflow")), SubmitOutcome::Dropped);
    }
    assert!(start.elapsed() < Duration::from_secs(1));
    assert_eq!(recorder.snapshot().dropped, 1000);
    assert_eq!(recorder.snapshot().accepted, QUEUE_CAPACITY as u64);
    assert!(
        !recorder.flush(Duration::from_secs(60)),
        "full queue does not block to enqueue flush"
    );
    drop(receiver);
    assert_eq!(recorder.submit(record("dead")), SubmitOutcome::Dropped);
    recorder.counters.dropped.store(u64::MAX, Ordering::Relaxed);
    assert_eq!(recorder.submit(record("dead")), SubmitOutcome::Dropped);
    assert_eq!(recorder.snapshot().dropped, u64::MAX);
}

#[test]
fn invalid_record_is_not_queued_logged_or_persisted() {
    let (recorder, receiver) = Recorder::channel();
    let mut invalid = record("r");
    invalid.model = "SENSITIVE_SENTINEL/path with spaces".into();
    assert_eq!(recorder.submit(invalid.clone()), SubmitOutcome::Invalid);
    assert!(receiver.try_recv().is_err());
    assert_eq!(recorder.snapshot().invalid, 1);
    let dir = tempfile::tempdir().unwrap();
    let counters = Counters::default();
    process_record(
        dir.path(),
        controls(true, true, true),
        &invalid,
        &counters,
        |_| panic!("invalid record emitted"),
    );
    assert!(!dir.path().join("memory-usage").exists());
}

#[test]
fn real_worker_flush_shutdown_and_storage_failure_preserve_records() {
    let dir = tempfile::tempdir().unwrap();
    let recorder = Recorder::new(dir.path().to_path_buf(), controls(true, true, false));
    assert_eq!(recorder.submit(record("success")), SubmitOutcome::Enqueued);
    assert!(recorder.flush(MAX_FLUSH_WAIT));
    assert_eq!(recorder.snapshot().persisted, 1);
    assert_eq!(read_in_dir(dir.path(), None).unwrap().calls.len(), 1);
    assert!(recorder.shutdown(MAX_FLUSH_WAIT));
    assert_eq!(
        recorder.submit(record("after-shutdown")),
        SubmitOutcome::Dropped
    );
    assert!(!recorder.flush(MAX_FLUSH_WAIT));
    let broken = dir.path().join("blocked");
    fs::write(&broken, b"SENSITIVE_SENTINEL").unwrap();
    let recorder = Recorder::new(broken, controls(true, true, false));
    assert_eq!(recorder.submit(record("failure")), SubmitOutcome::Enqueued);
    assert!(
        !recorder.flush(MAX_FLUSH_WAIT),
        "drained does not mean durably persisted"
    );
    assert_eq!(recorder.snapshot().persistence_failures, 1);
    assert!(
        !serde_json::to_string(&recorder.snapshot())
            .unwrap()
            .contains("SENSITIVE_SENTINEL")
    );
    // Shutdown drains and exits but still reports the failed persistence honestly.
    assert!(!recorder.shutdown(MAX_FLUSH_WAIT));
}

#[test]
fn unresponsive_worker_flush_and_shutdown_have_fixed_deadlines() {
    let (recorder, _receiver) = Recorder::channel();
    let start = Instant::now();
    assert!(!recorder.flush(Duration::from_secs(60)));
    assert!(!recorder.shutdown(Duration::from_secs(60)));
    assert!(start.elapsed() < Duration::from_secs(1));
    assert_eq!(recorder.snapshot().flush_failures, 2);
}

#[test]
fn submission_reports_bounded_normal_and_saturated_overhead() {
    let sample = record("timing");
    let iterations = 20_000;
    let start = Instant::now();
    for _ in 0..iterations {
        std::hint::black_box(sample.clone());
    }
    let baseline = start.elapsed().as_nanos() / iterations;
    let (recorder, receiver) = Recorder::channel();
    let start = Instant::now();
    for _ in 0..iterations {
        assert_eq!(recorder.submit(sample.clone()), SubmitOutcome::Enqueued);
        assert!(receiver.try_recv().is_ok());
    }
    let normal = start.elapsed().as_nanos() / iterations;
    for _ in 0..QUEUE_CAPACITY {
        recorder.submit(sample.clone());
    }
    let start = Instant::now();
    for _ in 0..iterations {
        assert_eq!(recorder.submit(sample.clone()), SubmitOutcome::Dropped);
    }
    let saturated = start.elapsed().as_nanos() / iterations;
    // Reuse the existing pressure test's catastrophic ceiling (1000 submits
    // within one second), not a newly invented microsecond acceptance budget.
    assert!(normal < 1_000_000 && saturated < 1_000_000);
    assert_eq!(recorder.snapshot().dropped, iterations as u64);
    let queue_record_bound =
        QUEUE_CAPACITY * (std::mem::size_of::<MemoryRequestObservation>() + 5 * 128);
    assert!(queue_record_bound < 1024 * 1024);
    eprintln!(
        "accounting submission ns/op: clone baseline={baseline}, enqueue+drain={normal}, saturated={saturated}; queue={QUEUE_CAPACITY}, record memory upper estimate={queue_record_bound} bytes"
    );
}

#[test]
fn cold_recorder_start_and_first_submission_are_measured_separately() {
    let mut samples = Vec::new();
    for _ in 0..5 {
        let dir = tempfile::tempdir().unwrap();
        let start = Instant::now();
        let recorder = Recorder::new(dir.path().to_owned(), controls(true, true, false));
        let cold_ns = start.elapsed().as_nanos();
        let start = Instant::now();
        assert_eq!(recorder.submit(record("cold")), SubmitOutcome::Enqueued);
        let submit_ns = start.elapsed().as_nanos();
        assert!(recorder.flush(MAX_FLUSH_WAIT));
        assert_eq!(read_in_dir(dir.path(), None).unwrap().calls.len(), 1);
        assert!(recorder.shutdown(MAX_FLUSH_WAIT));
        samples.push((cold_ns, submit_ns));
    }
    eprintln!("cold recorder (initialization ns, first submission ns), 5 samples: {samples:?}");
}
