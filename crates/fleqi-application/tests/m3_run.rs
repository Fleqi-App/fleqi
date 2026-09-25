//! M3 Run 编排测试：计划→策略→确认→执行→结果状态机；取消；重试新 Run；
//! 确认绑定 planRevision；输出订阅；持久化与事件。

use fleqi_application::ports::{
    Clock, EventSink, IdGenerator, ProcessEvent, ProcessPort, ProcessStreamKind, SessionStore,
    SettingsStore, StorageError,
};
use fleqi_application::run_service::{RunService, RunSubmit};
use fleqi_domain::execution::{
    Effect, EffectKind, ExecutionPlan, ExecutionStep, PlanPreviewCompleteness, StepKind,
};
use fleqi_domain::revision::Revision;
use fleqi_domain::settings::{AiPolicy, Settings};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

struct FakeClock;
impl Clock for FakeClock {
    fn now_rfc3339(&self) -> String {
        "2026-09-18T00:00:00Z".into()
    }
}

#[derive(Default)]
struct Events(Mutex<Vec<String>>);
impl EventSink for Events {
    fn emit(&self, event: fleqi_application::dto::AppEvent) {
        self.0.lock().unwrap().push(event.name().to_owned());
    }
}
struct SeqIds(AtomicU64);
impl IdGenerator for SeqIds {
    fn next_id(&self, prefix: &str) -> String {
        format!("{prefix}-{}", self.0.fetch_add(1, Ordering::SeqCst))
    }
}

#[derive(Default)]
struct MemorySettings(Mutex<Option<(Settings, Revision)>>);
impl SettingsStore for MemorySettings {
    fn load(&self) -> Result<Option<fleqi_application::ports::PersistedSettings>, StorageError> {
        Ok(self.0.lock().unwrap().clone().map(|(settings, revision)| {
            fleqi_application::ports::PersistedSettings { settings, revision }
        }))
    }
    fn commit(
        &self,
        settings: &Settings,
        expected: Option<Revision>,
        receipt: &fleqi_domain::idempotency::Receipt,
    ) -> Result<Revision, StorageError> {
        let mut guard = self.0.lock().unwrap();
        let current = guard.as_ref().map(|(_, r)| *r).unwrap_or(Revision::new(0));
        if expected != Some(current) {
            return Err(StorageError::RevisionMismatch { current });
        }
        let next = current.next();
        *guard = Some((settings.clone(), next));
        let _ = receipt;
        Ok(next)
    }
    fn find_receipt(
        &self,
        _request_id: &str,
    ) -> Result<Option<fleqi_domain::idempotency::Receipt>, StorageError> {
        Ok(None)
    }
}

#[derive(Default)]
struct MemorySessions(Mutex<HashMap<String, fleqi_domain::session::Session>>);
impl SessionStore for MemorySessions {
    fn load_all(&self) -> Result<Vec<fleqi_domain::session::Session>, StorageError> {
        Ok(self.0.lock().unwrap().values().cloned().collect())
    }
    fn upsert(&self, session: &fleqi_domain::session::Session) -> Result<(), StorageError> {
        self.0
            .lock()
            .unwrap()
            .insert(session.id.clone(), session.clone());
        Ok(())
    }
    fn delete(&self, session_id: &str) -> Result<(), StorageError> {
        self.0.lock().unwrap().remove(session_id);
        Ok(())
    }
    fn append_entry(
        &self,
        _entry: &fleqi_domain::session::ConversationEntry,
    ) -> Result<(), StorageError> {
        Ok(())
    }
    fn entries(
        &self,
        _session_id: &str,
        _limit: usize,
        _before: Option<&str>,
    ) -> Result<Vec<fleqi_domain::session::ConversationEntry>, StorageError> {
        Ok(Vec::new())
    }
}

