//! Fleqi 平台适配层。
//!
//! macOS 原生能力在 `macos`：Finder、窗口、快捷键、权限、凭据（architecture.md §10）。
//! Windows 与 Linux 模块提供各自的上下文、凭据、权限、窗口材质与更新说明。
//! 未经验收的桌面环境不得标成与 Finder 等价。原生对象不进入跨平台 DTO。

pub mod credentials;
pub mod frame;
pub mod host;
pub mod scheduling;

#[cfg(target_os = "macos")]
pub mod macos;

pub mod linux;
pub mod windows;
