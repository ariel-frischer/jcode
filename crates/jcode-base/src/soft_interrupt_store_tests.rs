use super::*;
use serde_json::{Value, json};

fn ordered_images(label: &str) -> Value {
    json!([
        ["image/png", format!("{label}-png-bytes-base64")],
        ["image/jpeg", format!("{label}-jpeg-bytes-base64")],
        ["image/webp", format!("{label}-webp-bytes-base64")]
    ])
}

fn persisted_user_message(label: &str, enqueue_sequence: u64) -> Value {
    json!({
        "content": format!("original {label}"),
        "images": ordered_images(label),
        "urgent": false,
        "source": "user",
        "message_id": format!("message-{label}"),
        "owner_client_instance_id": "client-owner",
        "enqueue_sequence": enqueue_sequence,
    })
}

fn write_store_fixture(session_id: &str, value: &Value) -> PathBuf {
    let path = path_for_session(session_id).expect("soft interrupt fixture path");
    std::fs::create_dir_all(path.parent().expect("soft interrupt fixture parent"))
        .expect("create soft interrupt fixture parent");
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(value).expect("serialize soft interrupt fixture"),
    )
    .expect("write soft interrupt fixture");
    path
}

struct JcodeHomeRestore(Option<std::ffi::OsString>);

impl Drop for JcodeHomeRestore {
    fn drop(&mut self) {
        if let Some(previous) = self.0.take() {
            crate::env::set_var("JCODE_HOME", previous);
        } else {
            crate::env::remove_var("JCODE_HOME");
        }
    }
}

fn with_test_home<T>(test: impl FnOnce() -> T) -> T {
    let _guard = crate::storage::lock_test_env();
    let temp = tempfile::TempDir::new().expect("temp dir");
    let prev_home = std::env::var_os("JCODE_HOME");
    let _restore = JcodeHomeRestore(prev_home);
    crate::env::set_var("JCODE_HOME", temp.path());
    test()
}

#[test]
fn append_take_and_clear_round_trip() {
    let _guard = crate::storage::lock_test_env();
    let temp = tempfile::TempDir::new().expect("temp dir");
    let prev_home = std::env::var_os("JCODE_HOME");
    crate::env::set_var("JCODE_HOME", temp.path());

    let session_id = "ses_soft_interrupt_store";
    append(
        session_id,
        SoftInterruptMessage {
            content: "hello".to_string(),
            images: Vec::new(),
            urgent: true,
            source: SoftInterruptSource::System,
            message_id: None,
            owner_client_instance_id: None,
        },
    )
    .expect("append first interrupt");
    append(
        session_id,
        SoftInterruptMessage {
            content: "world".to_string(),
            images: Vec::new(),
            urgent: false,
            source: SoftInterruptSource::BackgroundTask,
            message_id: None,
            owner_client_instance_id: None,
        },
    )
    .expect("append second interrupt");

    let loaded = load(session_id).expect("load interrupts");
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].content, "hello");
    assert!(loaded[0].urgent);
    assert_eq!(loaded[1].content, "world");

    let taken = take(session_id).expect("take interrupts");
    assert_eq!(taken.len(), 2);
    assert!(load(session_id).expect("reload after take").is_empty());

    append(
        session_id,
        SoftInterruptMessage {
            content: "later".to_string(),
            images: Vec::new(),
            urgent: false,
            source: SoftInterruptSource::User,
            message_id: None,
            owner_client_instance_id: None,
        },
    )
    .expect("append later interrupt");
    clear(session_id).expect("clear interrupts");
    assert!(load(session_id).expect("load after clear").is_empty());

    if let Some(prev_home) = prev_home {
        crate::env::set_var("JCODE_HOME", prev_home);
    } else {
        crate::env::remove_var("JCODE_HOME");
    }
}

