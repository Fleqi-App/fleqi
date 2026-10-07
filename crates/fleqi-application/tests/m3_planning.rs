//! M3.2 规划闭环测试：模型 → 结构化计划 → RunService 提交；错误闭环
//! （未配置端点、认证失败、取消）；非法计划一次修正、两次仍非法回退摘要。

use fleqi_application::planning_service::{PlanOutcome, PlanningService};
use fleqi_application::ports::{
    Clock, CredentialPort, EventSink, IdGenerator, ModelChatRequest, ModelGateway,
    ModelGatewayError, ProcessEvent, ProcessPort, ProcessStreamKind, ProviderStore, SessionStore,
    SettingsStore, StorageError,
};
use fleqi_application::provider_service::{ProviderSaveRequest, ProviderService};
use fleqi_application::run_service::RunService;
use fleqi_domain::revision::Revision;
use fleqi_domain::settings::{AiPolicy, Settings};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

struct FakeClock;
impl Clock for FakeClock {
    fn now_rfc3339(&self) -> String {
        "2026-09-18T00:00:00Z".into()
    }
}

struct NoEvents;
impl EventSink for NoEvents {
    fn emit(&self, _event: fleqi_application::dto::AppEvent) {}
}

struct SeqIds(AtomicU64);
impl IdGenerator for SeqIds {
    fn next_id(&self, prefix: &str) -> String {
        format!("{prefix}-{}", self.0.fetch_add(1, Ordering::SeqCst))
    }
}

#[derive(Default)]
struct MemorySettings(Mutex<Option<Settings>>);
impl SettingsStore for MemorySettings {
    fn load(&self) -> Result<Option<fleqi_application::ports::PersistedSettings>, StorageError> {
        Ok(self.0.lock().unwrap().clone().map(|settings| {
            fleqi_application::ports::PersistedSettings {
                settings,
                revision: Revision::new(1),
            }
        }))
    }
    fn commit(
        &self,
        _settings: &Settings,
        _expected: Option<Revision>,
        _receipt: &fleqi_domain::idempotency::Receipt,
    ) -> Result<Revision, StorageError> {
        Err(StorageError::Unavailable("测试只读".into()))
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
            .filter(|run| run.session_id == session_id)
            .cloned()
            .collect())
    }
    fn append_output(&self, _run_id: &str, _bytes: &[u8]) -> Result<(), StorageError> {
        Ok(())
    }
}

#[derive(Default)]
struct MemoryProviders(Mutex<Vec<fleqi_application::provider_service::ProviderRecord>>);
impl ProviderStore for MemoryProviders {
    fn upsert(
        &self,
        provider: &fleqi_application::provider_service::ProviderRecord,
    ) -> Result<(), StorageError> {
        let mut providers = self.0.lock().unwrap();
        providers.retain(|existing| existing.id != provider.id);
        providers.push(provider.clone());
        Ok(())
    }
    fn load_all(
        &self,
    ) -> Result<Vec<fleqi_application::provider_service::ProviderRecord>, StorageError> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn delete(&self, provider_id: &str) -> Result<(), StorageError> {
        self.0.lock().unwrap().retain(|p| p.id != provider_id);
        Ok(())
    }
}

#[derive(Default)]
struct MemoryCredentials(Mutex<HashMap<String, Vec<u8>>>);
impl CredentialPort for MemoryCredentials {
    fn namespace(&self) -> &str {
        "test"
    }
    fn store(
        &self,
        key: &str,
        secret: &[u8],
    ) -> Result<(), fleqi_application::ports::CredentialError> {
        self.0.lock().unwrap().insert(key.into(), secret.to_vec());
        Ok(())
    }
    fn replace(
        &self,
        key: &str,
        secret: &[u8],
    ) -> Result<(), fleqi_application::ports::CredentialError> {
        self.0.lock().unwrap().insert(key.into(), secret.to_vec());
        Ok(())
    }
    fn exists(&self, key: &str) -> Result<bool, fleqi_application::ports::CredentialError> {
        Ok(self.0.lock().unwrap().contains_key(key))
    }
    fn delete(&self, key: &str) -> Result<(), fleqi_application::ports::CredentialError> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
    fn load(&self, key: &str) -> Result<Vec<u8>, fleqi_application::ports::CredentialError> {
        self.0
            .lock()
            .unwrap()
            .get(key)
            .cloned()
            .ok_or(fleqi_application::ports::CredentialError::NotFound)
    }
}