#[derive(Default)]
struct MemoryRuns(Mutex<HashMap<String, fleqi_application::run_service::RunRecord>>);
impl fleqi_application::ports::RunStore for MemoryRuns {
    fn load_all(&self) -> Result<Vec<fleqi_application::run_service::RunRecord>, StorageError> {
        Ok(self.0.lock().unwrap().values().cloned().collect())
    }
    fn upsert(&self, run: &fleqi_application::run_service::RunRecord) -> Result<(), StorageError> {
        self.0.lock().unwrap().insert(run.id.clone(), run.clone());
        Ok(())
    }
    fn get(
        &self,
        run_id: &str,
    ) -> Result<Option<fleqi_application::run_service::RunRecord>, StorageError> {
        Ok(self.0.lock().unwrap().get(run_id).cloned())
    }
    fn list(
        &self,
        session_id: &str,
    ) -> Result<Vec<fleqi_application::run_service::RunRecord>, StorageError> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .values()
            .filter(|r| r.session_id == session_id)
            .cloned()
            .collect())
    }
    fn append_output(&self, _run_id: &str, _bytes: &[u8]) -> Result<(), StorageError> {
        Ok(())
    }
}

/// 脚本化 ProcessPort：记录请求，可编程输出与退出码。
struct FakeProcess {
    requests: Mutex<Vec<Vec<String>>>,
    pending: Mutex<Option<Sender<ProcessEvent>>>,
    output: Vec<u8>,
    exit: Option<i32>,
    slow: bool,
}

impl ProcessPort for FakeProcess {
    fn spawn(
        &self,
        executable: &str,
        args: &[String],
        _cwd: &std::path::Path,
        _env: &[(String, String)],
        events: Sender<ProcessEvent>,
    ) -> Result<Box<dyn fleqi_application::ports::ProcessHandle>, String> {
        self.requests
            .lock()
            .unwrap()
            .push(vec![executable.to_owned(), args.join(" ")]);
        if self.slow {
            *self.pending.lock().unwrap() = Some(events);
            // 由测试控制退出回执。
            let handle = Box::new(SlowHandle {});
            return Ok(handle);
        }
        let (bytes, exit) = (self.output.clone(), self.exit);
        std::thread::spawn(move || {
            if !bytes.is_empty() {
                let _ = events.send(ProcessEvent::Output {
                    stream: ProcessStreamKind::Stdout,
                    bytes,
                    seq: 1,
                });
            }
            let _ = events.send(ProcessEvent::Exited { status: exit });
        });
        Ok(Box::new(InstantHandle { exit }))
    }
}

struct InstantHandle {
    exit: Option<i32>,
}
impl fleqi_application::ports::ProcessHandle for InstantHandle {
    fn wait(&self) -> Option<i32> {
        self.exit
    }
    fn cancel(&self) {}
}

struct SlowHandle;
impl fleqi_application::ports::ProcessHandle for SlowHandle {
    fn wait(&self) -> Option<i32> {
        std::thread::sleep(std::time::Duration::from_secs(120));
        None
    }
    fn cancel(&self) {}
}

fn plan(effects: Vec<Effect>, script: &str) -> ExecutionPlan {
    ExecutionPlan {
        id: "plan-1".into(),
        revision: Revision::new(1),
        capability_id: None,
        context_id: "ctx-1".into(),
        steps: vec![ExecutionStep {
            kind: StepKind::Script,
            operation: String::new(),
            executable_ref: None,
            script: Some(script.into()),
            args: vec![],
            cwd_ref: None,
            env_refs: vec![],
            input_refs: vec![],
            expected_outputs: vec![],
        }],
        required_tools: vec![],
        effects,
        preview_completeness: PlanPreviewCompleteness::Complete,
        source_fingerprint: "fp".into(),
    }
}

fn read_effect() -> Effect {
    Effect {
        kind: EffectKind::Read,
        source_ref: None,
        destination_ref: None,
        explanation: "查询".into(),
    }
}

fn change_effect() -> Effect {
    Effect {
        kind: EffectKind::Create,
        source_ref: Some("a".into()),
        destination_ref: Some("b".into()),
        explanation: "生成".into(),
    }
}

