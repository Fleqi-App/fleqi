//! M1.1 应用服务测试：内存端口验证幂等/冲突/并发合并/允许表/降级/权限代际/上下文规则。

use fleqi_application::context_service::ContextService;
use fleqi_application::dto::{
    AppEvent, DirectoryPickResult, ErrorCode, SettingsUpdateRequest, StorageState,
};
use fleqi_application::lifecycle::HostLifecycle;
use fleqi_application::paths::PathRegistry;
use fleqi_application::permission_service::PermissionService;
use fleqi_application::ports::{
    Clock, ContextPort, DirectoryPick, EventSink, PermissionPort, PermissionProbe,
    PersistedSettings, RawContext, RawPath, SettingsStore, Spawner, StorageError,
};
use fleqi_application::settings_service::SettingsService;
use fleqi_domain::context::{ContextAvailability, ContextSource, PathKind, ViewKind};
use fleqi_domain::idempotency::Receipt;
use fleqi_domain::lifecycle::HostState;
use fleqi_domain::permissions::{Permission, PermissionProcedure, PermissionStatus};
use fleqi_domain::revision::Revision;
use fleqi_domain::settings::{FieldAllowlist, Settings, SettingsPatch, Theme};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

struct FakeClock;
impl Clock for FakeClock {
    fn now_rfc3339(&self) -> String {
        "2026-09-17T12:00:00Z".into()
    }
}

#[derive(Default)]
struct Events(Mutex<Vec<AppEvent>>);
impl EventSink for Events {
    fn emit(&self, event: AppEvent) {
        self.0.lock().unwrap().push(event);
    }
}
impl Events {
    fn count(&self, name: &str) -> usize {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.name() == name)
            .count()
    }
}

#[derive(Default)]
struct MemoryStore {
    settings: Mutex<Option<PersistedSettings>>,
    receipts: Mutex<HashMap<String, Receipt>>,
    commits: AtomicUsize,
    fail_load: bool,
    commit_delay_ms: u64,
}
impl SettingsStore for MemoryStore {
    fn load(&self) -> Result<Option<PersistedSettings>, StorageError> {
        if self.fail_load {
            return Err(StorageError::Corrupt(
                "database disk image is malformed".into(),
            ));
        }
        Ok(self.settings.lock().unwrap().clone())
    }
    fn commit(
        &self,
        settings: &Settings,
        expected: Option<Revision>,
        receipt: &Receipt,
    ) -> Result<Revision, StorageError> {
        if self.commit_delay_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(self.commit_delay_ms));
        }
        let mut current = self.settings.lock().unwrap();
        let current_rev = current
            .as_ref()
            .map(|p| p.revision)
            .unwrap_or(Revision::new(0));
        if let Some(expected) = expected
            && expected != current_rev
        {
            return Err(StorageError::RevisionMismatch {
                current: current_rev,
            });
        }
        let next = current_rev.next();
        *current = Some(PersistedSettings {
            settings: settings.clone(),
            revision: next,
        });
        self.receipts
            .lock()
            .unwrap()
            .insert(receipt.request_id.clone(), receipt.clone());
        self.commits.fetch_add(1, Ordering::SeqCst);
        Ok(next)
    }
    fn find_receipt(&self, request_id: &str) -> Result<Option<Receipt>, StorageError> {
        Ok(self.receipts.lock().unwrap().get(request_id).cloned())
    }
}

struct InlineSpawner;
impl Spawner for InlineSpawner {
    fn spawn(&self, work: Box<dyn FnOnce() + Send>) {
        work();
    }
}

struct DeferredSpawner(Mutex<Vec<Box<dyn FnOnce() + Send>>>);
impl Spawner for DeferredSpawner {
    fn spawn(&self, work: Box<dyn FnOnce() + Send>) {
        self.0.lock().unwrap().push(work);
    }
}
impl DeferredSpawner {
    fn run_all(&self) {
        let jobs: Vec<_> = self.0.lock().unwrap().drain(..).collect();
        for job in jobs {
            job();
        }
    }
}

