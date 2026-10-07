//! 平台能力状态推导：与权限事实和凭据服务自检结果对应，与构建平台名分离。

use fleqi_domain::permissions::{Permission, PermissionStatus};
use fleqi_domain::platform::{CapabilityState, PlatformCapabilities, PlatformCapability};
use fleqi_domain::revision::Revision;

use crate::dto::{CredentialStoreStatus, PermissionSnapshot};

fn from_permission(
    id: &str,
    permissions: &PermissionSnapshot,
    permission: Permission,
) -> PlatformCapability {
    let record = permissions
        .records
        .iter()
        .find(|r| r.permission == permission);
    let (state, reason, recovery) = match record.map(|r| r.status) {
        Some(PermissionStatus::Allowed) => (CapabilityState::Supported, None, None),
        Some(PermissionStatus::NeedsConsent) => (
            CapabilityState::PermissionRequired,
            Some("系统尚未授权".into()),
            Some("在权限与自检页显式申请".into()),
        ),
        Some(PermissionStatus::Denied) => (
            CapabilityState::PermissionRequired,
            Some("系统已拒绝".into()),
            Some("打开系统设置授予后重新检查".into()),
        ),
        Some(PermissionStatus::TargetNotRunning) => (
            CapabilityState::TemporarilyUnavailable,
            Some("Finder 未运行".into()),
            Some("启动 Finder 后重新检查".into()),
        ),
        Some(PermissionStatus::Failed) => (
            CapabilityState::TemporarilyUnavailable,
            record.and_then(|r| r.error.clone()),
            Some("重新检查".into()),
        ),
        Some(PermissionStatus::Unknown) | None => (
            CapabilityState::TemporarilyUnavailable,
            Some("尚未检查".into()),
            Some("运行权限自检".into()),
        ),
    };
    PlatformCapability {
        id: id.into(),
        state,
        reason,
        recovery,
    }
}

pub fn derive_capabilities(
    revision: Revision,
    permissions: &PermissionSnapshot,
    credentials: &CredentialStoreStatus,
) -> PlatformCapabilities {
    let items = vec![
        from_permission("finderContext", permissions, Permission::FinderAutomation),
        from_permission(
            "accessibilityGeometry",
            permissions,
            Permission::Accessibility,
        ),
        PlatformCapability {
            id: "directoryPicker".into(),
            state: CapabilityState::Supported,
            reason: None,
            recovery: None,
        },
        PlatformCapability {
            id: "credentialStore".into(),
            state: if credentials.available {
                CapabilityState::Supported
            } else {
                CapabilityState::TemporarilyUnavailable
            },
            reason: credentials.message.clone(),
            recovery: if credentials.available {
                None
            } else {
                Some("检查系统凭据服务后重新自检".into())
            },
        },
    ];
    PlatformCapabilities { revision, items }
}
