//! Fleqi 平台适配层。
//!
//! 所有权：macOS 原生能力——Finder Apple Events、窗口、快捷键、权限、凭据与
//! 系统操作（architecture.md §10）。产出不可变 ContextSnapshot，不把 AppKit/AX
//! 对象带进跨平台 DTO；M1 起接入原生业务。

#[cfg(target_os = "macos")]
pub mod macos;