/// 可编程模型网关：按序弹出脚本化回复，记录每次请求。
struct FakeGateway {
    replies: Mutex<VecDeque<Result<String, ModelGatewayError>>>,
    requests: Mutex<Vec<ModelChatRequest>>,
}

impl FakeGateway {
    fn with(replies: Vec<Result<String, ModelGatewayError>>) -> Arc<Self> {
        Arc::new(Self {
            replies: Mutex::new(replies.into_iter().collect()),
            requests: Mutex::new(Vec::new()),
        })
    }
}

impl ModelGateway for FakeGateway {
    fn complete(
        &self,
        request: &ModelChatRequest,
        _cancel: &AtomicBool,
    ) -> Result<String, ModelGatewayError> {
        self.requests.lock().unwrap().push(request.clone());
        self.replies
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Ok(String::new()))
    }
}

/// 即时完成进程端口：发送固定输出后退出 0。
struct InstantProcess;
impl ProcessPort for InstantProcess {
    fn spawn(
        &self,
        _executable: &str,
        _args: &[String],
        _cwd: &std::path::Path,
        _env: &[(String, String)],
        events: Sender<ProcessEvent>,
    ) -> Result<Box<dyn fleqi_application::ports::ProcessHandle>, String> {
        std::thread::spawn(move || {
            let _ = events.send(ProcessEvent::Output {
                stream: ProcessStreamKind::Stdout,
                bytes: b"ok".to_vec(),
                seq: 1,
            });
            let _ = events.send(ProcessEvent::Exited { status: Some(0) });
        });
        Ok(Box::new(InstantHandle))
    }
}

struct InstantHandle;
impl fleqi_application::ports::ProcessHandle for InstantHandle {
    fn wait(&self) -> Option<i32> {
        Some(0)
    }
    fn cancel(&self) {}
}

struct Fixture {
    planning: Arc<PlanningService>,
    runs: Arc<RunService>,
    store: Arc<MemoryRuns>,
    gateway: Arc<FakeGateway>,
    providers: Arc<ProviderService>,
}

fn fixture(replies: Vec<Result<String, ModelGatewayError>>, policy: AiPolicy) -> Fixture {
    fixture_with(replies, policy, None)
}

fn fixture_with(
    replies: Vec<Result<String, ModelGatewayError>>,
    policy: AiPolicy,
    default_model: Option<&str>,
) -> Fixture {
    let sessions = Arc::new(MemorySessions::default());
    sessions
        .upsert(&fleqi_domain::session::Session {
            id: "session-1".into(),
            parent_session_id: None,
            title: "测试会话".into(),
            state: fleqi_domain::session::SessionState::Active,
            initial_directory: None,
            current_directory: None,
            target_directory: None,
            directory_sync: fleqi_domain::directory_sync::DirectorySync::Synced,
            terminal_id: None,
            pinned: false,
            created_at: "2026-09-18T00:00:00Z".into(),
            last_used_at: "2026-09-18T00:00:00Z".into(),
            ended_at: None,
            revision: Revision::new(1),
        })
        .expect("种子会话");
    let settings_store: Arc<MemorySettings> =
        Arc::new(MemorySettings(Mutex::new(Some(Settings {
            ai_policy: policy,
            default_model: default_model.map(|m| m.to_string()),
            ..Settings::default()
        }))));
    let run_memory = Arc::new(MemoryRuns::default());
    let runs = RunService::new(
        run_memory.clone(),
        sessions,
        settings_store.clone(),
        Arc::new(InstantProcess),
        Arc::new(FakeClock),
        Arc::new(SeqIds(AtomicU64::new(0))),
        Arc::new(NoEvents),
        Arc::new(fleqi_application::paths::PathRegistry::new()),
    );
    let providers = Arc::new(ProviderService::new(
        Arc::new(MemoryProviders::default()),
        Arc::new(MemoryCredentials::default()),
        Arc::new(FakeClock),
        Arc::new(NoEvents),
    ));
    let gateway = FakeGateway::with(replies);
    let planning = Arc::new(PlanningService::new(
        providers.clone(),
        gateway.clone(),
        settings_store,
        runs.clone(),
        Arc::new(FakeClock),
    ));
    Fixture {
        planning,
        runs,
        store: run_memory,
        gateway,
        providers,
    }
}