#[test]
fn identity_and_owner_round_trip_while_legacy_entries_default_to_unowned() {
    let _guard = crate::storage::lock_test_env();
    let temp = tempfile::TempDir::new().expect("temp dir");
    let prev_home = std::env::var_os("JCODE_HOME");
    crate::env::set_var("JCODE_HOME", temp.path());

    let session_id = "ses_soft_interrupt_identity";
    append(
        session_id,
        SoftInterruptMessage {
            content: "owned".to_string(),
            images: vec![("image/png".to_string(), "cG5n".to_string())],
            urgent: false,
            source: SoftInterruptSource::User,
            message_id: Some("soft-interrupt-1".to_string()),
            owner_client_instance_id: Some("client-instance-1".to_string()),
        },
    )
    .expect("append owned interrupt");

    let loaded = load(session_id).expect("load owned interrupt");
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].message_id.as_deref(), Some("soft-interrupt-1"));
    assert_eq!(
        loaded[0].owner_client_instance_id.as_deref(),
        Some("client-instance-1")
    );
    assert_eq!(
        loaded[0].images,
        vec![("image/png".to_string(), "cG5n".to_string())]
    );

    let legacy_session_id = "ses_soft_interrupt_legacy";
    let legacy_path = path_for_session(legacy_session_id).expect("legacy path");
    std::fs::create_dir_all(legacy_path.parent().expect("legacy parent"))
        .expect("create legacy parent");
    std::fs::write(
        &legacy_path,
        r#"[{"content":"legacy","urgent":false,"source":"user"}]"#,
    )
    .expect("write legacy fixture");

    let legacy = load(legacy_session_id).expect("load legacy interrupt");
    assert_eq!(legacy.len(), 1);
    assert_eq!(legacy[0].content, "legacy");
    assert!(legacy[0].images.is_empty());
    assert_eq!(legacy[0].message_id, None);
    assert_eq!(legacy[0].owner_client_instance_id, None);

    if let Some(prev_home) = prev_home {
        crate::env::set_var("JCODE_HOME", prev_home);
    } else {
        crate::env::remove_var("JCODE_HOME");
    }
}

