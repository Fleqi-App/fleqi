//! Linux 权限端口。
//!
//! Finder 自动化与 macOS 辅助功能信任在 Linux 上不存在。显式申请与被动检查
//! 返回同一失败状态，不弹出伪造的同意对话框。

pub use fleqi_application::ports::{PermissionPort, PermissionProbe};
pub use fleqi_domain::permissions::{Permission, PermissionProcedure, PermissionStatus};

const SETTINGS_URI: &str = "settings://privacy";
const NOT_GRANTED: &str = "Linux 没有 Finder 自动化，也没有 macOS 辅助功能信任。文件管理器上下文按当前桌面（X11/Wayland）分别报告，并不等同于 Finder。";

pub struct LinuxPermissions;

impl LinuxPermissions {
    pub fn for_installed_build(_data_dir: &std::path::Path) -> Self {
        Self
    }

    /// Linux 没有 TCC，更新后无需重置辅助功能信任。
    pub fn reset_error(&self) -> Option<String> {
        None
    }
}

impl Default for LinuxPermissions {
    fn default() -> Self {
        Self
    }
}

impl PermissionPort for LinuxPermissions {
    fn check(&self, _permission: Permission, _procedure: PermissionProcedure) -> PermissionProbe {
        PermissionProbe {
            status: PermissionStatus::Failed,
            error: Some(NOT_GRANTED.to_owned()),
        }
    }

    fn open_system_settings(&self, _permission: Permission) -> Result<(), String> {
        match std::process::Command::new("xdg-open")
            .arg(SETTINGS_URI)
            .status()
        {
            Ok(status) if status.success() => Ok(()),
            Ok(status) => Err(format!(
                "xdg-open 退出状态 {status}，未能打开系统设置。请手动打开 设置 → 隐私 / 文件管理器。"
            )),
            Err(error) => Err(format!(
                "无法运行 xdg-open（{error}）。请手动打开 设置 → 隐私 / 文件管理器。"
            )),
        }
    }
}
