//! RunService（M3；architecture.md §7、§8）：AI 任务的提交/确认/取消/重试编排。
//! 确认绑定 runId + planRevision；策略只作用于 AI；并发上限排队；输出持久化与订阅。

use crate::dto::{AppError, AppEvent, AppResult};
use crate::ports::{
    Clock, EventSink, IdGenerator, ProcessEvent, ProcessPort, RunStore, SessionStore, SettingsStore,
};
use fleqi_domain::execution::{ExecutionPlan, ExecutionStep, RunState, StepKind, policy_decision};
use fleqi_domain::revision::Revision;
use fleqi_domain::settings::AiPolicy;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use ts_rs::TS;

/// 在创建重试会话或执行副作用前检查解释器，旧记录仅保留原有 POSIX 语义。
pub fn validate_script_runtime(plan: &ExecutionPlan) -> AppResult<()> {
    use fleqi_domain::execution::ScriptRuntime;
    if plan.steps.iter().any(|step| {
        step.kind == StepKind::Script
            && step.script_runtime.unwrap_or(ScriptRuntime::PosixSh) != ScriptRuntime::current()
    }) {
        Err(AppError::unavailable(
            "脚本未记录当前平台支持的解释器，请重新规划任务",
        ))
    } else {
        Ok(())
    }
}

/// Run 记录（持久化 JSON 载荷；输出大块分段走 append_output）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct RunRecord {
    pub id: String,
    pub session_id: String,
    pub parent_run_id: Option<String>,
    pub origin: RunOriginWire,
    pub prompt: String,
    pub context_id: String,
    pub plan_revision: Revision,
    pub state: RunState,
    pub policy: AiPolicyWire,
    /// 内存输出缓冲（持久化截断由 append_output 分段负责）。
    #[serde(default)]
    pub output: String,
    pub exit_status: Option<i32>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub plan: Option<ExecutionPlan>,
    #[serde(default)]
    pub step_results: Vec<RunStepResult>,
    #[serde(default)]
    pub directory_display: String,
    #[serde(default)]
    pub request_id: Option<String>,
    #[serde(default)]
    pub request_fingerprint: String,
    #[serde(default)]
    pub approval_request_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum RunOriginWire {
    Ai,
    Capability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum AiPolicyWire {
    Yolo,
    ReadOnlyAutoConfirmChanges,
}

impl AiPolicyWire {
    pub fn to_domain(self) -> AiPolicy {
        match self {
            AiPolicyWire::Yolo => AiPolicy::Yolo,
            AiPolicyWire::ReadOnlyAutoConfirmChanges => AiPolicy::ReadOnlyAutoConfirmChanges,
        }
    }
}

/// run_submit 载荷：计划由规划层固化；origin 不可由模型改为 manual。
#[derive(Clone)]
pub struct RunSubmit {
    pub working_directory: PathBuf,
    pub session_id: String,
    pub prompt: String,
    pub plan: ExecutionPlan,
    pub policy: AiPolicy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct RunStepResult {
    pub index: usize,
    pub state: RunState,
    pub exit_status: Option<i32>,
    pub message: String,
}

/// 原生步骤适配器：参数保持结构化；取消由每个可中断操作显式检查。
pub struct NativeOutput {
    pub output: String,
    pub partial: bool,
}

impl From<String> for NativeOutput {
    fn from(output: String) -> Self {
        Self {
            output,
            partial: false,
        }
    }
}

struct StepExit {
    status: Option<i32>,
    partial: bool,
}

pub trait NativeStepPort: Send + Sync {
    fn execute(
        &self,
        step: &ExecutionStep,
        cwd: &std::path::Path,
        cancel: &AtomicBool,
    ) -> Result<NativeOutput, String>;
}

struct RunEntry {
    record: RunRecord,
    directories: Vec<PathBuf>,
    cancel: Arc<AtomicBool>,
    handle: Option<Arc<dyn crate::ports::ProcessHandle>>,
    worker_active: bool,
    input_versions: Vec<(PathBuf, String)>,
}

fn file_version(path: &std::path::Path) -> AppResult<String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| AppError::unavailable(format!("输入已不可访问：{error}")))?;
    #[cfg(unix)]
    let identity = {
        use std::os::unix::fs::MetadataExt;
        (metadata.dev(), metadata.ino())
    };
    #[cfg(not(unix))]
    let identity = (0u64, 0u64);
    Ok(format!(
        "{:?}|{:?}|{}|{:?}",
        identity,
        metadata.modified().ok(),
        metadata.len(),
        metadata.file_type()
    ))
}

