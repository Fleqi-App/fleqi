//! Fleqi 应用编排层。
//!
//! 所有权：SessionRegistry、RunRegistry、用户动作排序与生命周期；声明端口并
//! 只依赖领域模型（architecture.md §3）。跨 IPC DTO 位于本 crate，serde 使用
//! camelCase，ts-rs 显式导出到 packages/contracts（architecture.md §12.1）。

pub mod capability_service;
pub mod collection_service;
pub mod context_service;
pub mod dto;
pub mod file_capability_specs;
pub mod fingerprint;
pub mod lifecycle;
pub mod paths;
pub mod permission_service;
pub mod planning_service;
pub mod platform_caps;
pub mod ports;
pub mod provider_service;
pub mod run_service;
pub mod secrets;
pub mod session_service;
pub mod settings_service;
pub mod summary_service;
pub mod surface_service;
pub mod system_capability_specs;
pub mod terminal_service;
pub mod tool_service;

pub use dto::{AppError, AppResult, BuildInfo, ErrorCode};