fn make_service<P: ProcessPort + 'static>(
    policy: AiPolicy,
    process: Arc<P>,
) -> (Arc<RunService>, Arc<Events>) {
    let events = Arc::new(Events::default());
    let clock: Arc<dyn Clock> = Arc::new(FakeClock);
    let ids: Arc<dyn IdGenerator> = Arc::new(SeqIds(AtomicU64::new(1)));
    let settings_store: Arc<dyn SettingsStore> = Arc::new(MemorySettings::default());
    let sessions: Arc<dyn SessionStore> = Arc::new(MemorySessions::default());
    sessions
        .upsert(&fleqi_domain::session::Session {
            id: "s1".into(),
            parent_session_id: None,
            title: "测试会话".into(),
            state: fleqi_domain::session::SessionState::Active,
            initial_directory: Some("/tmp".into()),
            current_directory: Some("/tmp".into()),
            target_directory: None,
            directory_sync: fleqi_domain::directory_sync::DirectorySync::Synced,
            terminal_id: None,
            pinned: false,
            created_at: "2026-09-18T00:00:00Z".into(),
            last_used_at: "2026-09-18T00:00:00Z".into(),
            ended_at: None,
            revision: Revision::new(1),
        })
        .unwrap();
    let runs: Arc<dyn fleqi_application::ports::RunStore> = Arc::new(MemoryRuns::default());
    let settings = Settings {
        ai_policy: policy,
        ..Settings::default()
    };
    settings_store
        .commit(
            &settings,
            Some(Revision::new(0)),
            &fleqi_domain::idempotency::Receipt {
                request_id: "init".into(),
                fingerprint: "fp".into(),
                result_json: "{}".into(),
            },
        )
        .unwrap();
    let process_port: Arc<dyn ProcessPort> = process;
    let service = RunService::new(
        runs,
        sessions,
        settings_store,
        process_port,
        clock,
        ids,
        events.clone(),
        Arc::new(fleqi_application::paths::PathRegistry::new()),
    );
    (service, events)
}