fn save_provider(providers: &ProviderService, id: &str) {
    providers
        .save(ProviderSaveRequest {
            id: id.into(),
            display_name: "本地端点".into(),
            base_url: "http://127.0.0.1:9/v1".into(),
            models: vec!["m-mini".into()],
            default_generation_model: Some("m-mini".into()),
            summary_model: None,
            timeout_ms: 5_000,
            api_key: Some("test-only".into()),
        })
        .expect("保存端点");
}

const VALID_PLAN: &str =
    r#"{"scripts":["printf planned"],"effects":["read"],"previewComplete":true}"#;

#[test]
fn no_provider_reports_guidance() {
    let fixture = fixture(Vec::new(), AiPolicy::ReadOnlyAutoConfirmChanges);
    let cancel = AtomicBool::new(false);
    let error = fixture
        .planning
        .plan_and_submit(
            "req-1",
            "session-1",
            "ctx-1",
            "列出文件",
            &cancel,
            std::env::temp_dir().into(),
        )
        .unwrap_err();
    assert_eq!(error.code, fleqi_application::ErrorCode::Unavailable);
    assert!(
        error.message.contains("模型端点"),
        "信息：{}",
        error.message
    );
}

#[test]
fn valid_plan_executes_run_under_policy() {
    let fixture = fixture(
        vec![Ok(VALID_PLAN.into())],
        AiPolicy::ReadOnlyAutoConfirmChanges,
    );
    save_provider(&fixture.providers, "p1");
    let cancel = AtomicBool::new(false);
    let outcome = fixture
        .planning
        .plan_and_submit(
            "req-2",
            "session-1",
            "ctx-9",
            "列出文件",
            &cancel,
            std::env::temp_dir().into(),
        )
        .unwrap();
    let PlanOutcome::Execute { run } = outcome else {
        panic!("应提交执行：{outcome:?}")
    };
    assert_eq!(run.session_id, "session-1");
    assert_eq!(run.context_id, "ctx-9");
    if cfg!(windows) {
        assert_eq!(
            run.state,
            fleqi_domain::execution::RunState::AwaitingApproval
        );
        fixture
            .runs
            .approve("approve-windows-plan", &run.id, run.plan_revision)
            .unwrap();
    }
    // 只读 + previewComplete：readOnly 策略自动执行，最终成功。
    let final_run = fixture
        .runs
        .wait_for(&run.id, std::time::Duration::from_secs(5))
        .unwrap_or_else(|| fixture.runs.get(&run.id).expect("Run 存在"));
    assert_eq!(
        final_run.state,
        fleqi_domain::execution::RunState::Succeeded,
        "状态 {:?}，输出：{:?}",
        final_run.state,
        final_run.output
    );
    assert!(final_run.output.contains("ok"));
}

#[test]
fn invalid_plan_gets_one_repair_then_executes() {
    let fixture = fixture(
        vec![Ok("我认为你应该…".into()), Ok(VALID_PLAN.into())],
        AiPolicy::ReadOnlyAutoConfirmChanges,
    );
    save_provider(&fixture.providers, "p1");
    let cancel = AtomicBool::new(false);
    let outcome = fixture
        .planning
        .plan_and_submit(
            "req-3",
            "session-1",
            "ctx-1",
            "整理文件",
            &cancel,
            std::env::temp_dir().into(),
        )
        .unwrap();
    assert!(matches!(outcome, PlanOutcome::Execute { .. }));
    let requests = fixture.gateway.requests.lock().unwrap();
    assert_eq!(requests.len(), 2, "第二次应携带修正提示");
    assert!(requests[1].system.contains("不合规"));
}

#[test]
fn twice_invalid_falls_back_to_summary_without_run() {
    let fixture = fixture(
        vec![Ok("答非所问一".into()), Ok("答非所问二".into())],
        AiPolicy::ReadOnlyAutoConfirmChanges,
    );
    save_provider(&fixture.providers, "p1");
    let cancel = AtomicBool::new(false);
    let outcome = fixture
        .planning
        .plan_and_submit(
            "req-4",
            "session-1",
            "ctx-1",
            "写点什么",
            &cancel,
            std::env::temp_dir().into(),
        )
        .unwrap();
    let PlanOutcome::Summary { text, attempts } = outcome else {
        panic!("两次非法应回退摘要：{outcome:?}")
    };
    assert_eq!(text, "答非所问二");
    assert_eq!(attempts, 2);
    assert!(fixture.store.0.lock().unwrap().is_empty(), "不应产生 Run");
}

