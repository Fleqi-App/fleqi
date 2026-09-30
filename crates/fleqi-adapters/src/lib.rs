//! Fleqi 基础设施适配层。
//!
//! 所有权：一次性进程（ProcessRunner）、持续 PTY（TerminalManager）、模型端点、
//! SQLite 存储与工具下载安装（architecture.md §3）。实现应用层端口；不与
//! fleqi-platform 相互直接调用，跨模块工作经用例编排。

pub mod capabilities;
pub mod document_operations;
mod environment;
pub mod extended;
pub mod file_operations;
mod heif;
pub use heif::is_heif;
pub mod image_operations;
pub mod logging;
pub mod media_operations;
pub mod model;
pub mod native_steps;
mod output_transaction;
pub mod pdf_operations;
pub mod process;
pub mod storage;
pub mod system_operations;
mod system_tools;
pub mod terminal;
pub mod tools;
