use super::publication::{PublicationClaim, PublicationStore};
use super::{LibrarianArtifactPaths, LibrarianGeneration};
use jcode_session_types::{
    BoundedUsage, LibrarianBudgetIdentity, LibrarianConfigurationIdentity, RouteIdentity,
    SessionSummary, SourceFingerprint,
};
use serde_json::{Value, json};
use std::{
    fs::{self, File, FileTimes},
    io,
    path::Path,
    sync::{
        Arc, Barrier,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant, UNIX_EPOCH},
};

const SESSION_ID: &str = "publication-session";
const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const FORMAT_VERSION: &str = "session-summary.v1";
const GENERATED_AT: &str = "2026-08-14T03:30:00Z";

fn route() -> RouteIdentity {
    RouteIdentity {
        provider: "openai".into(),
        api_method: "openai-oauth".into(),
        model: "gpt-5.6-luna".into(),
        reasoning_effort: "xhigh".into(),
    }
}

fn fingerprint_with_digest(digest: &str) -> SourceFingerprint {
    SourceFingerprint {
        algorithm_version: "session-librarian-fingerprint.v1".into(),
        digest: digest.into(),
        configuration_identity: LibrarianConfigurationIdentity {
            budgets: LibrarianBudgetIdentity {
                deadline_seconds: 120,
                max_cost_micros_usd: 500_000,
                max_input_tokens: 12_000,
                max_output_tokens: 2_500,
                max_requests: 1,
            },
            filter_version: "session-librarian-filter.v1".into(),
            prompt_version: "session-librarian-prompt.v1".into(),
            receipt_version: "session-librarian-receipt.v1".into(),
            renderer_version: "session-librarian-markdown.v1".into(),
            route: route(),
            schema_version: FORMAT_VERSION.into(),
        },
    }
}

fn fingerprint() -> SourceFingerprint {
    fingerprint_with_digest(DIGEST)
}

fn usage() -> BoundedUsage {
    BoundedUsage {
        input_tokens: 640,
        output_tokens: 420,
        request_count: 1,
        elapsed_ms: 1_250,
        cost_micros_usd: 24_000,
    }
}

fn summary_value(fingerprint: &SourceFingerprint) -> Value {
    json!({
        "format_version": FORMAT_VERSION,
        "session_id": SESSION_ID,
        "source_fingerprint": fingerprint,
        "generated_at": GENERATED_AT,
        "effective_route": route(),
        "usage": usage(),
        "summary": {
            "goal": "Publish one safe, bounded session summary.",
            "outcomes": ["Admission and fingerprinting are complete."],
            "decisions": ["Publish immutable Markdown and JSON as one generation."],
            "unresolved_work": ["Connect publication to provider orchestration."],
            "risks": ["Never expose a partial artifact pair."],
            "next_steps": ["Implement the publication contract specified by these tests."]
        },
        "handoff_brief": "Continue with schema validation and atomic publication.",
        "relevant_files": [
            "crates/jcode-app-core/src/session_librarian/publication.rs",
            "crates/jcode-app-core/src/session_librarian/tests/publication.rs"
        ]
    })
}

fn generation_from_value(value: Value) -> LibrarianGeneration {
    LibrarianGeneration {
        response_json: serde_json::to_string(&value).expect("serialize generation fixture"),
        usage: usage(),
    }
}

fn valid_generation(fingerprint: &SourceFingerprint) -> LibrarianGeneration {
    generation_from_value(summary_value(fingerprint))
}

fn store(root: &Path) -> PublicationStore {
    PublicationStore::new(root.to_path_buf())
}

fn expect_generate(claim: PublicationClaim) -> super::publication::PublicationLease {
    match claim {
        PublicationClaim::Generate(lease) => lease,
        PublicationClaim::Reused(paths) => {
            panic!(
                "fixture unexpectedly reused {}",
                paths.directory().display()
            )
        }
    }
}

fn claim_new(
    store: &PublicationStore,
    fingerprint: &SourceFingerprint,
) -> super::publication::PublicationLease {
    expect_generate(
        store
            .claim(SESSION_ID, fingerprint)
            .expect("new fingerprint should acquire publication lease"),
    )
}

fn assert_no_generation(root: &Path, fingerprint: &SourceFingerprint) {
    assert!(
        !root.join(SESSION_ID).join(&fingerprint.digest).exists(),
        "rejected output must not expose a generation directory"
    );
}

fn read_pair(paths: &LibrarianArtifactPaths) -> (String, String) {
    let markdown = fs::read_to_string(paths.markdown()).expect("read summary.md");
    let json = fs::read_to_string(paths.json()).expect("read summary.json");
    (markdown, json)
}

