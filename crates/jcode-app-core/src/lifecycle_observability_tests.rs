//! Shared fixtures for lifecycle observability tests.
//!
//! These helpers deliberately use an isolated temporary directory, synthetic
//! identifiers, and a deterministic clock. They do not start the shared
//! daemon or access remote telemetry.

#![allow(dead_code)]

use chrono::{DateTime, Duration, Utc};
use std::path::PathBuf;
use tempfile::TempDir;

pub(crate) const TEST_SESSION_ID: &str = "synthetic-session-001";
const BASE_TIMESTAMP_SECONDS: i64 = 1_700_000_000;

/// A self-contained fixture for one synthetic lifecycle session.
pub(crate) struct LifecycleTestHarness {
    pub(crate) temp_root: TempDir,
    pub(crate) session_dir: PathBuf,
    pub(crate) base_time: DateTime<Utc>,
}

impl LifecycleTestHarness {
    pub(crate) fn new() -> Self {
        let temp_root = tempfile::tempdir().expect("create lifecycle test root");
        let session_dir = temp_root.path().join(TEST_SESSION_ID);
        std::fs::create_dir(&session_dir).expect("create synthetic lifecycle session dir");

        Self {
            temp_root,
            session_dir,
            base_time: DateTime::from_timestamp(BASE_TIMESTAMP_SECONDS, 0)
                .expect("valid deterministic lifecycle timestamp"),
        }
    }

    pub(crate) fn timestamp(&self, offset_seconds: i64) -> DateTime<Utc> {
        self.base_time + Duration::seconds(offset_seconds)
    }

    /// Yield once so async recorder tests can let queued work make progress.
    pub(crate) async fn yield_to_recorder() {
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn lifecycle_harness_is_isolated_and_deterministic() {
    let harness = LifecycleTestHarness::new();

    assert!(harness.session_dir.is_dir());
    assert_eq!(
        harness
            .session_dir
            .file_name()
            .and_then(|name| name.to_str()),
        Some(TEST_SESSION_ID)
    );
    assert_eq!(harness.timestamp(0), harness.base_time);
    assert_eq!(harness.timestamp(5).timestamp(), BASE_TIMESTAMP_SECONDS + 5);

    LifecycleTestHarness::yield_to_recorder().await;
}
