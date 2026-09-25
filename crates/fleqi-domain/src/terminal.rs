//! 终端快照类型（architecture.md §4、§6）。

use crate::directory_sync::DirectorySync;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum TerminalState {
    Starting,
    Running,
    Stopping,
    Exited,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum ShellReadinessState {
    Ready,
    Busy,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct TerminalSize {
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct TerminalSnapshot {
    pub terminal_id: String,
    pub session_id: String,
    pub state: TerminalState,
    pub shell: String,
    pub size: TerminalSize,
    pub shell_readiness: ShellReadinessState,
    pub foreground_process: Option<String>,
    pub current_directory: String,
    pub pending_directory: Option<String>,
    pub directory_sync: DirectorySync,
    /// 屏幕恢复数据：主屏/备用屏 + 光标（xterm 以此重建）。
    pub screen: String,
    /// 十进制字符串序号；订阅从此处继续。
    pub stream_cursor: String,
    pub exit_status: Option<i32>,
    /// 持久输出是否已截断（超过 100 MiB 限额）。
    pub truncated: bool,
}