#[test]
fn malformed_truncated_and_over_budget_responses_publish_nothing() {
    let cases = [
        (
            "malformed",
            LibrarianGeneration {
                response_json: "not json".into(),
                usage: usage(),
            },
        ),
        (
            "truncated",
            LibrarianGeneration {
                response_json: r#"{"format_version":"session-summary.v1""#.into(),
                usage: usage(),
            },
        ),
        ("over-output-budget", {
            let mut value = summary_value(&fingerprint());
            value["usage"]["output_tokens"] = json!(2_501);
            LibrarianGeneration {
                response_json: serde_json::to_string(&value)
                    .expect("serialize over-budget fixture"),
                usage: BoundedUsage {
                    output_tokens: 2_501,
                    ..usage()
                },
            }
        }),
    ];

    for (name, generation) in cases {
        let temp = tempfile::tempdir().expect("publication tempdir");
        let fingerprint = fingerprint();
        let error = claim_new(&store(temp.path()), &fingerprint)
            .publish_generation(generation)
            .expect_err("invalid provider output must fail closed");

        assert_eq!(
            error.stage,
            super::LibrarianFailureStage::Validation,
            "{name}"
        );
        assert_no_generation(temp.path(), &fingerprint);
    }
}

#[test]
fn mismatched_identity_usage_or_sensitive_response_publishes_nothing() {
    let mut wrong_session = summary_value(&fingerprint());
    wrong_session["session_id"] = json!("another-session");

    let mut wrong_fingerprint = summary_value(&fingerprint());
    wrong_fingerprint["source_fingerprint"]["digest"] =
        json!("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff");

    let mut wrong_usage = summary_value(&fingerprint());
    wrong_usage["usage"]["output_tokens"] = json!(419);

    let mut sensitive = summary_value(&fingerprint());
    sensitive["handoff_brief"] =
        json!("Leaked OPENAI_API_KEY=sk-or-v1-0123456789abcdefghijklmnopqrstuv");

    for (name, value) in [
        ("session", wrong_session),
        ("fingerprint", wrong_fingerprint),
        ("usage", wrong_usage),
        ("sensitive", sensitive),
    ] {
        let temp = tempfile::tempdir().expect("publication tempdir");
        let fingerprint = fingerprint();
        let error = claim_new(&store(temp.path()), &fingerprint)
            .publish_generation(generation_from_value(value))
            .expect_err("untrusted response must fail closed");

        assert_eq!(
            error.stage,
            super::LibrarianFailureStage::Validation,
            "{name}"
        );
        assert_no_generation(temp.path(), &fingerprint);
    }
}

#[test]
fn markdown_and_json_are_rendered_from_one_validated_summary() {
    let temp = tempfile::tempdir().expect("publication tempdir");
    let fingerprint = fingerprint();
    let paths = claim_new(&store(temp.path()), &fingerprint)
        .publish_generation(valid_generation(&fingerprint))
        .expect("valid generation should publish");
    let (markdown, json) = read_pair(&paths);
    let persisted: SessionSummary = serde_json::from_str(&json).expect("published JSON contract");

    assert_eq!(persisted.format_version, FORMAT_VERSION);
    assert_eq!(persisted.session_id, SESSION_ID);
    assert_eq!(persisted.source_fingerprint, fingerprint);
    assert_eq!(
        serde_json::to_value(persisted.generated_at).expect("serialize generated_at"),
        json!(GENERATED_AT)
    );
    assert_eq!(persisted.effective_route, route());
    assert_eq!(persisted.usage, usage());

    for shared_value in [
        FORMAT_VERSION,
        DIGEST,
        GENERATED_AT,
        "openai-oauth",
        "gpt-5.6-luna",
        "Publish one safe, bounded session summary.",
        "Admission and fingerprinting are complete.",
        "Continue with schema validation and atomic publication.",
    ] {
        assert!(
            markdown.contains(shared_value),
            "Markdown must project validated value {shared_value:?}"
        );
    }
    assert!(markdown.contains("640"));
    assert!(markdown.contains("420"));
    assert!(markdown.contains("24000") || markdown.contains("24,000"));
}

#[test]
fn failed_atomic_rename_preserves_an_existing_generation_and_exposes_no_new_pair() {
    let temp = tempfile::tempdir().expect("publication tempdir");
    let store = store(temp.path());
    let previous =
        fingerprint_with_digest("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    let previous_paths = claim_new(&store, &previous)
        .publish_generation(valid_generation(&previous))
        .expect("publish previous valid generation");
    let previous_pair = read_pair(&previous_paths);

    let next = fingerprint();
    let error = claim_new(&store, &next)
        .publish_generation_with_rename(valid_generation(&next), |_staging, _destination| {
            Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "simulated interruption before atomic visibility",
            ))
        })
        .expect_err("interrupted rename must fail publication");

    assert_eq!(error.stage, super::LibrarianFailureStage::Publication);
    assert_eq!(read_pair(&previous_paths), previous_pair);
    assert_no_generation(temp.path(), &next);
    assert!(
        fs::read_dir(temp.path().join(SESSION_ID))
            .expect("session publication directory")
            .all(|entry| {
                let name = entry
                    .expect("publication entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned();
                !name.contains("staging") && !name.contains("tmp")
            }),
        "failed publication must clean private staging directories"
    );
}

