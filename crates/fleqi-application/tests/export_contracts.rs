//! 显式契约导出：仅在 `--features export-contracts` 时作为测试生成 TS 绑定。
//! 普通 `cargo test` 不运行本文件，也不改生成产物（P0-CONTRACT-001）。
//! 输出目录由 `TS_RS_EXPORT_DIR` 控制（check:contracts 用临时目录比对，
//! contracts:regen 用仓库根）。跨 IPC 走 JSON，u64 在 JS 侧是 number；
//! 需要精确的版本/序号已用十进制字符串表达（architecture.md §4）。
#![cfg(feature = "export-contracts")]

use fleqi_application::collection_service::{Favorite, Rule, RuleScope};
use fleqi_application::dto::{
    AppBootstrap, AppError, AppEvent, BuildInfo, DiagnosticsSnapshot, DirectoryPickResult,
    ErrorCode, PermissionOperation, SettingsUpdateRequest,
};
use fleqi_application::planning_service::PlanOutcome;
use fleqi_application::ports::InstallProgress;
use fleqi_application::provider_service::{ProviderRecord, ProviderSaveRequest, ProviderView};
use fleqi_application::run_service::{AiPolicyWire, RunOriginWire, RunRecord};
use fleqi_application::tool_service::ToolEntry;
use fleqi_domain::context::ContextSnapshot;
use fleqi_domain::directory_sync::QueuedLine;
use fleqi_domain::session::{Session, SessionState};
use fleqi_domain::settings::Settings;
use fleqi_domain::terminal::TerminalSnapshot;
use fleqi_domain::tools::{InstalledTool, ToolManifest, ToolOwner, ToolStatus};
use ts_rs::{Config, TS};

#[test]
fn export_bindings() {
    let cfg = Config::from_env().with_large_int("number");
    BuildInfo::export_all(&cfg).expect("导出 BuildInfo 绑定");
    fleqi_application::dto::AppUpdateStatus::export_all(&cfg).expect("export update status");
    AppError::export_all(&cfg).expect("导出 AppError 绑定");
    ErrorCode::export_all(&cfg).expect("导出 ErrorCode 绑定");
    AppBootstrap::export_all(&cfg).expect("导出 AppBootstrap 及依赖");
    Settings::export_all(&cfg).expect("导出 Settings（SettingsSnapshot 平铺其字段）");
    DiagnosticsSnapshot::export_all(&cfg).expect("导出 DiagnosticsSnapshot 及依赖");
    SettingsUpdateRequest::export_all(&cfg).expect("导出 SettingsUpdateRequest 及依赖");
    PermissionOperation::export_all(&cfg).expect("导出 PermissionOperation");
    DirectoryPickResult::export_all(&cfg).expect("导出 DirectoryPickResult");
    AppEvent::export_all(&cfg).expect("导出 AppEvent");
    fleqi_domain::session::ConversationEntry::export_all(&cfg).expect("export conversation entry");
    Session::export_all(&cfg).expect("导出 Session 及依赖");
    SessionState::export_all(&cfg).expect("导出 SessionState");
    TerminalSnapshot::export_all(&cfg).expect("导出 TerminalSnapshot 及依赖");
    QueuedLine::export_all(&cfg).expect("导出 QueuedLine");
    ToolManifest::export_all(&cfg).expect("导出 ToolManifest 及依赖");
    ToolOwner::export_all(&cfg).expect("导出 ToolOwner");
    ToolStatus::export_all(&cfg).expect("导出 ToolStatus");
    InstalledTool::export_all(&cfg).expect("导出 InstalledTool");
    InstallProgress::export_all(&cfg).expect("导出 InstallProgress");
    ToolEntry::export_all(&cfg).expect("导出 ToolEntry 及依赖");
    fleqi_application::tool_service::ToolPreparation::export_all(&cfg)
        .expect("export tool preparation");
    RunOriginWire::export_all(&cfg).expect("导出 RunOriginWire");
    AiPolicyWire::export_all(&cfg).expect("导出 AiPolicyWire");
    fleqi_domain::execution::ExecutionPlan::export_all(&cfg).expect("export execution plan");
    fleqi_application::capability_service::CapabilityForm::export_all(&cfg)
        .expect("export capability form");
    RunRecord::export_all(&cfg).expect("导出 RunRecord 及依赖");
    RuleScope::export_all(&cfg).expect("导出 RuleScope");
    Rule::export_all(&cfg).expect("导出 Rule 及依赖");
    Favorite::export_all(&cfg).expect("导出 Favorite");
    ProviderRecord::export_all(&cfg).expect("导出 ProviderRecord");
    ProviderSaveRequest::export_all(&cfg).expect("导出 ProviderSaveRequest 及依赖");
    ProviderView::export_all(&cfg).expect("导出 ProviderView 及依赖");
    PlanOutcome::export_all(&cfg).expect("导出 PlanOutcome 及依赖");
}