#[test]
fn auth_and_network_errors_surface_with_retryable_flag() {
    let auth = fixture(
        vec![Err(ModelGatewayError::Auth {
            message: "401".into(),
        })],
        AiPolicy::ReadOnlyAutoConfirmChanges,
    );
    save_provider(&auth.providers, "p1");
    let cancel = AtomicBool::new(false);
    let error = auth
        .planning
        .plan_and_submit(
            "req-5",
            "session-1",
            "ctx-1",
            "x",
            &cancel,
            std::env::temp_dir().into(),
        )
        .unwrap_err();
    assert_eq!(error.code, fleqi_application::ErrorCode::Unavailable);
    assert!(!error.retryable);
    assert!(error.message.contains("认证失败"));

    let rate = fixture(
        vec![Err(ModelGatewayError::RateLimited {
            message: "429".into(),
        })],
        AiPolicy::ReadOnlyAutoConfirmChanges,
    );
    save_provider(&rate.providers, "p1");
    let error = rate
        .planning
        .plan_and_submit(
            "req-6",
            "session-1",
            "ctx-1",
            "x",
            &cancel,
            std::env::temp_dir().into(),
        )
        .unwrap_err();
    assert!(error.retryable, "限流应可重试");
}

#[test]
fn settings_default_model_overrides_provider_default_when_listed() {
    let fixture = fixture_with(
        vec![Ok(VALID_PLAN.into())],
        AiPolicy::ReadOnlyAutoConfirmChanges,
        Some("m-big"),
    );
    fixture
        .providers
        .save(ProviderSaveRequest {
            id: "p1".into(),
            display_name: "本地端点".into(),
            base_url: "http://127.0.0.1:9/v1".into(),
            models: vec!["m-mini".into(), "m-big".into()],
            default_generation_model: Some("m-mini".into()),
            summary_model: None,
            timeout_ms: 5_000,
            api_key: Some("test-only".into()),
        })
        .expect("保存端点");
    let cancel = AtomicBool::new(false);
    fixture
        .planning
        .plan_and_submit(
            "req-8",
            "session-1",
            "ctx-1",
            "列出文件",
            &cancel,
            std::env::temp_dir().into(),
        )
        .unwrap();
    let requests = fixture.gateway.requests.lock().unwrap();
    assert_eq!(
        requests[0].model, "m-big",
        "settings.defaultModel 优先于端点记录默认"
    );
}

#[test]
fn invalid_default_model_is_rejected_without_using_another_endpoint() {
    let fixture = fixture_with(
        vec![Ok(VALID_PLAN.into())],
        AiPolicy::ReadOnlyAutoConfirmChanges,
        Some("m-ghost"),
    );
    save_provider(&fixture.providers, "p1");
    let cancel = AtomicBool::new(false);
    let error = fixture
        .planning
        .plan_and_submit(
            "req-9",
            "session-1",
            "ctx-1",
            "列出文件",
            &cancel,
            std::env::temp_dir().into(),
        )
        .unwrap_err();
    assert!(error.message.contains("重新选择"));
    let requests = fixture.gateway.requests.lock().unwrap();
    assert!(requests.is_empty());
}

#[test]
fn change_effect_plan_waits_for_approval_under_readonly() {
    let change_plan =
        r#"{"scripts":["touch out.txt"],"effects":["create"],"previewComplete":true}"#;
    let fixture = fixture(
        vec![Ok(change_plan.into())],
        AiPolicy::ReadOnlyAutoConfirmChanges,
    );
    save_provider(&fixture.providers, "p1");
    let cancel = AtomicBool::new(false);
    let outcome = fixture
        .planning
        .plan_and_submit(
            "req-7",
            "session-1",
            "ctx-1",
            "新建文件",
            &cancel,
            std::env::temp_dir().into(),
        )
        .unwrap();
    let PlanOutcome::Execute { run } = outcome else {
        panic!("应提交等待确认：{outcome:?}")
    };
    assert_eq!(
        fixture.runs.get(&run.id).unwrap().state,
        fleqi_domain::execution::RunState::AwaitingApproval
    );
}
