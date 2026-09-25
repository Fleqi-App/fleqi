//! 跨 IPC DTO（architecture.md §4、§8、§12）。
//!
//! serde 一律 camelCase；ts-rs 显式导出到 `packages/contracts/src/bindings/`，
//! 只有 `export-contracts` feature 的测试会生成文件（生成与校验分离，普通测试
//! 不改生成产物）。

use fleqi_domain::context::ContextSnapshot;
use fleqi_domain::lifecycle::HostState;
use fleqi_domain::permissions::{Permission, PermissionRecord};
use fleqi_domain::platform::PlatformCapabilities;
use fleqi_domain::revision::Revision;
use fleqi_domain::settings::{FieldError, Settings, SettingsPatch};
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;
use ts_rs::TS;

/// 受校验常量：与发布合同（development-plan.md P0）一致，编译期即固定。
pub const PRODUCT_NAME: &str = "Fleqi";
pub const BUNDLE_IDENTIFIER: &str = "app.fleqi.desktop";
/// 发布渠道标签；版本来自 workspace 元数据。
pub const STAGE: &str = "BETA";
pub const MINIMUM_MACOS_VERSION: &str = "14.0";

/// Update metadata only; download URLs and signatures remain in the host.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateStatus {
    pub phase: AppUpdatePhase,
    pub version: Option<String>,
    pub notes: Option<String>,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum AppUpdatePhase {
    #[default]
    Idle,
    Checking,
    Current,
    Available,
    Downloading,
    Installing,
    Failed,
}

/// 工程构建信息。`app_build_info` 与 `app_bootstrap` 的载荷之一。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct BuildInfo {
    pub product_name: String,
    pub version: String,
    pub bundle_identifier: String,
    pub stage: String,
    pub target_os: String,
    pub target_arch: String,
    pub build_profile: String,
    pub minimum_macos_version: String,
}

impl BuildInfo {
    /// 来自编译目标与 Cargo 元数据；不接收用户输入。
    pub fn current() -> Self {
        Self {
            product_name: PRODUCT_NAME.to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            bundle_identifier: BUNDLE_IDENTIFIER.to_owned(),
            stage: STAGE.to_owned(),
            target_os: std::env::consts::OS.to_owned(),
            target_arch: std::env::consts::ARCH.to_owned(),
            build_profile: if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            }
            .to_owned(),
            minimum_macos_version: MINIMUM_MACOS_VERSION.to_owned(),
        }
    }
}

/// 稳定错误码（architecture.md §8）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// 调用方无权（窗口/来源不符）。
    Forbidden,
    /// expectedRevision 过期或同 requestId 不同载荷。
    Conflict,
    /// 字段错误，见 `fieldErrors`。
    Validation,
    /// 宿主正在停止或服务降级，不接纳变更。
    Unavailable,
    NotFound,
    /// 持久化失败；真实值与草稿由调用方保留。
    Storage,
    /// 平台调用失败或超时。
    Platform,
    Internal,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::Forbidden => "forbidden",
            ErrorCode::Conflict => "conflict",
            ErrorCode::Validation => "validation",
            ErrorCode::Unavailable => "unavailable",
            ErrorCode::NotFound => "not_found",
            ErrorCode::Storage => "storage",
            ErrorCode::Platform => "platform",
            ErrorCode::Internal => "internal",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 统一错误载荷：所有请求返回 `Result<T, AppError>`。
#[derive(Debug, Clone, Serialize, Deserialize, TS, Error)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
#[error("{code}: {message}")]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub field_errors: Option<Vec<FieldError>>,
    /// conflict 时携带宿主当前版本，便于窗口重拉快照。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub current_revision: Option<Revision>,
}

impl AppError {
    fn new(code: ErrorCode, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code,
            message: message.into(),
            retryable,
            field_errors: None,
            current_revision: None,
        }
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Forbidden, message, false)
    }

    pub fn conflict(message: impl Into<String>, current_revision: Option<Revision>) -> Self {
        Self {
            current_revision,
            ..Self::new(ErrorCode::Conflict, message, false)
        }
    }

    pub fn validation(field_errors: Vec<FieldError>) -> Self {
        let message = field_errors
            .iter()
            .map(|e| format!("{}: {}", e.field, e.message))
            .collect::<Vec<_>>()
            .join("；");
        Self {
            field_errors: Some(field_errors),
            ..Self::new(ErrorCode::Validation, message, false)
        }
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Unavailable, message, true)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, message, false)
    }

    pub fn storage(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Storage, message, true)
    }

    pub fn platform(message: impl Into<String>, retryable: bool) -> Self {
        Self::new(ErrorCode::Platform, message, retryable)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message, false)
    }
}

/// 跨 IPC 结果别名。
pub type AppResult<T> = Result<T, AppError>;