struct FakePermissions(Mutex<HashMap<(Permission, PermissionProcedure), PermissionProbe>>);
impl FakePermissions {
    fn new() -> Arc<Self> {
        Arc::new(Self(Mutex::new(HashMap::new())))
    }
    fn set(
        &self,
        permission: Permission,
        procedure: PermissionProcedure,
        status: PermissionStatus,
    ) {
        self.0.lock().unwrap().insert(
            (permission, procedure),
            PermissionProbe {
                status,
                error: None,
            },
        );
    }
}
impl PermissionPort for FakePermissions {
    fn check(&self, permission: Permission, procedure: PermissionProcedure) -> PermissionProbe {
        self.0
            .lock()
            .unwrap()
            .get(&(permission, procedure))
            .cloned()
            .unwrap_or(PermissionProbe {
                status: PermissionStatus::Failed,
                error: Some("未配置的探测".into()),
            })
    }
    fn open_system_settings(&self, _permission: Permission) -> Result<(), String> {
        Ok(())
    }
}

struct FakeContext {
    raw: Mutex<RawContext>,
    pick: Mutex<DirectoryPick>,
}

#[test]
fn picker_survives_missing_file_manager_but_yields_to_a_real_context() {
    struct ExplorerPort(FakeContext);
    impl ContextPort for ExplorerPort {
        fn source(&self) -> ContextSource {
            if cfg!(target_os = "linux") {
                ContextSource::Finder
            } else {
                ContextSource::Explorer
            }
        }
        fn capture(&self) -> RawContext {
            self.0.capture()
        }
        fn pick_directory(&self) -> DirectoryPick {
            self.0.pick_directory()
        }
    }
    let port = Arc::new(ExplorerPort(FakeContext {
        raw: Mutex::new(RawContext {
            unavailable: Some(fleqi_domain::context::ContextAvailability::NoDirectory {
                reason: "no window".into(),
            }),
            ..RawContext::default()
        }),
        pick: Mutex::new(DirectoryPick::Selected(fleqi_application::ports::RawPath {
            native: std::env::temp_dir(),
            kind: fleqi_domain::context::PathKind::Directory,
        })),
    }));
    let context = ContextService::new(
        port.clone(),
        Arc::new(FakeClock),
        Arc::new(Events::default()),
        Arc::new(PathRegistry::new()),
    );
    context.pick_directory();
    let picked = context.latest().unwrap();
    assert_eq!(context.refresh(), picked);
    *port.0.raw.lock().unwrap() = RawContext {
        directory: Some(fleqi_application::ports::RawPath {
            native: std::env::current_dir().unwrap(),
            kind: fleqi_domain::context::PathKind::Directory,
        }),
        ..RawContext::default()
    };
    let captured = context.refresh();
    assert_eq!(captured.source, port.source());
    assert_ne!(captured.id, picked.id);
}
impl ContextPort for FakeContext {
    fn capture(&self) -> RawContext {
        self.raw.lock().unwrap().clone()
    }
    fn pick_directory(&self) -> DirectoryPick {
        self.pick.lock().unwrap().clone()
    }
}

fn ready_lifecycle(events: &Arc<Events>) -> Arc<HostLifecycle> {
    let lifecycle = Arc::new(HostLifecycle::new(events.clone()));
    lifecycle.transition(HostState::Ready).unwrap();
    lifecycle
}

fn settings_service(
    store: Arc<MemoryStore>,
    events: Arc<Events>,
    lifecycle: Arc<HostLifecycle>,
) -> (SettingsService, StorageState) {
    let (service, state, _) = SettingsService::load(
        store,
        Arc::new(FakeClock),
        events,
        lifecycle,
        FieldAllowlist::M1,
    );
    (service, state)
}

fn request(id: &str, revision: u64, json: &str) -> SettingsUpdateRequest {
    SettingsUpdateRequest {
        request_id: id.into(),
        expected_revision: Revision::new(revision),
        patch: serde_json::from_str::<SettingsPatch>(json).unwrap(),
    }
}

