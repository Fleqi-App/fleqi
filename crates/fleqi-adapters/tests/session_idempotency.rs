//! Real SQLite receipts, restart replay, concurrent delivery and transaction rollback.
use fleqi_adapters::storage::{Database, SqliteSessionStore};
use fleqi_application::{
    dto::{AppEvent, ErrorCode},
    ports::{EventSink, SequenceIds, SessionStore, StorageError, SystemClock},
    session_service::{SessionGroup, SessionService},
};
use fleqi_domain::{
    context::{ContextSnapshot, ContextSnapshotBuilder, PathKind, PathRef},
    idempotency::Receipt,
    revision::Revision,
    session::EntryRole,
};
use std::sync::{
    Arc, Barrier,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Default)]
struct Events(AtomicUsize);
impl EventSink for Events {
    fn emit(&self, _event: AppEvent) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
fn service(db: Arc<Database>, events: Arc<Events>) -> Arc<SessionService> {
    Arc::new(
        SessionService::load(
            Arc::new(SqliteSessionStore::new(db)),
            Arc::new(SystemClock),
            Arc::new(SequenceIds::new()),
            events,
        )
        .unwrap(),
    )
}
fn context(path: &str) -> ContextSnapshot {
    ContextSnapshotBuilder::new("context", Revision::new(1), "2026-09-20T00:00:00Z")
        .directory(Some(PathRef::new("directory", path, PathKind::Directory)))
        .build()
}
fn sql(db: &Database, statement: &str) {
    let statement = statement.to_owned();
    db.run(move |connection| {
        connection
            .execute_batch(&statement)
            .map_err(|e| StorageError::Io(e.to_string()))
    })
    .unwrap();
}
fn end_all(sessions: &SessionService, request: &str) -> usize {
    sessions
        .run_request(request, "session_end_all", &(), || {
            let ids = sessions.active_ids();
            for id in &ids {
                sessions.begin_end(id)?;
                sessions.mark_ended(id, false)?;
            }
            Ok(ids.len())
        })
        .unwrap()
}

#[test]
fn update_pause_prevents_late_session_creation_and_resumes_after_failure() {
    let directory = tempfile::tempdir().unwrap();
    let sessions = service(
        Arc::new(Database::open(directory.path()).unwrap()),
        Arc::new(Events::default()),
    );
    sessions.pause_creation_for_update().unwrap();
    assert!(sessions.create(None, None).is_err());
    assert!(sessions.create_request("late-request", None).is_err());
    sessions.resume_creation_after_update();
    sessions.create_request("late-request", None).unwrap();
    assert!(sessions.pause_creation_for_update().is_err());
    assert_eq!(sessions.active_count(), 1);
}

#[test]
fn create_receipt_replays_across_context_changes_and_database_restart() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(Events::default());
    let created;
    {
        let db = Arc::new(Database::open(dir.path()).unwrap());
        let sessions = service(db.clone(), events.clone());
        created = sessions
            .create_request("create-1", Some(&context("/first")))
            .unwrap();
        assert_eq!(
            sessions
                .create_request("create-1", Some(&context("/later")))
                .unwrap(),
            created
        );
        assert_eq!(sessions.list(SessionGroup::All, 0, 100).len(), 1);
        assert_eq!(events.0.load(Ordering::SeqCst), 1);
        assert!(
            SqliteSessionStore::new(db)
                .find_receipt("create-1")
                .unwrap()
                .is_some()
        );
    }
    let db = Arc::new(Database::open(dir.path()).unwrap());
    let sessions = service(db, events.clone());
    assert_eq!(
        sessions
            .create_request("create-1", Some(&context("/after-restart")))
            .unwrap(),
        created
    );
    assert_eq!(sessions.list(SessionGroup::All, 0, 100).len(), 1);
    assert_eq!(
        sessions.active_count(),
        0,
        "replay must not revive the interrupted process"
    );
    assert_eq!(events.0.load(Ordering::SeqCst), 1);
}