#[test]
fn versioned_envelope_recovers_originals_not_working_drafts_exactly_once() {
    with_test_home(|| {
        let session_id = "ses_versioned_soft_interrupt_recovery";
        let pre_snapshot = persisted_user_message("pre-snapshot", 5);
        let original_old = persisted_user_message("held-old", 10);
        let original_new = persisted_user_message("held-new", 11);
        let post_snapshot = persisted_user_message("post-snapshot", 20);
        let fixture = json!({
            "version": 1,
            "dispatchable": [pre_snapshot, post_snapshot],
            "reservations": [{
                "navigation_session_id": "navigation-session-1",
                "owner_client_instance_id": "client-owner",
                "snapshot_queue_sequence": 11,
                "state": "active",
                "selected_index": 1,
                "predecessor_message_id": "message-pre-snapshot",
                "successor_message_id": "message-post-snapshot",
                "held": [
                    {
                        "message_id": "message-held-old",
                        "original": original_old,
                        "draft": {
                            "content": "uncommitted edited old",
                            "images": ordered_images("working-old")
                        },
                        "original_relative_index": 0
                    },
                    {
                        "message_id": "message-held-new",
                        "original": original_new,
                        "draft": {
                            "content": "uncommitted edited new",
                            "images": ordered_images("working-new")
                        },
                        "original_relative_index": 1
                    }
                ],
                "completed_operations": []
            }]
        });
        let path = write_store_fixture(session_id, &fixture);

        let first = load(session_id).expect("versioned envelope should recover after restart");
        assert_eq!(
            first
                .iter()
                .map(|message| message.content.as_str())
                .collect::<Vec<_>>(),
            vec![
                "original pre-snapshot",
                "original held-old",
                "original held-new",
                "original post-snapshot"
            ]
        );
        assert_eq!(
            first[1].images,
            vec![
                (
                    "image/png".to_string(),
                    "held-old-png-bytes-base64".to_string()
                ),
                (
                    "image/jpeg".to_string(),
                    "held-old-jpeg-bytes-base64".to_string()
                ),
                (
                    "image/webp".to_string(),
                    "held-old-webp-bytes-base64".to_string()
                )
            ],
            "restart recovery must restore immutable ordered original images"
        );
        assert!(
            first
                .iter()
                .all(|message| !message.content.starts_with("uncommitted")),
            "working drafts must never be applied by abandoned-session recovery"
        );

        let second = load(session_id).expect("recovered envelope should remain readable");
        let identity_and_content = |messages: &[SoftInterruptMessage]| {
            messages
                .iter()
                .map(|message| {
                    (
                        message.message_id.clone(),
                        message.content.clone(),
                        message.images.clone(),
                        message.owner_client_instance_id.clone(),
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            identity_and_content(&second),
            identity_and_content(&first),
            "repeated recovery must not duplicate or alter held records"
        );

        let recovered: Value = serde_json::from_slice(
            &std::fs::read(path).expect("read recovered versioned envelope"),
        )
        .expect("recovered store remains valid JSON");
        assert_eq!(recovered["version"], 1);
        assert_eq!(recovered["reservations"], json!([]));
        assert_eq!(
            recovered["dispatchable"]
                .as_array()
                .expect("dispatchable array")
                .len(),
            4
        );
    });
}

#[test]
fn overwrite_emits_one_versioned_envelope_without_partial_temp_state() {
    with_test_home(|| {
        let session_id = "ses_versioned_soft_interrupt_overwrite";
        let messages = vec![SoftInterruptMessage {
            content: "persist atomically".to_string(),
            images: vec![
                (
                    "image/png".to_string(),
                    "atomic-png-bytes-base64".to_string(),
                ),
                (
                    "image/jpeg".to_string(),
                    "atomic-jpeg-bytes-base64".to_string(),
                ),
            ],
            urgent: false,
            source: SoftInterruptSource::User,
            message_id: Some("message-atomic".to_string()),
            owner_client_instance_id: Some("client-owner".to_string()),
        }];

        overwrite(session_id, &messages).expect("overwrite versioned soft interrupt envelope");
        let path = path_for_session(session_id).expect("versioned soft interrupt path");
        let persisted: Value = serde_json::from_slice(
            &std::fs::read(&path).expect("read versioned soft interrupt envelope"),
        )
        .expect("versioned soft interrupt envelope is valid JSON");
        assert_eq!(persisted["version"], 1);
        assert_eq!(persisted["dispatchable"].as_array().map(Vec::len), Some(1));
        assert_eq!(persisted["reservations"], json!([]));
        assert_eq!(
            persisted["dispatchable"][0]["images"],
            json!([
                ["image/png", "atomic-png-bytes-base64"],
                ["image/jpeg", "atomic-jpeg-bytes-base64"]
            ])
        );

        let sibling_names = std::fs::read_dir(path.parent().expect("store parent"))
            .expect("read store parent")
            .map(|entry| {
                entry
                    .expect("store sibling")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            sibling_names,
            vec![path.file_name().unwrap().to_string_lossy()]
        );
    });
}

#[test]
fn corrupt_or_unsupported_versioned_envelopes_fail_visibly() {
    with_test_home(|| {
        let unsupported_session = "ses_unsupported_soft_interrupt_envelope";
        write_store_fixture(
            unsupported_session,
            &json!({
                "version": 999,
                "dispatchable": [],
                "reservations": []
            }),
        );
        let error = load(unsupported_session).expect_err("unsupported version must fail");
        assert!(!error.to_string().is_empty());

        let corrupt_session = "ses_corrupt_soft_interrupt_envelope";
        write_store_fixture(
            corrupt_session,
            &json!({
                "version": 1,
                "dispatchable": "not-an-array",
                "reservations": [{"navigation_session_id": "missing-required-fields"}]
            }),
        );
        let error = load(corrupt_session).expect_err("corrupt envelope must fail");
        assert!(!error.to_string().is_empty());
    });
}