#[test]
fn update_commits_bumps_revision_and_broadcasts_after_commit() {
    let store = Arc::new(MemoryStore::default());
    let events = Arc::new(Events::default());
    let (service, state) =
        settings_service(store.clone(), events.clone(), ready_lifecycle(&events));
    assert_eq!(state, StorageState::Ready);
    assert_eq!(service.snapshot().revision, Revision::new(0));

    let snapshot = service
        .update(request("r1", 0, r#"{"theme":"light"}"#))
        .unwrap();
    assert_eq!(snapshot.revision, Revision::new(1));
    assert_eq!(snapshot.settings.theme, Theme::Light);
    assert!(snapshot.persisted);
    assert_eq!(store.commits.load(Ordering::SeqCst), 1);
    assert_eq!(events.count("settings:changed"), 1);
    assert_eq!(
        store.load().unwrap().unwrap().settings.theme,
        Theme::Light,
        "真实持久化"
    );
}

#[test]
fn same_request_id_replays_without_second_commit_and_conflicts_on_new_payload() {
    let store = Arc::new(MemoryStore::default());
    let events = Arc::new(Events::default());
    let (service, _) = settings_service(store.clone(), events.clone(), ready_lifecycle(&events));

    let first = service
        .update(request("r1", 0, r#"{"theme":"light"}"#))
        .unwrap();
    let replay = service
        .update(request("r1", 0, r#"{"theme":"light"}"#))
        .unwrap();
    assert_eq!(first, replay);
    assert_eq!(
        store.commits.load(Ordering::SeqCst),
        1,
        "重复 requestId 不重复执行"
    );

    let conflict = service
        .update(request("r1", 0, r#"{"theme":"system"}"#))
        .unwrap_err();
    assert_eq!(conflict.code, ErrorCode::Conflict);
    assert_eq!(conflict.current_revision, Some(Revision::new(1)));
}

#[test]
fn stale_expected_revision_is_conflict_with_current_revision() {
    let store = Arc::new(MemoryStore::default());
    let events = Arc::new(Events::default());
    let (service, _) = settings_service(store, events.clone(), ready_lifecycle(&events));
    service
        .update(request("a", 0, r#"{"theme":"light"}"#))
        .unwrap();
    let error = service
        .update(request("b", 0, r#"{"transparency":false}"#))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Conflict);
    assert_eq!(error.current_revision, Some(Revision::new(1)));
    assert!(service.snapshot().settings.transparency, "冲突不应用");
}

#[test]
fn m1_blocked_field_returns_validation_with_field_errors() {
    let store = Arc::new(MemoryStore::default());
    let events = Arc::new(Events::default());
    let (service, _) = settings_service(store.clone(), events.clone(), ready_lifecycle(&events));
    let error = service
        .update(request("a", 0, r#"{"activation":"followFinder"}"#))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Validation);
    let fields = error.field_errors.unwrap();
    assert_eq!(fields[0].field, "activation");
    assert_eq!(fields[0].code, "notAvailable");
    assert_eq!(store.commits.load(Ordering::SeqCst), 0);
    assert_eq!(events.count("settings:changed"), 0);
}

#[test]
fn degraded_storage_keeps_unpersisted_defaults_and_rejects_updates() {
    let store = Arc::new(MemoryStore {
        fail_load: true,
        ..MemoryStore::default()
    });
    let events = Arc::new(Events::default());
    let (service, state) = settings_service(store, events.clone(), ready_lifecycle(&events));
    assert_eq!(state, StorageState::Degraded);
    let snapshot = service.snapshot();
    assert!(!snapshot.persisted);
    assert_eq!(snapshot.settings, Settings::default());
    let error = service
        .update(request("a", 0, r#"{"theme":"light"}"#))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Storage);
    assert_eq!(service.snapshot().settings.theme, Theme::Dark, "保留真实值");
}

#[test]
fn stopping_host_rejects_changes() {
    let store = Arc::new(MemoryStore::default());
    let events = Arc::new(Events::default());
    let lifecycle = ready_lifecycle(&events);
    let (service, _) = settings_service(store, events.clone(), lifecycle.clone());
    lifecycle.transition(HostState::Stopping).unwrap();
    let error = service
        .update(request("a", 0, r#"{"theme":"light"}"#))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Unavailable);
}

#[test]
fn concurrent_duplicates_merge_into_single_commit() {
    let store = Arc::new(MemoryStore {
        commit_delay_ms: 80,
        ..MemoryStore::default()
    });
    let events = Arc::new(Events::default());
    let (service, _) = settings_service(store.clone(), events.clone(), ready_lifecycle(&events));
    let service = Arc::new(service);
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let service = Arc::clone(&service);
            std::thread::spawn(move || service.update(request("dup", 0, r#"{"theme":"light"}"#)))
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert!(results.iter().all(|r| r.is_ok()), "{results:?}");
    assert!(
        results
            .windows(2)
            .all(|w| w[0].as_ref().unwrap() == w[1].as_ref().unwrap())
    );
    assert_eq!(store.commits.load(Ordering::SeqCst), 1);
    assert_eq!(events.count("settings:changed"), 1);
}

#[test]
fn permission_passive_check_and_explicit_request_update_records() {
    let events = Arc::new(Events::default());
    let lifecycle = ready_lifecycle(&events);
    let port = FakePermissions::new();
    port.set(
        Permission::FinderAutomation,
        PermissionProcedure::Passive,
        PermissionStatus::NeedsConsent,
    );
    port.set(
        Permission::Accessibility,
        PermissionProcedure::Passive,
        PermissionStatus::Allowed,
    );
    port.set(
        Permission::FinderAutomation,
        PermissionProcedure::Explicit,
        PermissionStatus::Allowed,
    );
    let service = PermissionService::new(
        port.clone(),
        Arc::new(FakeClock),
        events.clone(),
        lifecycle.clone(),
        Arc::new(InlineSpawner),
    );

    assert!(
        service
            .snapshot()
            .records
            .iter()
            .all(|r| r.status == PermissionStatus::Unknown)
    );
    let snapshot = service.check_all();
    assert_eq!(snapshot.records[0].status, PermissionStatus::NeedsConsent);
    assert_eq!(snapshot.records[1].status, PermissionStatus::Allowed);
    assert_eq!(snapshot.revision, Revision::new(2));

    let op = service.request(Permission::FinderAutomation).unwrap();
    assert!(op.operation_id.starts_with("perm-op-"));
    let after = service.snapshot();
    let finder = after
        .records
        .iter()
        .find(|r| r.permission == Permission::FinderAutomation)
        .unwrap();
    assert_eq!(finder.status, PermissionStatus::Allowed);
    assert_eq!(finder.procedure, PermissionProcedure::Explicit);
    assert!(service.inflight(Permission::FinderAutomation).is_none());

    port.set(
        Permission::FinderAutomation,
        PermissionProcedure::Passive,
        PermissionStatus::Denied,
    );
    let revoked = service.check(Permission::FinderAutomation);
    assert!(revoked.revoked, "先前允许后被拒绝 → 撤销");
}

#[test]
fn explicit_request_is_single_inflight_and_late_result_is_dropped_after_stopping() {
    let events = Arc::new(Events::default());
    let lifecycle = ready_lifecycle(&events);
    let port = FakePermissions::new();
    port.set(
        Permission::Accessibility,
        PermissionProcedure::Explicit,
        PermissionStatus::Allowed,
    );
    let spawner = Arc::new(DeferredSpawner(Mutex::new(Vec::new())));
    let service = PermissionService::new(
        port,
        Arc::new(FakeClock),
        events.clone(),
        lifecycle.clone(),
        spawner.clone(),
    );

    let first = service.request(Permission::Accessibility).unwrap();
    let second = service.request(Permission::Accessibility).unwrap();
    assert_eq!(
        first.operation_id, second.operation_id,
        "同一权限只有一个在途申请"
    );
    assert!(service.inflight(Permission::Accessibility).is_some());

    lifecycle.transition(HostState::Stopping).unwrap();
    spawner.run_all();
    let record = service
        .snapshot()
        .records
        .into_iter()
        .find(|r| r.permission == Permission::Accessibility)
        .unwrap();
    assert_eq!(
        record.status,
        PermissionStatus::Unknown,
        "新代际后迟到结果不落地"
    );
    assert!(service.inflight(Permission::Accessibility).is_none());
    assert_eq!(events.count("permissions:changed"), 0);
}

#[test]
fn context_refresh_builds_snapshot_and_picker_cancel_keeps_latest() {
    let events = Arc::new(Events::default());
    let port = Arc::new(FakeContext {
        raw: Mutex::new(RawContext {
            source_window_id: Some(11),
            directory: Some(RawPath {
                native: PathBuf::from("/Users/me/含 空格/目录"),
                kind: PathKind::Directory,
            }),
            selection: vec![RawPath {
                native: PathBuf::from("/Users/me/含 空格/目录/a b.txt"),
                kind: PathKind::File,
            }],
            view_kind: Some(ViewKind::Physical),
            unavailable: None,
        }),
        pick: Mutex::new(DirectoryPick::Cancelled),
    });
    let paths = Arc::new(PathRegistry::new());
    let service = ContextService::new(
        port.clone(),
        Arc::new(FakeClock),
        events.clone(),
        paths.clone(),
    );

    let snapshot = service.refresh();
    assert!(snapshot.id.starts_with("ctx-"));
    assert_eq!(snapshot.availability, ContextAvailability::Available);
    assert_eq!(snapshot.source_window_id, Some(11));
    let dir = snapshot.directory_ref.clone().unwrap();
    assert_eq!(
        paths.resolve(&dir.id),
        Some(PathBuf::from("/Users/me/含 空格/目录")),
        "ID 绑定原始路径"
    );
    assert_eq!(service.get(Some(&snapshot.id)).unwrap(), snapshot);
    assert_eq!(
        service.get(Some("ctx-99")).unwrap_err().code,
        ErrorCode::NotFound
    );

    assert_eq!(service.pick_directory(), DirectoryPickResult::Cancelled);
    assert_eq!(service.latest().unwrap().id, snapshot.id, "取消不改快照");

    *port.pick.lock().unwrap() = DirectoryPick::Selected(RawPath {
        native: PathBuf::from("/tmp/picked"),
        kind: PathKind::Directory,
    });
    let previous_id = snapshot.id;
    match service.pick_directory() {
        DirectoryPickResult::Selected { snapshot } => {
            assert_eq!(snapshot.source, ContextSource::Picker);
            assert_ne!(snapshot.id, previous_id);
            assert_eq!(snapshot.availability, ContextAvailability::Available);
        }
        other => panic!("{other:?}"),
    }

    *port.raw.lock().unwrap() = RawContext {
        unavailable: Some(ContextAvailability::PermissionRequired),
        ..RawContext::default()
    };
    let unavailable = service.refresh();
    assert_eq!(
        unavailable.availability,
        ContextAvailability::PermissionRequired
    );
    assert_eq!(events.count("context:changed"), 3);
}

#[test]
fn context_rejects_changed_selection_and_unchanged_polling_preserves_displayed_inputs() {
    let events = Arc::new(Events::default());
    let port = Arc::new(FakeContext {
        raw: Mutex::new(RawContext {
            source_window_id: Some(11),
            directory: Some(RawPath {
                native: "/tmp/images".into(),
                kind: PathKind::Directory,
            }),
            selection: vec![RawPath {
                native: "/tmp/images/a.png".into(),
                kind: PathKind::File,
            }],
            view_kind: Some(ViewKind::Physical),
            unavailable: None,
        }),
        pick: Mutex::new(DirectoryPick::Cancelled),
    });
    let service = ContextService::new(
        port.clone(),
        Arc::new(FakeClock),
        events.clone(),
        Arc::new(PathRegistry::new()),
    );
    let displayed = service.refresh();
    for _ in 0..100 {
        assert_eq!(service.refresh().id, displayed.id);
    }
    assert_eq!(
        events.count("context:changed"),
        1,
        "unchanged polls must not reload windows or expire a form"
    );
    assert_eq!(service.validate_current(&displayed.id).unwrap(), displayed);
    port.raw.lock().unwrap().selection = vec![RawPath {
        native: "/tmp/images/b.png".into(),
        kind: PathKind::File,
    }];
    assert_eq!(
        service.validate_current(&displayed.id).unwrap_err().code,
        ErrorCode::Conflict
    );
    assert_eq!(
        service.get(Some(&displayed.id)).unwrap().selected_items[0].display_path,
        "/tmp/images/a.png"
    );
    let updated = service.latest().unwrap();
    assert_eq!(updated.selected_items[0].display_path, "/tmp/images/b.png");
    assert_eq!(service.validate_current(&updated.id).unwrap(), updated);
    port.raw.lock().unwrap().unavailable = Some(ContextAvailability::PermissionRequired);
    assert_eq!(
        service.validate_current(&updated.id).unwrap_err().code,
        ErrorCode::Unavailable
    );
}
