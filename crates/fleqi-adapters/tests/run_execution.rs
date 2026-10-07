//! 使用真实 SQLite、工作目录与进程验证执行/恢复；不以 mock 输出证明文件操作成功。
use fleqi_adapters::{
    process::ProcessRunnerPort,
    storage::{Database, SqliteRunStore, SqliteSessionStore, SqliteSettingsStore},
};
use fleqi_application::{
    paths::PathRegistry,
    ports::{Clock, EventSink, RunStore, SequenceIds, SessionStore},
    run_service::{RunService, RunSubmit},
};
use fleqi_domain::{
    directory_sync::DirectorySync,
    execution::{
        Effect, EffectKind, ExecutionPlan, ExecutionStep, PlanPreviewCompleteness, RunState,
        StepKind,
    },
    revision::Revision,
    session::{Session, SessionState},
    settings::AiPolicy,
};
use std::{sync::Arc, time::Duration};
struct TestClock;
impl Clock for TestClock {
    fn now_rfc3339(&self) -> String {
        "2026-09-20T12:00:00Z".into()
    }
}
struct Events;
impl EventSink for Events {
    fn emit(&self, _: fleqi_application::dto::AppEvent) {}
}
fn fixture() -> (tempfile::TempDir, Arc<Database>, Arc<RunService>) {
    let directory = tempfile::tempdir().unwrap();
    let db = Arc::new(Database::open(directory.path()).unwrap());
    let sessions = Arc::new(SqliteSessionStore::new(db.clone()));
    sessions
        .upsert(&Session {
            id: "session".into(),
            parent_session_id: None,
            title: "execution".into(),
            state: SessionState::Active,
            initial_directory: Some(directory.path().display().to_string()),
            current_directory: Some(directory.path().display().to_string()),
            target_directory: None,
            directory_sync: DirectorySync::Synced,
            terminal_id: None,
            pinned: false,
            created_at: "now".into(),
            last_used_at: "now".into(),
            ended_at: None,
            revision: Revision::new(1),
        })
        .unwrap();
    let service = reopen(db.clone());
    (directory, db, service)
}
fn reopen(db: Arc<Database>) -> Arc<RunService> {
    RunService::new(
        Arc::new(SqliteRunStore::new(db.clone())),
        Arc::new(SqliteSessionStore::new(db.clone())),
        Arc::new(SqliteSettingsStore::new(db)),
        Arc::new(ProcessRunnerPort),
        Arc::new(TestClock),
        Arc::new(SequenceIds::new()),
        Arc::new(Events),
        Arc::new(PathRegistry::new()),
    )
}
fn request(directory: &std::path::Path, scripts: &[&str]) -> RunSubmit {
    RunSubmit {
        working_directory: directory.to_owned(),
        session_id: "session".into(),
        prompt: "real execution".into(),
        policy: AiPolicy::Yolo,
        plan: ExecutionPlan {
            id: "plan".into(),
            revision: Revision::new(1),
            capability_id: None,
            context_id: "context".into(),
            steps: scripts
                .iter()
                .map(|script| ExecutionStep {
                    script_runtime: Some(fleqi_domain::execution::ScriptRuntime::current()),
                    kind: StepKind::Script,
                    operation: String::new(),
                    executable_ref: None,
                    script: Some((*script).into()),
                    args: vec![],
                    cwd_ref: None,
                    env_refs: vec![],
                    input_refs: vec![],
                    expected_outputs: vec![],
                })
                .collect(),
            required_tools: vec![],
            effects: vec![Effect {
                kind: EffectKind::Create,
                source_ref: None,
                destination_ref: None,
                explanation: "test files".into(),
            }],
            preview_completeness: PlanPreviewCompleteness::Complete,
            source_fingerprint: String::new(),
        },
    }
}
#[test]
fn real_multi_step_cwd_partial_failure_and_persistent_plan() {
    let (root, db, service) = fixture();
    let work = root.path().join("非 ASCII with spaces");
    std::fs::create_dir(&work).unwrap();
    let run = service
        .submit(
            "real",
            request(
                &work,
                &[
                    "printf first > first.txt",
                    "cat first.txt > second.txt",
                    "exit 7",
                    "touch should-not-exist",
                ],
            ),
        )
        .unwrap();
    let result = service.wait_for(&run.id, Duration::from_secs(10)).unwrap();
    assert_eq!(result.state, RunState::PartiallySucceeded);
    assert_eq!(result.exit_status, Some(7));
    assert_eq!(
        std::fs::read_to_string(work.join("second.txt")).unwrap(),
        "first"
    );
    assert!(!work.join("should-not-exist").exists());
    assert!(!root.path().join("first.txt").exists());
    assert_eq!(result.step_results.len(), 3);
    let restored = reopen(db);
    restored.restore().unwrap();
    assert_eq!(restored.get(&run.id).unwrap(), result);
    assert_eq!(restored.plan(&run.id).unwrap().steps.len(), 4);
}
#[test]
fn restart_interrupts_unfinished_record_without_executing() {
    let (root, db, service) = fixture();
    let mut pending = request(root.path(), &["touch must-not-replay"]);
    pending.policy = AiPolicy::ReadOnlyAutoConfirmChanges;
    let run = service.submit("pending", pending).unwrap();
    assert_eq!(run.state, RunState::AwaitingApproval);
    drop(service);
    let restored = reopen(db.clone());
    restored.restore().unwrap();
    assert_eq!(restored.get(&run.id).unwrap().state, RunState::Interrupted);
    assert!(!root.path().join("must-not-replay").exists());
    assert_eq!(
        SqliteRunStore::new(db).get(&run.id).unwrap().unwrap().state,
        RunState::Interrupted
    );
}
#[test]
fn cancelling_real_process_prevents_later_step_and_releases_worker() {
    let (root, _, service) = fixture();
    let run = service
        .submit(
            "cancel-real",
            request(
                root.path(),
                &["printf ready; sleep 30", "touch must-not-run"],
            ),
        )
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !service.get(&run.id).unwrap().output.contains("ready") {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    service.cancel(&run.id).unwrap();
    std::thread::sleep(Duration::from_millis(250));
    assert_eq!(service.get(&run.id).unwrap().state, RunState::Cancelled);
    assert!(!root.path().join("must-not-run").exists());
    let next = service
        .submit("after-cancel", request(root.path(), &["printf resumed"]))
        .unwrap();
    assert_eq!(
        service
            .wait_for(&next.id, Duration::from_secs(5))
            .unwrap()
            .state,
        RunState::Succeeded
    );
}

#[test]
fn local_capability_plan_creates_real_utf16_file_without_a_model() {
    let (root, _, service) = fixture();
    service
        .set_native_executor(Arc::new(fleqi_adapters::native_steps::NativeSteps::new(
            Arc::new(PathRegistry::new()),
        )))
        .unwrap();
    let context =
        fleqi_domain::context::ContextSnapshotBuilder::new("ctx", Revision::new(1), "now").build();
    let form = fleqi_application::capability_service::form("CAP-TEXT-002", context).unwrap();
    let parameters = std::collections::BTreeMap::from([
        ("name".into(), "中文输出.txt".into()),
        ("content".into(), "第一行\n第二行".into()),
        ("encoding".into(), "utf-16le".into()),
        ("newline".into(), "crlf".into()),
    ]);
    let submit = fleqi_application::capability_service::plan(
        &form,
        &parameters,
        "session".into(),
        root.path().to_owned(),
        AiPolicy::ReadOnlyAutoConfirmChanges,
    )
    .unwrap();
    let record = service.submit("local-file", submit).unwrap();
    assert_eq!(record.state, RunState::AwaitingApproval);
    assert!(!root.path().join("中文输出.txt").exists());
    service
        .approve("confirm-local", &record.id, record.plan_revision)
        .unwrap();
    let done = service
        .wait_for(&record.id, Duration::from_secs(5))
        .unwrap();
    assert_eq!(done.state, RunState::Succeeded);
    let bytes = std::fs::read(root.path().join("中文输出.txt")).unwrap();
    assert_eq!(&bytes[..2], &[0xff, 0xfe]);
    let (text, encoding) = fleqi_adapters::capabilities::FileCapabilities::new()
        .read_text(&root.path().join("中文输出.txt"), None)
        .unwrap();
    assert_eq!(encoding, "utf-16le");
    assert_eq!(text, "第一行\r\n第二行");
}

#[test]
fn output_preferences_bind_native_paths_and_changed_inputs_block_approval() {
    use fleqi_application::capability_service::{bind_output_locations, form, plan};
    use fleqi_domain::{
        context::{ContextSnapshotBuilder, PathKind},
        settings::OutputLocation,
    };
    let (root, db, _) = fixture();
    let paths = Arc::new(PathRegistry::new());
    let source_dir = root.path().join("源文件");
    let output_dir = root.path().join("自定义输出");
    std::fs::create_dir(&source_dir).unwrap();
    std::fs::create_dir(&output_dir).unwrap();
    let source = fleqi_adapters::capabilities::FileCapabilities::new()
        .generate_test_png(&source_dir, "image.png", 40, 20)
        .unwrap();
    let reference = paths.register(&source, PathKind::File);
    let context = ContextSnapshotBuilder::new("context", Revision::new(1), "now")
        .selection(vec![reference])
        .build();
    let editor = form("CAP-IMAGE-002", context).unwrap();
    let mut submit = plan(
        &editor,
        &Default::default(),
        "session".into(),
        root.path().into(),
        AiPolicy::ReadOnlyAutoConfirmChanges,
    )
    .unwrap();
    bind_output_locations(&mut submit, &OutputLocation::BesideSource, &paths).unwrap();
    assert_eq!(
        paths
            .resolve(submit.plan.steps[0].cwd_ref.as_ref().unwrap())
            .unwrap(),
        source_dir
    );
    bind_output_locations(
        &mut submit,
        &OutputLocation::Directory {
            display_path: output_dir.display().to_string(),
        },
        &paths,
    )
    .unwrap();
    assert_eq!(
        paths
            .resolve(submit.plan.steps[0].cwd_ref.as_ref().unwrap())
            .unwrap(),
        output_dir
    );
    let service = RunService::new(
        Arc::new(SqliteRunStore::new(db.clone())),
        Arc::new(SqliteSessionStore::new(db.clone())),
        Arc::new(SqliteSettingsStore::new(db)),
        Arc::new(ProcessRunnerPort),
        Arc::new(TestClock),
        Arc::new(SequenceIds::new()),
        Arc::new(Events),
        paths.clone(),
    );
    service
        .set_native_executor(Arc::new(fleqi_adapters::native_steps::NativeSteps::new(
            paths,
        )))
        .unwrap();
    let first = service.submit("output-policy", submit.clone()).unwrap();
    service
        .approve("confirm-output-policy", &first.id, first.plan_revision)
        .unwrap();
    assert_eq!(
        service
            .wait_for(&first.id, Duration::from_secs(5))
            .unwrap()
            .state,
        RunState::Succeeded
    );
    assert_eq!(std::fs::read_dir(&output_dir).unwrap().count(), 1);
    assert_eq!(std::fs::read_dir(&source_dir).unwrap().count(), 1);
    let pending = service.submit("input-version", submit).unwrap();
    std::fs::write(&source, b"replaced after preview").unwrap();
    assert!(
        service
            .approve("stale-input", &pending.id, pending.plan_revision)
            .is_err()
    );
    assert_eq!(std::fs::read_dir(&output_dir).unwrap().count(), 1);
}