/// 已提交的设置快照：`persisted=false` 表示存储不可用时的临时默认值（未持久化）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct SettingsSnapshot {
    pub revision: Revision,
    pub persisted: bool,
    #[serde(flatten)]
    #[ts(flatten)]
    pub settings: Settings,
}

/// `settings_update` 请求：变更携带 requestId 与 expectedRevision（architecture.md §8）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct SettingsUpdateRequest {
    pub request_id: String,
    pub expected_revision: Revision,
    pub patch: SettingsPatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct PermissionSnapshot {
    pub revision: Revision,
    pub records: Vec<PermissionRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum OperationState {
    InProgress,
    Completed,
    Failed,
}

/// `permissions_request` 立即返回的操作句柄；结果经 permissions.changed 事件与快照异步更新。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct PermissionOperation {
    pub operation_id: String,
    pub permission: Permission,
    pub state: OperationState,
    pub started_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum StorageState {
    Ready,
    /// 数据库不可用/损坏：设置为未持久化的临时默认值，可进入诊断与权限。
    Degraded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct StorageStatus {
    pub state: StorageState,
    pub data_dir_display: String,
    pub schema_version: u32,
    pub message: Option<String>,
    /// 迁移前备份文件（显示路径），无则 None。
    pub last_backup_display: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum DirectoryPickResult {
    Selected {
        snapshot: ContextSnapshot,
    },
    /// 取消不改快照、不建会话或进程。
    Cancelled,
    Failed {
        message: String,
    },
}

/// `app_bootstrap`：窗口角色由宿主注入，不带凭据或全部日志。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct AppBootstrap {
    pub build_info: BuildInfo,
    pub host_state: HostState,
    pub window_role: String,
    pub storage: StorageStatus,
    pub settings: SettingsSnapshot,
    pub permissions: PermissionSnapshot,
    pub platform: PlatformCapabilities,
    pub context: Option<ContextSnapshot>,
}

/// `diagnostics_get`：仅安全字段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsSnapshot {
    pub build_info: BuildInfo,
    pub host_state: HostState,
    pub generation: String,
    pub storage: StorageStatus,
    pub log_dir_display: String,
    pub applied_migrations: Vec<String>,
    pub permissions: PermissionSnapshot,
    pub platform: PlatformCapabilities,
    pub credential_store: CredentialStoreStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CredentialStoreStatus {
    pub available: bool,
    pub namespace: String,
    pub message: Option<String>,
}

/// 低频事件载荷（settings.changed / permissions.changed / platform.changed / context.changed / host.changed）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum AppEvent {
    SettingsChanged {
        revision: Revision,
    },
    PermissionsChanged {
        revision: Revision,
    },
    PlatformChanged {
        revision: Revision,
    },
    ContextChanged {
        context_id: String,
        revision: Revision,
    },
    HostStateChanged {
        state: HostState,
    },
    /// 会话新建/更新/结束/删除；deleted=true 时 revision 无意义。
    SessionChanged {
        session_id: String,
        revision: Revision,
        deleted: bool,
    },
    /// 输入条显示状态变化（可见性/可见会话/抑制）。
    SurfaceChanged {
        revision: Revision,
    },
    /// 终端状态变化（创建/退出/目录同步/等待命令撤销）；大输出走 Channel。
    TerminalChanged {
        session_id: String,
        revision: Revision,
    },
    /// Run 状态变化（提交/确认/执行/终态/重试）。
    RunChanged {
        run_id: String,
        revision: Revision,
    },
    RulesChanged {
        revision: Revision,
    },
    FavoritesChanged {
        revision: Revision,
    },
    /// 工具安装状态变化（受管安装成功/卸载；探测刷新不产生事件）。
    ToolsChanged {
        tool_id: String,
    },
    /// 模型端点配置变化（保存/删除）。
    ProvidersChanged,
}

impl AppEvent {
    /// Tauri 事件名。合同中的 `settings.changed` 等名称在传输层写作 `settings:changed`：
    /// Tauri 事件名只允许字母数字与 `-` `/` `:` `_`，不允许 `.`。
    pub fn name(&self) -> &'static str {
        match self {
            AppEvent::SettingsChanged { .. } => "settings:changed",
            AppEvent::PermissionsChanged { .. } => "permissions:changed",
            AppEvent::PlatformChanged { .. } => "platform:changed",
            AppEvent::ContextChanged { .. } => "context:changed",
            AppEvent::HostStateChanged { .. } => "host:changed",
            AppEvent::SessionChanged { .. } => "session:changed",
            AppEvent::SurfaceChanged { .. } => "surface:changed",
            AppEvent::TerminalChanged { .. } => "terminal:changed",
            AppEvent::RunChanged { .. } => "run:changed",
            AppEvent::RulesChanged { .. } => "rules:changed",
            AppEvent::FavoritesChanged { .. } => "favorites:changed",
            AppEvent::ToolsChanged { .. } => "tools:changed",
            AppEvent::ProvidersChanged => "providers:changed",
        }
    }
}
