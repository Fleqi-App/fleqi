//! 应用层端口：由 adapters/platform 实现并注入；应用层不通过全局宿主对象取得能力。

use fleqi_domain::context::{ContextAvailability, PathKind, ViewKind};
use fleqi_domain::idempotency::Receipt;
use fleqi_domain::permissions::{Permission, PermissionProcedure, PermissionStatus};
use fleqi_domain::revision::Revision;
use fleqi_domain::settings::Settings;
use std::path::PathBuf;
use thiserror::Error;

use crate::dto::AppEvent;

pub trait Clock: Send + Sync {
    /// UTC RFC 3339。
    fn now_rfc3339(&self) -> String;
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum StorageError {
    #[error("存储不可用：{0}")]
    Unavailable(String),
    #[error("持久版本与期望不一致")]
    RevisionMismatch { current: Revision },
    #[error("数据损坏：{0}")]
    Corrupt(String),
    #[error("IO 错误：{0}")]
    Io(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PersistedSettings {
    pub settings: Settings,
    pub revision: Revision,
}

/// 设置与请求回执在同一事务提交（architecture.md §12.3）。
pub trait SettingsStore: Send + Sync {
    fn load(&self) -> Result<Option<PersistedSettings>, StorageError>;
    /// `expected` 为 Some 时必须等于当前持久 revision；成功返回新 revision。
    fn commit(
        &self,
        settings: &Settings,
        expected: Option<Revision>,
        receipt: &Receipt,
    ) -> Result<Revision, StorageError>;
    fn find_receipt(&self, request_id: &str) -> Result<Option<Receipt>, StorageError>;
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CredentialError {
    #[error("凭据服务不可用：{0}")]
    Unavailable(String),
    #[error("凭据项不存在")]
    NotFound,
    #[error("凭据操作失败：{0}")]
    Failed(String),
}

/// 系统凭据端口：仅 Fleqi 自有命名空间；不提供前端读回秘密的 IPC。
/// `load` 只供 Rust 侧把密钥直发用户配置的端点（architecture.md §9.1），
/// 不进入任何序列化快照或前端可见载荷。
pub trait CredentialPort: Send + Sync {
    fn namespace(&self) -> &str;
    fn store(&self, key: &str, secret: &[u8]) -> Result<(), CredentialError>;
    fn replace(&self, key: &str, secret: &[u8]) -> Result<(), CredentialError>;
    fn exists(&self, key: &str) -> Result<bool, CredentialError>;
    fn delete(&self, key: &str) -> Result<(), CredentialError>;
    /// Rust 侧读回（转发给用户端点用）；缺失返回 CredentialError::NotFound。
    fn load(&self, key: &str) -> Result<Vec<u8>, CredentialError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionProbe {
    pub status: PermissionStatus,
    pub error: Option<String>,
}

/// 权限端口：阻塞调用由应用层放入专用线程；同一权限至多一个显式申请在途。
pub trait PermissionPort: Send + Sync {
    fn check(&self, permission: Permission, procedure: PermissionProcedure) -> PermissionProbe;
    fn open_system_settings(&self, permission: Permission) -> Result<(), String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawPath {
    pub native: PathBuf,
    pub kind: PathKind,
}

/// 平台读取到的原始上下文；应用层据此生成不可变 ContextSnapshot。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RawContext {
    pub source_window_id: Option<u64>,
    pub directory: Option<RawPath>,
    pub selection: Vec<RawPath>,
    pub view_kind: Option<ViewKind>,
    /// 平台已判定的不可用原因（权限、Finder 未运行、读取失败）。
    pub unavailable: Option<ContextAvailability>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectoryPick {
    Selected(RawPath),
    Cancelled,
    Failed(String),
}

pub trait ContextPort: Send + Sync {
    fn source(&self) -> fleqi_domain::context::ContextSource {
        fleqi_domain::context::ContextSource::Finder
    }
    fn capture(&self) -> RawContext;
    fn pick_directory(&self) -> DirectoryPick;
}

pub trait EventSink: Send + Sync {
    fn emit(&self, event: AppEvent);
}

/// 把阻塞工作放到专用线程（生产：std::thread；测试可同步执行）。
pub trait Spawner: Send + Sync {
    fn spawn(&self, work: Box<dyn FnOnce() + Send>);
}

pub struct ThreadSpawner;

impl Spawner for ThreadSpawner {
    fn spawn(&self, work: Box<dyn FnOnce() + Send>) {
        std::thread::spawn(work);
    }
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now_rfc3339(&self) -> String {
        time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
    }
}

// ---------- M2：会话持久化 ----------

use fleqi_domain::session::{ConversationEntry, Session};

/// 会话与对话记录持久化；活跃进程句柄、输入租约只在内存。
pub trait SessionStore: Send + Sync {
    /// Durable request receipt lookup. Implementations without receipts must fail closed.
    fn find_receipt(&self, _request_id: &str) -> Result<Option<Receipt>, StorageError> {
        Err(StorageError::Unavailable(
            "会话存储不支持持久请求回执".into(),
        ))
    }
    /// Commit session writes/deletions and the receipt in one transaction. If the request
    /// already exists, return its receipt without applying any mutations (the service checks
    /// its fingerprint). This also protects concurrent callers across database connections.
    fn commit_request(
        &self,
        _sessions: &[Session],
        _deleted_ids: &[String],
        _receipt: &Receipt,
    ) -> Result<Receipt, StorageError> {
        Err(StorageError::Unavailable(
            "会话存储不支持原子请求提交".into(),
        ))
    }
    fn load_all(&self) -> Result<Vec<Session>, StorageError>;
    fn upsert(&self, session: &Session) -> Result<(), StorageError>;
    /// 删除会话及其对话记录（事务）。
    fn delete(&self, session_id: &str) -> Result<(), StorageError>;
    fn append_entry(&self, entry: &ConversationEntry) -> Result<(), StorageError>;
    fn entries(
        &self,
        session_id: &str,
        limit: usize,
        before: Option<&str>,
    ) -> Result<Vec<ConversationEntry>, StorageError>;
}

// ---------- M2：终端 ----------

use fleqi_domain::directory_sync::ShellReadiness;
use fleqi_domain::terminal::TerminalSnapshot;
use std::path::Path;
use std::sync::mpsc::Sender;

/// 终端适配器事件（由 PTY 读取线程产生）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalEvent {
    /// 转发给显示端的字节；`cursor` 为这些字节之后的流序号。
    Output {
        bytes: Vec<u8>,
        cursor: u64,
    },
    PromptReady {
        cwd: String,
    },
    Preexec,
    EditLine {
        len: usize,
    },
    CdResult {
        revision: Revision,
        ok: bool,
        cwd: String,
        message: Option<String>,
    },
    CdCancelled {
        revision: Revision,
    },
    Exited {
        status: Option<i32>,
    },
}

/// 一个持续 PTY 的句柄；实现负责真实 child、串行写入、落盘与回收。
pub trait TerminalHandle: Send + Sync {
    fn terminal_id(&self) -> String;
    fn write_input(&self, bytes: &[u8]) -> Result<(), String>;
    /// 目录控制消息（仅 terminal 模块生成）。
    fn send_cd(&self, target: &Path, revision: Revision) -> Result<(), String>;
    /// 私有通道可撤销尚未执行的目录请求；原有直接写入型端口保留在途回执。
    fn cancel_cd(&self) {}
    fn set_directory_visibility(&self, _visible: bool, _cancel: bool) {}
    fn resize(&self, cols: u16, rows: u16) -> Result<(), String>;
    fn readiness(&self) -> ShellReadiness;
    fn foreground_process(&self) -> Option<String>;
    fn current_directory(&self) -> String;
    fn snapshot(&self) -> TerminalSnapshot;
    fn subscribe_from(&self, cursor: u64, sender: Sender<TerminalEvent>) -> Result<u64, String>;
    fn unsubscribe(&self, subscription: u64) -> Result<(), String>;
    fn has_exited(&self) -> bool;
    fn shutdown(self: Box<Self>);
}

pub trait TerminalPort: Send + Sync {
    fn spawn(
        &self,
        session_id: &str,
        cwd: &Path,
        cols: u16,
        rows: u16,
        events: Sender<TerminalEvent>,
    ) -> Result<Box<dyn TerminalHandle>, String>;
}

/// 稳定 ID 生成（生产：时间 + 计数；测试可固定）。
pub trait IdGenerator: Send + Sync {
    fn next_id(&self, prefix: &str) -> String;
}

pub struct SequenceIds {
    counter: std::sync::atomic::AtomicU64,
    epoch: String,
}

impl SequenceIds {
    pub fn new() -> Self {
        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        Self {
            counter: std::sync::atomic::AtomicU64::new(1),
            epoch: format!("{epoch:x}"),
        }
    }
}

impl Default for SequenceIds {
    fn default() -> Self {
        Self::new()
    }
}

impl IdGenerator for SequenceIds {
    fn next_id(&self, prefix: &str) -> String {
        let n = self
            .counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        format!("{prefix}-{}-{n}", self.epoch)
    }
}

// ---------- M3：一次性进程与 Run 存储 ----------

/// 一次性进程事件（适配器产生；stdout/stderr 带流标识与序号）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessEvent {
    Output {
        stream: ProcessStreamKind,
        bytes: Vec<u8>,
        seq: u64,
    },
    Exited {
        status: Option<i32>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessStreamKind {
    Stdout,
    Stderr,
}

/// 一次性进程端口（AI 计划步骤；手动终端走 TerminalPort）。
pub trait ProcessPort: Send + Sync {
    fn spawn_script(
        &self,
        runtime: fleqi_domain::execution::ScriptRuntime,
        script: &str,
        cwd: &std::path::Path,
        events: Sender<ProcessEvent>,
    ) -> Result<Box<dyn ProcessHandle>, String> {
        use fleqi_domain::execution::ScriptRuntime;
        match runtime {
            ScriptRuntime::PosixSh => {
                self.spawn("/bin/sh", &["-c".into(), script.into()], cwd, &[], events)
            }
            ScriptRuntime::WindowsPowerShell => self.spawn(
                "powershell.exe",
                &[
                    "-NoProfile".into(),
                    "-NonInteractive".into(),
                    "-Command".into(),
                    script.into(),
                ],
                cwd,
                &[],
                events,
            ),
        }
    }
    fn spawn(
        &self,
        executable: &str,
        args: &[String],
        cwd: &std::path::Path,
        env: &[(String, String)],
        events: Sender<ProcessEvent>,
    ) -> Result<Box<dyn ProcessHandle>, String>;
}

pub trait ProcessHandle: Send + Sync {
    fn wait(&self) -> Option<i32>;
    fn cancel(&self);
}

/// 规则/收藏/输入历史持久化。
pub trait CollectionStore: Send + Sync {
    fn load_rules(&self) -> Result<Vec<crate::collection_service::Rule>, StorageError>;
    fn upsert_rule(&self, rule: &crate::collection_service::Rule) -> Result<(), StorageError>;
    fn delete_rule(&self, rule_id: &str) -> Result<(), StorageError>;
    fn load_favorites(&self) -> Result<Vec<crate::collection_service::Favorite>, StorageError>;
    fn upsert_favorite(
        &self,
        favorite: &crate::collection_service::Favorite,
    ) -> Result<(), StorageError>;
    fn delete_favorite(&self, favorite_id: &str) -> Result<(), StorageError>;
    fn history_append(&self, entry: &str) -> Result<(), StorageError>;
    fn history_list(&self) -> Result<Vec<String>, StorageError>;
    fn history_clear(&self) -> Result<(), StorageError>;
}

/// Run 记录持久化（runs 表；输出大块走 append_output 分段）。
pub trait RunStore: Send + Sync {
    fn load_all(&self) -> Result<Vec<crate::run_service::RunRecord>, StorageError>;
    fn upsert(&self, run: &crate::run_service::RunRecord) -> Result<(), StorageError>;
    fn get(&self, run_id: &str) -> Result<Option<crate::run_service::RunRecord>, StorageError>;
    fn list(&self, session_id: &str) -> Result<Vec<crate::run_service::RunRecord>, StorageError>;
    fn append_output(&self, run_id: &str, bytes: &[u8]) -> Result<(), StorageError>;
    fn find_request(
        &self,
        request_id: &str,
    ) -> Result<Option<crate::run_service::RunRecord>, StorageError> {
        Ok(self
            .load_all()?
            .into_iter()
            .find(|run| run.request_id.as_deref() == Some(request_id)))
    }
    fn interrupt_unfinished(&self, now: &str) -> Result<(), StorageError> {
        for mut run in self.load_all()? {
            if !run.state.is_terminal() {
                run.state = fleqi_domain::execution::RunState::Interrupted;
                run.updated_at = now.to_owned();
                self.upsert(&run)?;
            }
        }
        Ok(())
    }
}

/// 已安装工具持久化（installed_tools 表；只有受管安装写入）。
pub trait ToolStore: Send + Sync {
    fn upsert(&self, tool: &fleqi_domain::tools::InstalledTool) -> Result<(), StorageError>;
    fn load_all(&self) -> Result<Vec<fleqi_domain::tools::InstalledTool>, StorageError>;
    fn delete(&self, tool_id: &str) -> Result<(), StorageError>;
}

/// 工具设施端口（FR-TOOLS-001..005）：探测、staging 下载安装与所有权卸载
/// 在适配层实现；取消经 AtomicBool 在下载块间生效。
/// 工具安装进度（FR-TOOLS-002 / AC-COMMON-008）：确定字节量与阶段状态区分。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(tag = "stage", rename_all = "camelCase")]
pub enum InstallProgress {
    /// 下载中：`total = None` 表示来源未声明长度（不确定进度）。
    Download {
        bytes: u64,
        total: Option<u64>,
    },
    Verifying,
    Extracting,
    Publishing,
    Installing {
        message: String,
    },
}

pub trait ToolFacility: Send + Sync {
    fn supports_install(&self) -> bool {
        true
    }
    /// 探测工具真实状态（系统 PATH 或受管安装目录 + 检测参数真实执行）。
    fn detect(
        &self,
        manifest: &fleqi_domain::tools::ToolManifest,
    ) -> fleqi_domain::tools::ToolStatus;
    /// 受管安装：下载到 staging → 校验 → 安全解压 → 预检 → 原子发布；
    /// 失败/取消清理本次 staging，保留已安装版本。返回发布后的真实状态。
    /// `progress` 在阶段边界与下载分块时回调（展示进度用）。
    fn install(
        &self,
        manifest: &fleqi_domain::tools::ToolManifest,
        cancel: &std::sync::atomic::AtomicBool,
        progress: &dyn Fn(InstallProgress),
    ) -> Result<fleqi_domain::tools::ToolStatus, String>;
    /// 卸载：仅应用拥有的目录（校验所有权标记）；系统工具返回错误说明归属。
    fn uninstall(&self, installed: &fleqi_domain::tools::InstalledTool) -> Result<(), String>;
}

/// 模型端点持久化（providers 表；密钥只存凭据服务，记录仅保存配置形态）。
pub trait ProviderStore: Send + Sync {
    fn upsert(
        &self,
        provider: &crate::provider_service::ProviderRecord,
    ) -> Result<(), StorageError>;
    fn load_all(&self) -> Result<Vec<crate::provider_service::ProviderRecord>, StorageError>;
    fn delete(&self, provider_id: &str) -> Result<(), StorageError>;
}

/// 一次模型对话请求（M3.2 规划闭环）：应用层不感知具体端点协议。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelChatRequest {
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
    pub timeout_ms: u64,
    pub system: String,
    pub user: String,
}

/// 模型网关错误（FR-AI/需求：错误密钥、断流、限流、取消、非法响应各有闭环）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelGatewayError {
    Auth { message: String },
    Network { message: String },
    RateLimited { message: String },
    InvalidResponse { message: String },
    Cancelled,
}

/// 模型网关端口：阻塞式完整回复（内部可流式接收）；取消在块间生效。
pub trait ModelGateway: Send + Sync {
    fn complete(
        &self,
        request: &ModelChatRequest,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<String, ModelGatewayError>;
}