#[test]
fn read_only_plan_auto_executes_and_succeeds() {
    let process = Arc::new(FakeProcess {
        requests: Mutex::new(Vec::new()),
        pending: Mutex::new(None),
        output: "结果行\n".as_bytes().to_vec(),
        exit: Some(0),
        slow: false,
    });
    let (service, events) =
        make_service(AiPolicy::ReadOnlyAutoConfirmChanges, Arc::clone(&process));
    let submit = RunSubmit {
        working_directory: std::env::temp_dir(),
        session_id: "s1".into(),
        prompt: "查看".into(),
        plan: plan(vec![read_effect()], "ls -la"),
        policy: AiPolicy::ReadOnlyAutoConfirmChanges,
    };
    let record = service.submit("r1", submit).expect("提交");
    assert_eq!(
        record.state,
        fleqi_domain::execution::RunState::Running,
        "纯只读自动执行"
    );
    let finished = service
        .wait_for(&record.id, std::time::Duration::from_secs(5))
        .expect("完成");
    assert_eq!(finished.state, fleqi_domain::execution::RunState::Succeeded);
    assert!(finished.output.contains("结果行"));
    assert!(
        process.requests.lock().unwrap()[0]
            .join(" ")
            .contains("ls -la")
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while events.0.lock().unwrap().len() < 2 && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(
        events.0.lock().unwrap().len() >= 2,
        "submit and completion emit changes"
    );
}

#[test]
fn default_policy_holds_changes_for_approval_then_executes_on_approve() {
    let process = Arc::new(FakeProcess {
        requests: Mutex::new(Vec::new()),
        pending: Mutex::new(None),
        output: Vec::new(),
        exit: Some(0),
        slow: false,
    });
    let (service, _events) =
        make_service(AiPolicy::ReadOnlyAutoConfirmChanges, Arc::clone(&process));
    let submit = RunSubmit {
        working_directory: std::env::temp_dir(),
        session_id: "s1".into(),
        prompt: "改名".into(),
        plan: plan(vec![read_effect(), change_effect()], "mv a b"),
        policy: AiPolicy::ReadOnlyAutoConfirmChanges,
    };
    let record = service.submit("r2", submit).expect("提交");
    assert_eq!(
        record.state,
        fleqi_domain::execution::RunState::AwaitingApproval,
        "修改等待确认"
    );
    assert!(process.requests.lock().unwrap().is_empty(), "未确认不执行");
    // 过期确认：错误 planRevision 拒绝。
    let stale = service.approve("r2", &record.id, Revision::new(99));
    assert!(stale.is_err(), "旧确认失效");
    let approved = service
        .approve("r2", &record.id, Revision::new(1))
        .expect("确认");
    let finished = service
        .wait_for(&approved.id, std::time::Duration::from_secs(5))
        .expect("完成");
    assert_eq!(finished.state, fleqi_domain::execution::RunState::Succeeded);
    assert_eq!(process.requests.lock().unwrap().len(), 1, "确认后执行一次");
}

#[test]
fn yolo_executes_changes_without_approval() {
    let process = Arc::new(FakeProcess {
        requests: Mutex::new(Vec::new()),
        pending: Mutex::new(None),
        output: Vec::new(),
        exit: Some(0),
        slow: false,
    });
    let (service, _events) = make_service(AiPolicy::Yolo, Arc::clone(&process));
    let submit = RunSubmit {
        working_directory: std::env::temp_dir(),
        session_id: "s1".into(),
        prompt: "安装".into(),
        plan: plan(
            vec![Effect {
                kind: EffectKind::Install,
                source_ref: None,
                destination_ref: None,
                explanation: "安装".into(),
            }],
            "brew install x",
        ),
        policy: AiPolicy::Yolo,
    };
    let record = service.submit("r3", submit).expect("提交");
    assert_eq!(
        record.state,
        fleqi_domain::execution::RunState::Running,
        "yolo 免确认"
    );
    let finished = service
        .wait_for(&record.id, std::time::Duration::from_secs(5))
        .expect("完成");
    assert_eq!(finished.state, fleqi_domain::execution::RunState::Succeeded);
}

#[test]
fn nonzero_exit_marks_failed_with_output_preserved() {
    let process = Arc::new(FakeProcess {
        requests: Mutex::new(Vec::new()),
        pending: Mutex::new(None),
        output: "错误详情\n".as_bytes().to_vec(),
        exit: Some(42),
        slow: false,
    });
    let (service, _events) = make_service(AiPolicy::Yolo, Arc::clone(&process));
    let submit = RunSubmit {
        working_directory: std::env::temp_dir(),
        session_id: "s1".into(),
        prompt: "失败任务".into(),
        plan: plan(vec![read_effect()], "false"),
        policy: AiPolicy::Yolo,
    };
    let record = service.submit("r4", submit).expect("提交");
    let finished = service
        .wait_for(&record.id, std::time::Duration::from_secs(5))
        .expect("完成");
    assert_eq!(finished.state, fleqi_domain::execution::RunState::Failed);
    assert_eq!(finished.exit_status, Some(42));
    assert!(finished.output.contains("错误详情"), "失败前的输出保留");
}

#[test]
fn cancel_running_run_keeps_partial_output() {
    let process = Arc::new(FakeProcess {
        requests: Mutex::new(Vec::new()),
        pending: Mutex::new(None),
        output: Vec::new(),
        exit: None,
        slow: true,
    });
    let (service, _events) = make_service(AiPolicy::Yolo, Arc::clone(&process));
    let submit = RunSubmit {
        working_directory: std::env::temp_dir(),
        session_id: "s1".into(),
        prompt: "长任务".into(),
        plan: plan(
            vec![Effect {
                kind: EffectKind::Unknown,
                source_ref: None,
                destination_ref: None,
                explanation: "未知".into(),
            }],
            "sleep 300",
        ),
        policy: AiPolicy::Yolo,
    };
    let record = service.submit("r5", submit).expect("提交");
    service.append_output(&record.id, "部分输出\n".as_bytes());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while process.pending.lock().unwrap().is_none() && std::time::Instant::now() < deadline {
        std::thread::yield_now();
    }
    service.cancel(&record.id).expect("取消");
    let cancelled = service.get(&record.id).expect("记录");
    assert_eq!(
        cancelled.state,
        fleqi_domain::execution::RunState::Cancelled
    );
    assert!(cancelled.output.contains("部分输出"), "取消保留已产生输出");
    process
        .pending
        .lock()
        .unwrap()
        .take()
        .unwrap()
        .send(ProcessEvent::Exited { status: Some(0) })
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while service.get(&record.id).unwrap().exit_status.is_none()
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let after_exit = service.get(&record.id).unwrap();
    assert_eq!(after_exit.exit_status, Some(0));
    assert_eq!(
        after_exit.state,
        fleqi_domain::execution::RunState::Cancelled
    );
}

#[test]
fn retry_creates_new_run_bound_to_original() {
    let process = Arc::new(FakeProcess {
        requests: Mutex::new(Vec::new()),
        pending: Mutex::new(None),
        output: Vec::new(),
        exit: Some(0),
        slow: false,
    });
    let (service, _events) = make_service(AiPolicy::Yolo, Arc::clone(&process));
    let submit = RunSubmit {
        working_directory: std::env::temp_dir(),
        session_id: "s1".into(),
        prompt: "第一次".into(),
        plan: plan(vec![read_effect()], "echo 1"),
        policy: AiPolicy::Yolo,
    };
    let first = service.submit("r6", submit).expect("提交");
    service
        .wait_for(&first.id, std::time::Duration::from_secs(2))
        .expect("完成");
    let retry = service
        .retry("r7", &first.id, "ctx-new", std::env::temp_dir())
        .expect("重试");
    assert_ne!(retry.id, first.id, "重试是新 Run");
    assert_eq!(
        retry.parent_run_id.as_deref(),
        Some(first.id.as_str()),
        "关联原记录"
    );
    assert_eq!(retry.prompt, "第一次");
}

#[test]
fn concurrent_limit_queues_and_both_complete() {
    let process = Arc::new(FakeProcess {
        requests: Mutex::new(Vec::new()),
        pending: Mutex::new(None),
        output: Vec::new(),
        exit: Some(0),
        slow: false,
    });
    let (service, _events) = make_service(AiPolicy::Yolo, Arc::clone(&process));
    let first = service
        .submit(
            "r8",
            RunSubmit {
                working_directory: std::env::temp_dir(),
                session_id: "s1".into(),
                prompt: "a".into(),
                plan: plan(vec![read_effect()], "echo a"),
                policy: AiPolicy::Yolo,
            },
        )
        .unwrap();
    let second = service
        .submit(
            "r9",
            RunSubmit {
                working_directory: std::env::temp_dir(),
                session_id: "s1".into(),
                prompt: "b".into(),
                plan: plan(vec![read_effect()], "echo b"),
                policy: AiPolicy::Yolo,
            },
        )
        .unwrap();
    let f1 = service
        .wait_for(&first.id, std::time::Duration::from_secs(5))
        .expect("完成1");
    let f2 = service
        .wait_for(&second.id, std::time::Duration::from_secs(5))
        .expect("完成2");
    assert_eq!(f1.state, fleqi_domain::execution::RunState::Succeeded);
    assert_eq!(f2.state, fleqi_domain::execution::RunState::Succeeded);
}

fn submission(script: &str) -> RunSubmit {
    RunSubmit {
        working_directory: std::env::temp_dir(),
        session_id: "s1".into(),
        prompt: script.into(),
        plan: plan(vec![read_effect()], script),
        policy: AiPolicy::Yolo,
    }
}

#[test]
fn all_steps_execute_in_order_and_retry_finishes() {
    let process = Arc::new(FakeProcess {
        requests: Mutex::new(vec![]),
        pending: Mutex::new(None),
        output: vec![],
        exit: Some(0),
        slow: false,
    });
    let (service, _) = make_service(AiPolicy::Yolo, process.clone());
    let mut request = submission("echo first");
    let mut second = request.plan.steps[0].clone();
    second.kind = StepKind::Process;
    second.executable_ref = Some("/bin/echo".into());
    second.script = None;
    second.args = vec!["second with spaces".into()];
    request.plan.steps.push(second);
    let run = service.submit("steps", request).unwrap();
    let finished = service
        .wait_for(&run.id, std::time::Duration::from_secs(2))
        .unwrap();
    assert_eq!(finished.state, fleqi_domain::execution::RunState::Succeeded);
    assert_eq!(finished.step_results.len(), 2);
    let requests = process.requests.lock().unwrap().clone();
    assert!(requests[0][1].contains("first"));
    assert_eq!(requests[1], vec!["/bin/echo", "second with spaces"]);
    let retry = service
        .retry("retry", &run.id, "new-context", std::env::temp_dir())
        .unwrap();
    assert_eq!(retry.context_id, "new-context");
    assert_eq!(retry.parent_run_id.as_deref(), Some(run.id.as_str()));
    assert_eq!(
        service
            .wait_for(&retry.id, std::time::Duration::from_secs(2))
            .unwrap()
            .state,
        fleqi_domain::execution::RunState::Succeeded
    );
}

#[derive(Default)]
struct ControlledProcess {
    senders: Mutex<Vec<Sender<ProcessEvent>>>,
}
impl ProcessPort for ControlledProcess {
    fn spawn(
        &self,
        _exe: &str,
        _args: &[String],
        _cwd: &std::path::Path,
        _env: &[(String, String)],
        sender: Sender<ProcessEvent>,
    ) -> Result<Box<dyn fleqi_application::ports::ProcessHandle>, String> {
        self.senders.lock().unwrap().push(sender);
        Ok(Box::new(InstantHandle { exit: Some(0) }))
    }
}
fn eventually(mut predicate: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while !predicate() {
        assert!(std::time::Instant::now() < deadline, "condition timed out");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn fifth_task_runs_when_capacity_is_released_and_cancelled_queue_does_not_start() {
    let process = Arc::new(ControlledProcess::default());
    let (service, _) = make_service(AiPolicy::Yolo, process.clone());
    let records: Vec<_> = (0..6)
        .map(|i| {
            service
                .submit(&format!("queue-{i}"), submission("echo controlled"))
                .unwrap()
        })
        .collect();
    eventually(|| process.senders.lock().unwrap().len() == 4);
    assert_eq!(
        service.get(&records[4].id).unwrap().state,
        fleqi_domain::execution::RunState::Queued
    );
    service.cancel(&records[5].id).unwrap();
    process.senders.lock().unwrap()[0]
        .send(ProcessEvent::Exited { status: Some(0) })
        .unwrap();
    eventually(|| process.senders.lock().unwrap().len() == 5);
    for sender in process.senders.lock().unwrap().iter().skip(1) {
        sender
            .send(ProcessEvent::Exited { status: Some(0) })
            .unwrap();
    }
    for record in records.iter().take(5) {
        assert_eq!(
            service
                .wait_for(&record.id, std::time::Duration::from_secs(2))
                .unwrap()
                .state,
            fleqi_domain::execution::RunState::Succeeded
        );
    }
    assert_eq!(
        service.get(&records[5].id).unwrap().state,
        fleqi_domain::execution::RunState::Cancelled
    );
    assert_eq!(process.senders.lock().unwrap().len(), 5);
}

struct FailsToSpawn;
impl ProcessPort for FailsToSpawn {
    fn spawn(
        &self,
        _: &str,
        _: &[String],
        _: &std::path::Path,
        _: &[(String, String)],
        _: Sender<ProcessEvent>,
    ) -> Result<Box<dyn fleqi_application::ports::ProcessHandle>, String> {
        Err("spawn rejected".into())
    }
}
#[test]
fn spawn_failure_becomes_failed_and_does_not_leak_capacity() {
    let (service, _) = make_service(AiPolicy::Yolo, Arc::new(FailsToSpawn));
    for index in 0..8 {
        let record = service
            .submit(&format!("fail-{index}"), submission("echo never"))
            .unwrap();
        let result = service
            .wait_for(&record.id, std::time::Duration::from_secs(2))
            .unwrap();
        assert_eq!(result.state, fleqi_domain::execution::RunState::Failed);
        assert!(result.output.contains("spawn rejected"));
    }
}

#[test]
fn duplicate_requests_replay_and_changed_payload_conflicts() {
    let process = Arc::new(FakeProcess {
        requests: Mutex::new(vec![]),
        pending: Mutex::new(None),
        output: vec![],
        exit: Some(0),
        slow: false,
    });
    let (service, _) = make_service(AiPolicy::Yolo, process.clone());
    let first = service.submit("same", submission("echo once")).unwrap();
    let second = service.submit("same", submission("echo once")).unwrap();
    assert_eq!(first.id, second.id);
    assert!(service.submit("same", submission("echo changed")).is_err());
    service
        .wait_for(&first.id, std::time::Duration::from_secs(2))
        .unwrap();
    assert_eq!(process.requests.lock().unwrap().len(), 1);
}

#[test]
fn cancelled_session_rejects_late_model_submission() {
    let process = Arc::new(ControlledProcess::default());
    let (service, _) = make_service(AiPolicy::Yolo, process);
    service.cancel_session("s1").unwrap();
    assert!(
        service
            .submit("late", submission("echo must-not-run"))
            .is_err()
    );
}
