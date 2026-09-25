//! 权限模型（architecture.md §12.4）：只处理 Finder 自动化与辅助功能；
//! 权限是系统事实，不从 SQLite 恢复；先前允许后不允许才推导撤销。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum Permission {
    /// Apple Events → com.apple.finder（AEDeterminePermissionToAutomateTarget）。
    FinderAutomation,
    /// AXIsProcessTrustedWithOptions。
    Accessibility,
}

impl Permission {
    pub const ALL: [Permission; 2] = [Permission::FinderAutomation, Permission::Accessibility];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum PermissionStatus {
    /// 尚未检查（进入页面前）。
    Unknown,
    Allowed,
    /// 系统尚未询问过用户，需要显式申请。
    NeedsConsent,
    Denied,
    /// 目标应用（Finder）未运行，无法判定；被动检查不启动它。
    TargetNotRunning,
    /// 系统调用失败或超时；`error` 说明原因。
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum PermissionProcedure {
    /// 无提示检测（askUserIfNeeded=false）。
    Passive,
    /// 用户显式申请（askUserIfNeeded=true / 带提示）。
    Explicit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum RecoveryAction {
    None,
    RequestExplicitly,
    OpenSystemSettings,
    LaunchTarget,
    Recheck,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct PermissionRecord {
    pub permission: Permission,
    pub status: PermissionStatus,
    pub procedure: PermissionProcedure,
    /// RFC 3339；Unknown 时为 None。
    pub checked_at: Option<String>,
    pub error: Option<String>,
    pub recovery: RecoveryAction,
    /// 上一次为 Allowed、本次不再允许。
    pub revoked: bool,
}

impl PermissionRecord {
    pub fn unknown(permission: Permission) -> Self {
        Self {
            permission,
            status: PermissionStatus::Unknown,
            procedure: PermissionProcedure::Passive,
            checked_at: None,
            error: None,
            recovery: RecoveryAction::Recheck,
            revoked: false,
        }
    }

    pub fn new(
        permission: Permission,
        status: PermissionStatus,
        procedure: PermissionProcedure,
        checked_at: &str,
    ) -> Self {
        Self {
            permission,
            status,
            procedure,
            checked_at: Some(checked_at.to_owned()),
            error: None,
            recovery: recovery_for(status),
            revoked: false,
        }
    }

    pub fn with_error(mut self, error: impl Into<String>) -> Self {
        self.error = Some(error.into());
        self
    }

    /// 以本记录为前态生成新记录；撤销只在 Allowed → 非 Allowed 且能明确判定时成立。
    pub fn transition(
        &self,
        status: PermissionStatus,
        procedure: PermissionProcedure,
        checked_at: &str,
    ) -> Self {
        let revoked = self.status == PermissionStatus::Allowed
            && matches!(
                status,
                PermissionStatus::Denied | PermissionStatus::NeedsConsent
            );
        Self {
            permission: self.permission,
            status,
            procedure,
            checked_at: Some(checked_at.to_owned()),
            error: None,
            recovery: recovery_for(status),
            revoked,
        }
    }
}

fn recovery_for(status: PermissionStatus) -> RecoveryAction {
    match status {
        PermissionStatus::Allowed => RecoveryAction::None,
        PermissionStatus::NeedsConsent => RecoveryAction::RequestExplicitly,
        PermissionStatus::Denied => RecoveryAction::OpenSystemSettings,
        PermissionStatus::TargetNotRunning => RecoveryAction::LaunchTarget,
        PermissionStatus::Failed | PermissionStatus::Unknown => RecoveryAction::Recheck,
    }
}