#[test]
fn continuation_receipt_keeps_one_child_and_rejects_changed_parent() {
    let dir = tempfile::tempdir().unwrap();
    let child;
    let parent;
    let other;
    {
        let db = Arc::new(Database::open(dir.path()).unwrap());
        let sessions = service(db, Arc::new(Events::default()));
        parent = sessions.create(None, None).unwrap();
        sessions.mark_ended(&parent.id, false).unwrap();
        other = sessions.create(None, None).unwrap();
        sessions.mark_ended(&other.id, false).unwrap();
        child = sessions
            .continue_request("continue-1", &parent.id, Some(&context("/current")))
            .unwrap();
        assert_ne!(child.id, parent.id);
        assert_eq!(child.parent_session_id.as_deref(), Some(parent.id.as_str()));
        assert_eq!(
            sessions
                .continue_request("continue-1", &parent.id, None)
                .unwrap(),
            child
        );
        assert_eq!(
            sessions
                .continue_request("continue-1", &other.id, None)
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
    }
    let sessions = service(
        Arc::new(Database::open(dir.path()).unwrap()),
        Arc::new(Events::default()),
    );
    assert_eq!(
        sessions
            .continue_request("continue-1", &parent.id, None)
            .unwrap(),
        child
    );
    assert_eq!(sessions.list(SessionGroup::All, 0, 100).len(), 3);
    assert_eq!(
        sessions
            .create_request("continue-1", None)
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
}

#[test]
fn concurrent_duplicate_creation_commits_and_emits_once() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(Events::default());
    let sessions = service(
        Arc::new(Database::open(dir.path()).unwrap()),
        events.clone(),
    );
    let start = Arc::new(Barrier::new(12));
    let handles = (0..12)
        .map(|_| {
            let sessions = sessions.clone();
            let start = start.clone();
            std::thread::spawn(move || {
                start.wait();
                sessions.create_request("same-create", None).unwrap()
            })
        })
        .collect::<Vec<_>>();
    let results = handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect::<Vec<_>>();
    assert!(results.iter().all(|s| s.id == results[0].id));
    assert_eq!(sessions.active_count(), 1);
    assert_eq!(events.0.load(Ordering::SeqCst), 1);
}

#[test]
fn select_replays_without_reapplying_effect_and_payload_conflicts_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let effects = AtomicUsize::new(0);
    {
        let sessions = service(
            Arc::new(Database::open(dir.path()).unwrap()),
            Arc::new(Events::default()),
        );
        let result: String = sessions
            .run_request("select-1", "session_select", &"first", || {
                effects.fetch_add(1, Ordering::SeqCst);
                Ok("selected first".into())
            })
            .unwrap();
        assert_eq!(result, "selected first");
        let replay: String = sessions
            .run_request("select-1", "session_select", &"first", || {
                effects.fetch_add(1, Ordering::SeqCst);
                Ok("duplicate effect".into())
            })
            .unwrap();
        assert_eq!(replay, result);
        let changed = sessions.run_request("select-1", "session_select", &"other", || {
            Ok("wrong".to_owned())
        });
        assert_eq!(changed.unwrap_err().code, ErrorCode::Conflict);
    }
    let sessions = service(
        Arc::new(Database::open(dir.path()).unwrap()),
        Arc::new(Events::default()),
    );
    let replay: String = sessions
        .run_request("select-1", "session_select", &"first", || {
            effects.fetch_add(1, Ordering::SeqCst);
            Ok("after restart".into())
        })
        .unwrap();
    assert_eq!(replay, "selected first");
    assert_eq!(effects.load(Ordering::SeqCst), 1);
}

