use super::*;

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
