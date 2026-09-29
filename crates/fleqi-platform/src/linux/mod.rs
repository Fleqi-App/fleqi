//! Linux 桌面适配。
//!
//! 会话、文件管理器与安装包都按 Linux 事实报告：不把文件管理器读成 Finder，
//! 不把 Secret Service 退回文件，也不安装 macOS 的 `Fleqi.app`。

pub mod context;
pub mod credentials;
pub mod observer;
pub mod permissions;
pub mod picker;
pub mod surface;
pub mod update_install;