#[test]
fn repeated_end_all_does_not_end_sessions_created_after_first_request() {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Database::open(dir.path()).unwrap());
    let sessions = service(db.clone(), Arc::new(Events::default()));
    sessions.create(None, None).unwrap();
    sessions.create(None, None).unwrap();
    assert_eq!(end_all(&sessions, "end-all"), 2);
    let later = sessions.create(None, None).unwrap();
    assert_eq!(end_all(&sessions, "end-all"), 2);
    assert!(sessions.get(&later.id).unwrap().state.is_active());
    drop(sessions);
    drop(db);
    let sessions = service(
        Arc::new(Database::open(dir.path()).unwrap()),
        Arc::new(Events::default()),
    );
    let replay: usize = sessions
        .run_request("end-all", "session_end_all", &(), || {
            panic!("replayed end_all must never execute after restart")
        })
        .unwrap();
    assert_eq!(replay, 2);
}

#[test]
fn end_and_delete_receipts_do_not_repeat_hooks_or_fail_after_record_is_gone() {
    let dir = tempfile::tempdir().unwrap();
    let end_count = Arc::new(AtomicUsize::new(0));
    let delete_count = AtomicUsize::new(0);
    let deleted;
    let revision;
    {
        let sessions = service(
            Arc::new(Database::open(dir.path()).unwrap()),
            Arc::new(Events::default()),
        );
        let calls = end_count.clone();
        sessions
            .set_end_hook(Arc::new(move |_| {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }))
            .unwrap();
        deleted = sessions.create(None, None).unwrap();
        let end = || {
            sessions.run_request("end-1", "session_end", &deleted.id, || {
                sessions.begin_end(&deleted.id)?;
                sessions.mark_ended(&deleted.id, false)
            })
        };
        let ended = end().unwrap();
        assert_eq!(end().unwrap(), ended);
        assert_eq!(end_count.load(Ordering::SeqCst), 1);
        revision = ended.revision;
        sessions
            .delete_request("delete-1", &deleted.id, revision, |_| {
                delete_count.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
            .unwrap();
        sessions
            .delete_request("delete-1", &deleted.id, revision, |_| {
                panic!("duplicate deletion hook")
            })
            .unwrap();
        assert!(sessions.get(&deleted.id).is_err());
        assert_eq!(delete_count.load(Ordering::SeqCst), 0);
        assert_eq!(
            sessions
                .delete_request("delete-1", &deleted.id, revision.next(), |_| Ok(()))
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
    }
    let sessions = service(
        Arc::new(Database::open(dir.path()).unwrap()),
        Arc::new(Events::default()),
    );
    sessions
        .delete_request("delete-1", &deleted.id, revision, |_| {
            panic!("restart must replay deletion")
        })
        .unwrap();
}

#[test]
fn create_and_receipt_are_atomic_when_receipt_insert_fails() {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Database::open(dir.path()).unwrap());
    let events = Arc::new(Events::default());
    let sessions = service(db.clone(), events.clone());
    sql(
        &db,
        "CREATE TRIGGER reject_receipt BEFORE INSERT ON request_receipts WHEN NEW.request_id = 'failed-create' BEGIN SELECT RAISE(ABORT, 'receipt fault'); END;",
    );
    assert!(sessions.create_request("failed-create", None).is_err());
    let store = SqliteSessionStore::new(db.clone());
    assert!(store.load_all().unwrap().is_empty());
    assert!(store.find_receipt("failed-create").unwrap().is_none());
    assert_eq!(sessions.active_count(), 0);
    assert_eq!(events.0.load(Ordering::SeqCst), 0);
    sql(&db, "DROP TRIGGER reject_receipt;");
    let created = sessions.create_request("failed-create", None).unwrap();
    assert_eq!(
        sessions.create_request("failed-create", None).unwrap(),
        created
    );
    assert_eq!(store.load_all().unwrap().len(), 1);
}

#[test]
fn failed_deletion_receipt_rolls_back_session_and_conversation_deletion() {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Database::open(dir.path()).unwrap());
    let sessions = service(db.clone(), Arc::new(Events::default()));
    let created = sessions.create(None, None).unwrap();
    sessions
        .append_entry(
            &created.id,
            EntryRole::User,
            "retain this conversation",
            None,
        )
        .unwrap();
    let ended = sessions.mark_ended(&created.id, false).unwrap();
    sql(
        &db,
        "CREATE TRIGGER reject_delete BEFORE INSERT ON request_receipts WHEN NEW.request_id = 'failed-delete' BEGIN SELECT RAISE(ABORT, 'receipt fault'); END;",
    );
    assert!(
        sessions
            .delete_request("failed-delete", &created.id, ended.revision, |_| Ok(()))
            .is_err()
    );
    let store = SqliteSessionStore::new(db.clone());
    assert_eq!(store.load_all().unwrap().len(), 1);
    assert_eq!(store.entries(&created.id, 10, None).unwrap().len(), 1);
    assert!(store.find_receipt("failed-delete").unwrap().is_none());
    assert!(sessions.get(&created.id).is_ok());
    sql(&db, "DROP TRIGGER reject_delete;");
    sessions
        .delete_request("failed-delete", &created.id, ended.revision, |_| Ok(()))
        .unwrap();
    assert!(store.load_all().unwrap().is_empty());
    assert!(store.entries(&created.id, 10, None).unwrap().is_empty());
    assert!(store.find_receipt("failed-delete").unwrap().is_some());
}

#[test]
fn storage_replay_never_applies_a_second_session_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Database::open(dir.path()).unwrap());
    let sessions = service(db.clone(), Arc::new(Events::default()));
    let one = sessions.create(None, None).unwrap();
    let mut two = one.clone();
    two.id = "second-session".into();
    let store = SqliteSessionStore::new(db);
    let receipt = Receipt {
        request_id: "storage-race".into(),
        fingerprint: "first".into(),
        result_json: "null".into(),
    };
    assert_eq!(store.commit_request(&[], &[], &receipt).unwrap(), receipt);
    let different = Receipt {
        fingerprint: "different".into(),
        ..receipt.clone()
    };
    assert_eq!(
        store
            .commit_request(&[two], std::slice::from_ref(&one.id), &different)
            .unwrap(),
        receipt
    );
    assert_eq!(store.load_all().unwrap(), vec![one]);
}

