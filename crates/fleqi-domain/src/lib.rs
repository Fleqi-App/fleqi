//! Fleqi 领域层。
//!
//! 所有权：实体、值对象、状态转换与执行策略（architecture.md §2、§3）。
//! 边界：不依赖 Tauri、HTTP、SQLite、系统窗口或 React；可在无 WebView、
//! Finder 与网络的测试环境中验证（architecture.md §11）。serde/ts-rs 只用于
//! 类型定义与契约生成。

pub mod composer;
pub mod context;
pub mod directory_sync;
pub mod execution;
pub mod idempotency;
pub mod lifecycle;
pub mod permissions;
pub mod platform;
pub mod revision;
pub mod session;
pub mod settings;
pub mod surface;
pub mod terminal;
pub mod tools;
