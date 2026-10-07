//! Windows 没有 macOS 的 Finder 自动化或辅助功能信任。
//! 这两项权限一律失败，避免被上层当成已授权。

use fleqi_application::ports::{PermissionPort, PermissionProbe};
use fleqi_domain::permissions::{Permission, PermissionProcedure, PermissionStatus};

const MANUAL_SETTINGS: &str = "请手动打开 设置 → 隐私和安全性";

#[derive(Default)]
pub struct WindowsPermissions;

impl WindowsPermissions {
    pub fn for_installed_build(_data_dir: &std::path::Path) -> Self {
        Self
    }

    pub fn reset_error(&self) -> Option<String> {
        None
    }
}

fn failed(message: &str) -> PermissionProbe {
    PermissionProbe {
        status: PermissionStatus::Failed,
        error: Some(message.to_owned()),
    }
}

fn settings_uri(permission: Permission) -> &'static str {
    match permission {
        Permission::FinderAutomation => "ms-settings:privacy",
        Permission::Accessibility => "ms-settings:easeofaccess",
    }
}

impl PermissionPort for WindowsPermissions {
    fn check(&self, permission: Permission, procedure: PermissionProcedure) -> PermissionProbe {
        let _ = (self, procedure);
        match permission {
            Permission::FinderAutomation => failed(
                "Windows 不使用 Finder 自动化；资源管理器上下文从 Shell 读取，不等同于 macOS。",
            ),
            Permission::Accessibility => {
                failed("UI Automation 与窗口几何不会被报告为 macOS 辅助功能信任。")
            }
        }
    }

    fn open_system_settings(&self, permission: Permission) -> Result<(), String> {
        let uri = settings_uri(permission);
        #[cfg(target_os = "windows")]
        {
            spawn_settings(uri)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (self, uri);
            Err(MANUAL_SETTINGS.into())
        }
    }
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn spawn_settings(uri: &str) -> Result<(), String> {
    // 固定 ms-settings URI 作为独立参数。不经过 cmd.exe，也不拼接用户字符串。
    std::process::Command::new("explorer.exe")
        .arg(uri)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("无法打开系统设置（{error}）。{MANUAL_SETTINGS}"))
}