#[test]
fn blank_request_ids_fail_before_any_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let sessions = service(
        Arc::new(Database::open(dir.path()).unwrap()),
        Arc::new(Events::default()),
    );
    assert!(sessions.create_request("  ", None).is_err());
    let result = sessions.run_request(
        "",
        "session_select",
        &"id",
        || -> fleqi_application::dto::AppResult<()> { panic!("invalid request must not execute") },
    );
    assert!(result.is_err());
    assert_eq!(sessions.active_count(), 0);
}

#[test]
fn concurrent_same_id_different_payloads_conflict_instead_of_sharing_wrong_result() {
    let dir = tempfile::tempdir().unwrap();
    let sessions = service(
        Arc::new(Database::open(dir.path()).unwrap()),
        Arc::new(Events::default()),
    );
    let start = Arc::new(Barrier::new(2));
    let effects = Arc::new(AtomicUsize::new(0));
    let handles = ["first", "second"]
        .into_iter()
        .map(|payload| {
            let sessions = sessions.clone();
            let start = start.clone();
            let effects = effects.clone();
            std::thread::spawn(move || {
                start.wait();
                sessions.run_request("concurrent-select", "session_select", &payload, || {
                    effects.fetch_add(1, Ordering::SeqCst);
                    Ok(payload.to_owned())
                })
            })
        })
        .collect::<Vec<_>>();
    let results = handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter_map(|r| r.as_ref().err())
            .filter(|e| e.code == ErrorCode::Conflict)
            .count(),
        1
    );
    assert_eq!(effects.load(Ordering::SeqCst), 1);
}
