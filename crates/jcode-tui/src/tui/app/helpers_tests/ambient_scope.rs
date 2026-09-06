use super::super::{AMBIENT_INFO_CACHE, gather_ambient_info, invalidate_ambient_info_cache};
use super::*;

fn schedule(manager: &mut AmbientManager, target: ScheduleTarget, minutes: i64, task: &str) {
    manager
        .schedule(ScheduleRequest {
            wake_in_minutes: None,
            wake_at: Some(Utc::now() + ChronoDuration::minutes(minutes)),
            context: task.to_string(),
            priority: Priority::Normal,
            target,
            // Deliberately not the destination. Display ownership follows delivery.
            created_by_session: "creator".to_string(),
            working_dir: None,
            task_description: Some(task.to_string()),
            relevant_files: Vec::new(),
            git_branch: None,
            additional_context: None,
        })
        .expect("schedule fixture");
}

#[test]
fn ambient_info_uses_destination_and_spawn_parent_not_creator() {
    let _env_lock = crate::storage::lock_test_env();
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = EnvVarGuard::set_path("JCODE_HOME", temp.path());
    let mut manager = AmbientManager::new().expect("manager");
    schedule(&mut manager, ScheduleTarget::Ambient, 1, "ambient work");
    schedule(
        &mut manager,
        ScheduleTarget::Session {
            session_id: "a".into(),
        },
        10,
        "a reminder",
    );
    schedule(
        &mut manager,
        ScheduleTarget::Spawn {
            parent_session_id: "a".into(),
        },
        5,
        "a child",
    );
    schedule(
        &mut manager,
        ScheduleTarget::Session {
            session_id: "b".into(),
        },
        2,
        "b reminder",
    );
    schedule(
        &mut manager,
        ScheduleTarget::Spawn {
            parent_session_id: "b".into(),
        },
        3,
        "b child",
    );

    for enabled in [false, true] {
        for (session, preview) in [("a", "a child"), ("b", "b reminder")] {
            let info = gather_ambient_info_inner(enabled, Some(session)).expect("owned tasks");
            assert_eq!(info.reminder_count, 2);
            assert_eq!(info.queue_count, 3);
            assert_eq!(info.next_reminder_preview.as_deref(), Some(preview));
            assert_eq!(info.next_queue_preview.as_deref(), Some("ambient work"));
        }
        for session in [None, Some("empty"), Some("creator")] {
            let info = gather_ambient_info_inner(enabled, session);
            if enabled {
                let info = info.expect("global ambient widget");
                assert_eq!(info.reminder_count, 0);
                assert_eq!(info.queue_count, 1);
                assert!(info.next_reminder_preview.is_none());
                assert!(info.next_reminder_wake.is_none());
                assert_eq!(info.next_queue_preview.as_deref(), Some("ambient work"));
            } else {
                assert!(info.is_none(), "no scheduled indicator for {session:?}");
            }
        }
    }
}

#[test]
fn ambient_info_cache_isolates_session_switches_and_background_refreshes() {
    let _env_lock = crate::storage::lock_test_env();
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = EnvVarGuard::set_path("JCODE_HOME", temp.path());
    let mut manager = AmbientManager::new().expect("manager");
    for (session, minutes) in [("cache-a", 10), ("cache-b", 1)] {
        schedule(
            &mut manager,
            ScheduleTarget::Session {
                session_id: session.into(),
            },
            minutes,
            session,
        );
    }
    let sessions = [Some("cache-a"), Some("cache-b"), Some("cache-empty"), None];
    // Pin every key in the cold/in-flight state, then invalidate together. No
    // unrelated test refresh is needed to make this regression deterministic.
    {
        let mut cache = AMBIENT_INFO_CACHE.lock().expect("cache");
        for session in sessions {
            cache.insert(
                (session.map(str::to_owned), false),
                (std::time::Instant::now(), None, true),
            );
        }
    }
    invalidate_ambient_info_cache();
    for session in sessions {
        assert!(gather_ambient_info(false, session).is_none());
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let settled = {
            let cache = AMBIENT_INFO_CACHE.lock().expect("cache");
            sessions.iter().all(|session| {
                cache
                    .get(&(session.map(str::to_owned), false))
                    .is_some_and(|entry| !entry.2)
            })
        };
        if settled {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "refreshes must settle before removing fixture home"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    // Repeated switches, including two readers of the same session, must never
    // borrow another key's fresh data. No I/O is spawned by these cache hits.
    for session in [
        Some("cache-b"),
        None,
        Some("cache-a"),
        Some("cache-a"),
        Some("cache-empty"),
    ] {
        let info = gather_ambient_info(false, session);
        match session {
            Some("cache-a" | "cache-b") => {
                let info = info.expect("owned reminder");
                assert_eq!(info.reminder_count, 1);
                assert_eq!(info.next_reminder_preview.as_deref(), session);
                assert_eq!(info.next_queue_preview.as_deref(), session);
            }
            _ => assert!(info.is_none(), "empty session must have no reminder"),
        }
    }
    let mut cache = AMBIENT_INFO_CACHE.lock().expect("cache");
    for session in sessions {
        cache.remove(&(session.map(str::to_owned), false));
    }
}