#[test]
fn successful_pair_becomes_visible_with_exactly_one_same_filesystem_directory_rename() {
    let temp = tempfile::tempdir().expect("publication tempdir");
    let fingerprint = fingerprint();
    let rename_calls = Arc::new(AtomicUsize::new(0));
    let observed_calls = Arc::clone(&rename_calls);
    let expected_destination = temp.path().join(SESSION_ID).join(DIGEST);

    let paths = claim_new(&store(temp.path()), &fingerprint)
        .publish_generation_with_rename(
            valid_generation(&fingerprint),
            move |staging, destination| {
                observed_calls.fetch_add(1, Ordering::SeqCst);
                assert_eq!(destination, expected_destination);
                assert_eq!(staging.parent(), destination.parent());
                assert!(
                    !destination.exists(),
                    "final generation must not be visible before rename"
                );
                assert!(staging.join("summary.md").is_file());
                assert!(staging.join("summary.json").is_file());
                fs::rename(staging, destination)
            },
        )
        .expect("single atomic rename should publish pair");

    assert_eq!(rename_calls.load(Ordering::SeqCst), 1);
    assert!(paths.markdown().is_file());
    assert!(paths.json().is_file());
}

#[test]
fn same_fingerprint_race_allows_at_most_one_generation() {
    let temp = tempfile::tempdir().expect("publication tempdir");
    let store = Arc::new(store(temp.path()));
    let barrier = Arc::new(Barrier::new(2));
    let generations = Arc::new(AtomicUsize::new(0));

    let handles = (0..2)
        .map(|_| {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            let generations = Arc::clone(&generations);
            thread::spawn(move || {
                let fingerprint = fingerprint();
                barrier.wait();
                match store
                    .claim(SESSION_ID, &fingerprint)
                    .expect("racing claim should resolve")
                {
                    PublicationClaim::Generate(lease) => {
                        generations.fetch_add(1, Ordering::SeqCst);
                        thread::sleep(Duration::from_millis(50));
                        lease
                            .publish_generation(valid_generation(&fingerprint))
                            .expect("winning claim should publish");
                        "generated"
                    }
                    PublicationClaim::Reused(paths) => {
                        assert!(paths.markdown().is_file());
                        assert!(paths.json().is_file());
                        "reused"
                    }
                }
            })
        })
        .collect::<Vec<_>>();

    let mut outcomes = handles
        .into_iter()
        .map(|handle| handle.join().expect("publication race thread"))
        .collect::<Vec<_>>();
    outcomes.sort_unstable();

    assert_eq!(outcomes, ["generated", "reused"]);
    assert_eq!(generations.load(Ordering::SeqCst), 1);
}

#[test]
fn completed_generation_is_reused_without_another_rename() {
    let temp = tempfile::tempdir().expect("publication tempdir");
    let store = store(temp.path());
    let fingerprint = fingerprint();
    let published = claim_new(&store, &fingerprint)
        .publish_generation(valid_generation(&fingerprint))
        .expect("publish initial generation");

    let reused = match store
        .claim(SESSION_ID, &fingerprint)
        .expect("completed generation should be reusable")
    {
        PublicationClaim::Reused(paths) => paths,
        PublicationClaim::Generate(_) => panic!("existing valid pair must not regenerate"),
    };

    assert_eq!(reused, published);
}

#[test]
fn dead_stale_lock_is_reclaimed_before_generation() {
    let temp = tempfile::tempdir().expect("publication tempdir");
    let fingerprint = fingerprint();
    let session_directory = temp.path().join(SESSION_ID);
    let lock_directory = session_directory.join(format!(".{}.lock", fingerprint.digest));
    fs::create_dir_all(&lock_directory).expect("create stale lock directory");
    fs::write(
        lock_directory.join("owner.json"),
        serde_json::to_vec(&json!({
            "owner_pid": u32::MAX,
            "created_at_unix_ms": 0_u64
        }))
        .expect("serialize stale lock metadata"),
    )
    .expect("write stale lock metadata");

    let started = Instant::now();
    let lease = claim_new(&store(temp.path()), &fingerprint);

    assert!(
        started.elapsed() < Duration::from_secs(1),
        "a dead stale lock should be reclaimed without waiting for the contention timeout"
    );
    drop(lease);
    assert!(!lock_directory.exists());
}

#[test]
fn metadata_less_stale_lock_is_reclaimed_before_generation() {
    let temp = tempfile::tempdir().expect("publication tempdir");
    let fingerprint = fingerprint();
    let session_directory = temp.path().join(SESSION_ID);
    let lock_directory = session_directory.join(format!(".{}.lock", fingerprint.digest));
    fs::create_dir_all(&lock_directory).expect("create legacy stale lock directory");
    File::open(&lock_directory)
        .expect("open legacy stale lock directory")
        .set_times(FileTimes::new().set_modified(UNIX_EPOCH))
        .expect("age legacy stale lock directory");

    let started = Instant::now();
    let lease = claim_new(&store(temp.path()), &fingerprint);

    assert!(started.elapsed() < Duration::from_secs(1));
    drop(lease);
    assert!(!lock_directory.exists());
}
