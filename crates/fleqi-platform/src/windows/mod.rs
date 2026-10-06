//! Windows 平台适配。
//!
//! 凭据、资源管理器上下文、权限、窗口材质与更新说明。
//! Win32 与 PowerShell 只在 Windows 上执行；其它系统返回不可用或失败，不启动子进程。

#[cfg(windows)]
mod native;

pub mod context;
pub mod credentials;
pub mod observer;
pub mod permissions;
pub mod picker;
pub mod surface;
pub mod update_install;