fn validate_inputs(versions: &[(PathBuf, String)]) -> AppResult<()> {
    for (path, previous) in versions {
        if &file_version(path)? != previous {
            return Err(AppError::conflict(
                "输入在计划生成后发生变化，请重新生成并确认",
                None,
            ));
        }
    }
    Ok(())
}

const MAX_CONCURRENT: usize = 4;
const OUTPUT_MEMORY_CAP: usize = 8 * 1024 * 1024;

pub struct RunService {
    runs: Arc<dyn RunStore>,
    sessions: Arc<dyn SessionStore>,
    settings: Arc<dyn SettingsStore>,
    process: Arc<dyn ProcessPort>,
    paths: Arc<crate::paths::PathRegistry>,
    native: OnceLock<Arc<dyn NativeStepPort>>,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdGenerator>,
    events: Arc<dyn EventSink>,
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    entries: HashMap<String, RunEntry>,
    ready: VecDeque<String>,
    running: usize,
    blocked_sessions: HashSet<String>,
}

impl RunService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        runs: Arc<dyn RunStore>,
        sessions: Arc<dyn SessionStore>,
        settings: Arc<dyn SettingsStore>,
        process: Arc<dyn ProcessPort>,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdGenerator>,
        events: Arc<dyn EventSink>,
        paths: Arc<crate::paths::PathRegistry>,
    ) -> Arc<Self> {
        Arc::new(Self {
            runs,
            sessions,
            settings,
            process,
            paths,
            native: OnceLock::new(),
            clock,
            ids,
            events,
            inner: Mutex::new(Inner::default()),
        })
    }

    pub fn set_native_executor(&self, native: Arc<dyn NativeStepPort>) -> AppResult<()> {
        self.native
            .set(native)
            .map_err(|_| AppError::conflict("原生执行器已配置", None))
    }

    /// 启动恢复只读取/中断遗留任务，绝不重新启动进程或自动重放写入。
    pub fn restore(&self) -> AppResult<()> {
        self.runs
            .interrupt_unfinished(&self.clock.now_rfc3339())
            .map_err(|e| AppError::storage(e.to_string()))
    }

    fn persist(&self, record: &RunRecord) -> AppResult<()> {
        self.runs
            .upsert(record)
            .map_err(|e| AppError::storage(e.to_string()))
    }

    fn emit(&self, record: &RunRecord) {
        self.events.emit(AppEvent::RunChanged {
            run_id: record.id.clone(),
            revision: record.plan_revision,
        });
    }

    fn prepare(&self, submit: &RunSubmit) -> AppResult<Vec<PathBuf>> {
        validate_script_runtime(&submit.plan)?;
        if submit.plan.steps.is_empty() {
            return Err(AppError::unavailable("执行计划没有步骤"));
        }
        if !submit.working_directory.is_dir() {
            return Err(AppError::unavailable("任务工作目录不存在或不可访问"));
        }
        submit
            .plan
            .steps
            .iter()
            .map(|step| {
                let cwd = match &step.cwd_ref {
                    Some(reference) => self
                        .paths
                        .resolve(reference)
                        .ok_or_else(|| AppError::not_found("步骤目录引用已过期，请重新生成计划"))?,
                    None => submit.working_directory.clone(),
                };
                if !cwd.is_dir() {
                    return Err(AppError::unavailable("步骤目录不存在或不可访问"));
                }
                if !step.env_refs.is_empty() {
                    return Err(AppError::unavailable("计划包含尚未解析的环境凭据引用"));
                }
                match step.kind {
                    StepKind::Script
                        if step.script.as_ref().is_none_or(|s| s.trim().is_empty()) =>
                    {
                        return Err(AppError::unavailable("脚本步骤不能为空"));
                    }
                    StepKind::Process
                        if step
                            .executable_ref
                            .as_ref()
                            .is_none_or(|s| s.trim().is_empty()) =>
                    {
                        return Err(AppError::unavailable("进程步骤缺少可执行文件"));
                    }
                    StepKind::Native if self.native.get().is_none() => {
                        return Err(AppError::unavailable("原生执行器不可用"));
                    }
                    _ => {}
                }
                Ok(cwd)
            })
            .collect()
    }

    pub fn submit(self: &Arc<Self>, request_id: &str, submit: RunSubmit) -> AppResult<RunRecord> {
        self.submit_linked(request_id, submit, None)
    }

    fn submit_linked(
        self: &Arc<Self>,
        request_id: &str,
        submit: RunSubmit,
        parent: Option<String>,
    ) -> AppResult<RunRecord> {
        if request_id.trim().is_empty() {
            return Err(AppError::unavailable("requestId 不能为空"));
        }
        let fingerprint = crate::fingerprint::fingerprint(&(
            &submit.session_id,
            &submit.prompt,
            &submit.plan,
            submit.working_directory.as_os_str().as_encoded_bytes(),
            submit.policy,
            &parent,
        ));
        if let Some(previous) = self
            .runs
            .find_request(request_id)
            .map_err(|e| AppError::storage(e.to_string()))?
        {
            return if previous.request_fingerprint == fingerprint {
                Ok(previous)
            } else {
                Err(AppError::conflict("requestId 已用于不同任务", None))
            };
        }
        let session = self
            .sessions
            .load_all()
            .map_err(|e| AppError::storage(e.to_string()))?
            .into_iter()
            .find(|s| s.id == submit.session_id)
            .ok_or_else(|| AppError::not_found("会话不存在"))?;
        if session.state != fleqi_domain::session::SessionState::Active {
            return Err(AppError::conflict("会话已结束，请从历史继续为新会话", None));
        }
        let directories = self.prepare(&submit)?;
        let input_versions = submit
            .plan
            .steps
            .iter()
            .flat_map(|step| &step.input_refs)
            .map(|reference| {
                let path = self
                    .paths
                    .resolve(reference)
                    .ok_or_else(|| AppError::not_found("输入引用已失效"))?;
                let version = file_version(&path)?;
                Ok((path, version))
            })
            .collect::<AppResult<Vec<_>>>()?;
        let automatic = policy_decision(submit.policy, &submit.plan).auto_execute;
        let now = self.clock.now_rfc3339();
        let record = RunRecord {
            id: self.ids.next_id("run"),
            session_id: submit.session_id.clone(),
            parent_run_id: parent,
            origin: if submit.plan.capability_id.is_some() {
                RunOriginWire::Capability
            } else {
                RunOriginWire::Ai
            },
            prompt: submit.prompt,
            context_id: submit.plan.context_id.clone(),
            plan_revision: submit.plan.revision,
            state: if automatic {
                RunState::Queued
            } else {
                RunState::AwaitingApproval
            },
            policy: match submit.policy {
                AiPolicy::Yolo => AiPolicyWire::Yolo,
                AiPolicy::ReadOnlyAutoConfirmChanges => AiPolicyWire::ReadOnlyAutoConfirmChanges,
            },
            output: String::new(),
            exit_status: None,
            created_at: now.clone(),
            updated_at: now,
            plan: Some(submit.plan),
            step_results: vec![],
            directory_display: submit.working_directory.to_string_lossy().into_owned(),
            request_id: Some(request_id.to_owned()),
            request_fingerprint: fingerprint.clone(),
            approval_request_id: None,
        };
        let mut inner = self.inner.lock().expect("runs");
        if inner.blocked_sessions.contains(&record.session_id) {
            return Err(AppError::conflict("会话正在结束", None));
        }
        if let Some(previous) = self
            .runs
            .find_request(request_id)
            .map_err(|e| AppError::storage(e.to_string()))?
        {
            return if previous.request_fingerprint == fingerprint {
                Ok(previous)
            } else {
                Err(AppError::conflict("requestId 已用于不同任务", None))
            };
        }
        self.persist(&record)?;
        inner.entries.insert(
            record.id.clone(),
            RunEntry {
                record: record.clone(),
                directories,
                cancel: Arc::new(AtomicBool::new(false)),
                handle: None,
                worker_active: false,
                input_versions,
            },
        );
        if automatic {
            inner.ready.push_back(record.id.clone());
        }
        drop(inner);
        self.emit(&record);
        self.pump();
        self.get(&record.id)
    }

    pub fn approve(
        self: &Arc<Self>,
        request_id: &str,
        run_id: &str,
        revision: Revision,
    ) -> AppResult<RunRecord> {
        if request_id.is_empty() {
            return Err(AppError::unavailable("requestId 不能为空"));
        }
        let historical = self.get(run_id)?;
        if historical.state.is_terminal() {
            return if historical.approval_request_id.as_deref() == Some(request_id)
                && historical.plan_revision == revision
            {
                Ok(historical)
            } else {
                Err(AppError::conflict("任务状态不接受确认", None))
            };
        }
        let mut inner = self.inner.lock().expect("runs");
        let entry = inner
            .entries
            .get_mut(run_id)
            .ok_or_else(|| AppError::not_found("任务不存在"))?;
        if entry.record.plan_revision != revision {
            return Err(AppError::conflict(
                "计划版本已改变，请重新查看",
                Some(entry.record.plan_revision),
            ));
        }
        if entry.record.approval_request_id.as_deref() == Some(request_id) {
            return Ok(entry.record.clone());
        }
        if entry.record.state != RunState::AwaitingApproval {
            return Err(AppError::conflict("任务状态不接受确认", None));
        }
        validate_inputs(&entry.input_versions)?;
        let mut record = entry.record.clone();
        record.state = RunState::Queued;
        record.approval_request_id = Some(request_id.to_owned());
        record.updated_at = self.clock.now_rfc3339();
        self.persist(&record)?;
        entry.record = record.clone();
        inner.ready.push_back(run_id.to_owned());
        drop(inner);
        self.emit(&record);
        self.pump();
        self.get(run_id)
    }

    pub fn cancel(&self, run_id: &str) -> AppResult<RunRecord> {
        let previous = self.get(run_id)?;
        if previous.state == RunState::Cancelled {
            return Ok(previous);
        }
        if previous.state.is_terminal() {
            return Err(AppError::conflict("任务已结束", None));
        }
        let mut inner = self.inner.lock().expect("runs");
        let entry = inner
            .entries
            .get_mut(run_id)
            .ok_or_else(|| AppError::not_found("任务不存在"))?;
        if entry.record.state == RunState::Cancelled {
            return Ok(entry.record.clone());
        }
        if entry.record.state.is_terminal() {
            return Err(AppError::conflict("任务已结束", None));
        }
        let mut record = entry.record.clone();
        record.state = RunState::Cancelled;
        record.updated_at = self.clock.now_rfc3339();
        self.persist(&record)?;
        entry.record = record.clone();
        entry.cancel.store(true, Ordering::Release);
        let handle = entry.handle.clone();
        if !entry.worker_active
            && let Some(plan) = &entry.record.plan
        {
            crate::secrets::release_plan(plan);
        }
        drop(inner);
        if let Some(handle) = handle {
            handle.cancel();
        }
        self.emit(&record);
        // 运行容量由工作线程确认退出后释放，取消请求不会提前放行第五个进程。
        Ok(record)
    }

    pub fn cancel_session(&self, session_id: &str) -> AppResult<()> {
        let ids = {
            let mut inner = self.inner.lock().expect("runs");
            inner.blocked_sessions.insert(session_id.to_owned());
            inner
                .entries
                .values()
                .filter(|e| e.record.session_id == session_id && !e.record.state.is_terminal())
                .map(|e| e.record.id.clone())
                .collect::<Vec<_>>()
        };
        for id in ids {
            self.cancel(&id)?;
        }
        Ok(())
    }

    pub fn wait_idle(&self, timeout: std::time::Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if self.inner.lock().expect("runs").running == 0 {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    pub fn get(&self, run_id: &str) -> AppResult<RunRecord> {
        if let Some(entry) = self.inner.lock().expect("runs").entries.get(run_id) {
            return Ok(entry.record.clone());
        }
        self.runs
            .get(run_id)
            .map_err(|e| AppError::storage(e.to_string()))?
            .ok_or_else(|| AppError::not_found("任务不存在"))
    }

    pub fn list(&self, session_id: &str) -> AppResult<Vec<RunRecord>> {
        self.runs
            .list(session_id)
            .map_err(|e| AppError::storage(e.to_string()))
    }

    pub fn plan(&self, run_id: &str) -> AppResult<ExecutionPlan> {
        self.get(run_id)?
            .plan
            .ok_or_else(|| AppError::not_found("历史记录没有保存执行计划，请重新生成"))
    }

    pub fn request_record(
        &self,
        request_id: &str,
        session_id: &str,
        context_id: &str,
        prompt: &str,
    ) -> AppResult<Option<RunRecord>> {
        let Some(record) = self
            .runs
            .find_request(request_id)
            .map_err(|e| AppError::storage(e.to_string()))?
        else {
            return Ok(None);
        };
        if record.session_id != session_id
            || record.context_id != context_id
            || record.prompt != prompt
        {
            return Err(AppError::conflict("requestId 已用于不同任务", None));
        }
        Ok(Some(record.clone()))
    }

    pub fn retry_receipt(
        &self,
        request_id: &str,
        parent_run_id: &str,
    ) -> AppResult<Option<RunRecord>> {
        let previous = self
            .runs
            .find_request(request_id)
            .map_err(|e| AppError::storage(e.to_string()))?;
        if let Some(record) = &previous
            && record.parent_run_id.as_deref() != Some(parent_run_id)
        {
            return Err(AppError::conflict("requestId 已用于其他操作", None));
        }
        Ok(previous)
    }

    /// 重试明确绑定当前上下文与目录，按最新策略重新审批；旧确认不继承。
    pub fn retry(
        self: &Arc<Self>,
        request_id: &str,
        run_id: &str,
        context_id: &str,
        directory: PathBuf,
    ) -> AppResult<RunRecord> {
        self.retry_in_session(request_id, run_id, context_id, directory, None)
    }

    pub fn retry_in_session(
        self: &Arc<Self>,
        request_id: &str,
        run_id: &str,
        context_id: &str,
        directory: PathBuf,
        session_id: Option<String>,
    ) -> AppResult<RunRecord> {
        if let Some(previous) = self.retry_receipt(request_id, run_id)? {
            return Ok(previous);
        }
        let original = self.get(run_id)?;
        if !original.state.is_terminal() {
            return Err(AppError::conflict("请先等待任务结束或取消", None));
        }
        let mut plan = self.plan(run_id)?;
        plan.revision = plan.revision.next();
        plan.context_id = context_id.to_owned();
        // 脚本/argv 任务重新绑定工作目录；能力输入需由能力层重新确认，不能默默替换文件。
        if plan.capability_id.is_some() {
            return Err(AppError::unavailable("请从能力表单确认当前输入后重新提交"));
        }
        for step in &mut plan.steps {
            step.cwd_ref = None;
        }
        let policy = self
            .settings
            .load()
            .map_err(|e| AppError::storage(e.to_string()))?
            .map(|s| s.settings.ai_policy)
            .unwrap_or(AiPolicy::ReadOnlyAutoConfirmChanges);
        self.submit_linked(
            request_id,
            RunSubmit {
                session_id: session_id.unwrap_or(original.session_id),
                prompt: original.prompt,
                plan,
                policy,
                working_directory: directory,
            },
            Some(original.id),
        )
    }

    pub fn append_output(&self, run_id: &str, bytes: &[u8]) {
        let mut inner = self.inner.lock().expect("runs");
        if let Some(entry) = inner.entries.get_mut(run_id) {
            entry
                .record
                .output
                .push_str(&String::from_utf8_lossy(bytes));
            let mut overflow = entry.record.output.len().saturating_sub(OUTPUT_MEMORY_CAP);
            while !entry.record.output.is_char_boundary(overflow) {
                overflow += 1;
            }
            if overflow > 0 {
                entry.record.output.drain(..overflow);
            }
            if let Err(error) = self.runs.append_output(run_id, bytes) {
                entry
                    .record
                    .output
                    .push_str(&format!("\n[输出持久化失败：{error}]\n"));
            }
        }
    }

    pub fn wait_for(&self, run_id: &str, timeout: std::time::Duration) -> Option<RunRecord> {
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            if let Ok(record) = self.get(run_id)
                && record.state.is_terminal()
            {
                return Some(record);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        None
    }

    fn pump(self: &Arc<Self>) {
        loop {
            let next = {
                let mut inner = self.inner.lock().expect("runs");
                if inner.running >= MAX_CONCURRENT {
                    return;
                }
                let Some(id) = inner.ready.pop_front() else {
                    return;
                };
                let Some(entry) = inner.entries.get_mut(&id) else {
                    continue;
                };
                if entry.record.state != RunState::Queued {
                    continue;
                }
                entry.record.state = RunState::Running;
                entry.record.updated_at = self.clock.now_rfc3339();
                if let Err(error) = self.persist(&entry.record) {
                    entry.record.state = RunState::Failed;
                    entry.record.output.push_str(&error.message);
                    let record = entry.record.clone();
                    drop(inner);
                    self.emit(&record);
                    continue;
                }
                entry.worker_active = true;
                let record = entry.record.clone();
                inner.running += 1;
                (id, record)
            };
            self.emit(&next.1);
            let service = Arc::clone(self);
            let id = next.0;
            let worker_id = id.clone();
            if let Err(error) = std::thread::Builder::new()
                .name(format!("fleqi-{id}"))
                .spawn(move || service.execute_run(&worker_id))
            {
                self.finish(&id, RunState::Failed, None, Some(error.to_string()));
            }
        }
    }

    fn execute_run(self: &Arc<Self>, run_id: &str) {
        let inputs = self.inner.lock().expect("runs").entries[run_id]
            .input_versions
            .clone();
        if let Err(error) = validate_inputs(&inputs) {
            self.finish(run_id, RunState::Failed, None, Some(error.message));
            return;
        }
        let (plan, directories, cancel) = {
            let inner = self.inner.lock().expect("runs");
            let entry = &inner.entries[run_id];
            (
                entry.record.plan.clone().expect("submitted plan"),
                entry.directories.clone(),
                entry.cancel.clone(),
            )
        };
        let mut successful = 0;
        let mut partial = false;
        for (index, (step, directory)) in plan.steps.iter().zip(directories.iter()).enumerate() {
            if cancel.load(Ordering::Acquire) {
                self.finish(run_id, RunState::Cancelled, None, None);
                return;
            }
            let outcome = self.execute_step(run_id, step, directory, &cancel);
            let (state, status, message) = match outcome {
                Ok(exit) if cancel.load(Ordering::Acquire) => {
                    (RunState::Cancelled, exit.status, "已取消".to_owned())
                }
                Ok(exit) if exit.partial => (
                    RunState::PartiallySucceeded,
                    exit.status,
                    "部分输入未能处理，详见逐项输出".to_owned(),
                ),
                Ok(StepExit {
                    status: Some(0), ..
                }) => (RunState::Succeeded, Some(0), String::new()),
                Ok(exit) => (RunState::Failed, exit.status, "步骤退出失败".to_owned()),
                Err(error) if cancel.load(Ordering::Acquire) => (RunState::Cancelled, None, error),
                Err(error) => (RunState::Failed, None, error),
            };
            {
                let mut inner = self.inner.lock().expect("runs");
                let entry = inner.entries.get_mut(run_id).expect("run");
                entry.handle = None;
                entry.record.step_results.push(RunStepResult {
                    index,
                    state,
                    exit_status: status,
                    message: message.clone(),
                });
                entry.record.updated_at = self.clock.now_rfc3339();
                let record = entry.record.clone();
                if let Err(error) = self.persist(&record) {
                    drop(inner);
                    self.finish(run_id, RunState::Failed, status, Some(error.message));
                    return;
                }
                drop(inner);
                self.emit(&record);
            }
            if state != RunState::Succeeded {
                if plan.capability_id.is_some() && state != RunState::Cancelled {
                    if state == RunState::PartiallySucceeded {
                        successful += 1;
                        partial = true;
                    }
                    self.append_output(
                        run_id,
                        format!("\n步骤 {} 失败：{message}\n", index + 1).as_bytes(),
                    );
                    continue;
                }
                let final_state = if state == RunState::Cancelled {
                    state
                } else if successful > 0 {
                    RunState::PartiallySucceeded
                } else {
                    RunState::Failed
                };
                self.finish(run_id, final_state, status, Some(message));
                return;
            }
            successful += 1;
        }
        let state = if successful == plan.steps.len() && !partial {
            RunState::Succeeded
        } else if successful > 0 {
            RunState::PartiallySucceeded
        } else {
            RunState::Failed
        };
        self.finish(
            run_id,
            state,
            Some(if state == RunState::Succeeded { 0 } else { 1 }),
            None,
        );
    }

    fn execute_step(
        &self,
        run_id: &str,
        step: &ExecutionStep,
        cwd: &std::path::Path,
        cancel: &AtomicBool,
    ) -> Result<StepExit, String> {
        if !cwd.is_dir() {
            return Err("执行目录已不存在".into());
        }
        if step.kind == StepKind::Native {
            let output = self
                .native
                .get()
                .ok_or("原生执行器不可用")?
                .execute(step, cwd, cancel)?;
            self.append_output(run_id, output.output.as_bytes());
            return Ok(StepExit {
                status: Some(0),
                partial: output.partial,
            });
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        let handle: Arc<dyn crate::ports::ProcessHandle> = Arc::from(match step.kind {
            StepKind::Script => {
                let runtime = step
                    .script_runtime
                    .or_else(|| {
                        (!cfg!(windows)).then_some(fleqi_domain::execution::ScriptRuntime::PosixSh)
                    })
                    .ok_or("旧脚本未记录解释器，请重新规划任务")?;
                if runtime != fleqi_domain::execution::ScriptRuntime::current() {
                    return Err("脚本解释器与当前平台不兼容，请重新规划任务".into());
                }
                self.process.spawn_script(
                    runtime,
                    step.script.as_deref().ok_or("脚本为空")?,
                    cwd,
                    sender,
                )?
            }
            StepKind::Process => self.process.spawn(
                step.executable_ref.as_deref().ok_or("可执行文件为空")?,
                &step.args,
                cwd,
                &[],
                sender,
            )?,
            StepKind::Native => unreachable!(),
        });
        {
            let mut inner = self.inner.lock().expect("runs");
            inner.entries.get_mut(run_id).expect("run").handle = Some(handle.clone());
        }
        if cancel.load(Ordering::Acquire) {
            handle.cancel();
        }
        loop {
            match receiver.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(ProcessEvent::Output { bytes, .. }) => self.append_output(run_id, &bytes),
                Ok(ProcessEvent::Exited { status }) => {
                    return Ok(StepExit {
                        status,
                        partial: false,
                    });
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                    if cancel.load(Ordering::Acquire) =>
                {
                    handle.cancel();
                    return Ok(StepExit {
                        status: None,
                        partial: false,
                    });
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("进程输出通道已关闭，未收到退出状态".into());
                }
            }
        }
    }

    fn finish(
        self: &Arc<Self>,
        id: &str,
        state: RunState,
        status: Option<i32>,
        message: Option<String>,
    ) {
        let mut inner = self.inner.lock().expect("runs");
        let Some(entry) = inner.entries.get_mut(id) else {
            return;
        };
        if entry.record.state != RunState::Cancelled {
            entry.record.state = state;
        }
        entry.record.exit_status = status;
        entry.record.updated_at = self.clock.now_rfc3339();
        if let Some(message) = message {
            entry.record.output.push_str(&format!("\n{message}\n"));
        }
        entry.handle = None;
        let release = std::mem::replace(&mut entry.worker_active, false);
        let mut record = entry.record.clone();
        if let Some(plan) = &record.plan {
            crate::secrets::release_plan(plan);
        }
        let persisted = self.persist(&record);
        if let Err(error) = &persisted {
            record.state = RunState::Failed;
            record
                .output
                .push_str(&format!("\n结果未保存：{}", error.message));
            entry.record = record.clone();
        }
        if persisted.is_ok() {
            inner.entries.remove(id);
        }
        if release {
            inner.running = inner.running.saturating_sub(1);
        }
        drop(inner);
        self.emit(&record);
        self.pump();
    }
}
