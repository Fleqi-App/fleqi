//! 会话实体与生命周期规则（architecture.md §4、§6.1；FR-SESSION-*）。

use crate::directory_sync::DirectorySync;
use crate::revision::Revision;
use crate::settings::limits;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum SessionState {
    Active,
    Ending,
    Ended,
    Failed,
    /// 重启时发现的未终结记录。
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub parent_session_id: Option<String>,
    pub title: String,
    pub state: SessionState,
    pub initial_directory: Option<String>,
    pub current_directory: Option<String>,
    pub target_directory: Option<String>,
    pub directory_sync: DirectorySync,
    pub terminal_id: Option<String>,
    pub pinned: bool,
    pub created_at: String,
    pub last_used_at: String,
    pub ended_at: Option<String>,
    pub revision: Revision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum EntryRole {
    User,
    ManualCommand,
    Assistant,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ConversationEntry {
    pub id: String,
    pub session_id: String,
    pub role: EntryRole,
    pub content: String,
    pub run_id: Option<String>,
    pub created_at: String,
}

/// 活跃会话上限 16：达到上限返回 Err(limit)，不静默终止或复用旧会话。
pub fn validate_active_limit(active_count: usize) -> Result<(), u32> {
    if active_count >= limits::ACTIVE_SESSIONS as usize {
        Err(limits::ACTIVE_SESSIONS)
    } else {
        Ok(())
    }
}

pub fn next_state_after_end(state: SessionState) -> SessionState {
    match state {
        SessionState::Active => SessionState::Ending,
        other => other,
    }
}

impl SessionState {
    pub fn is_active(self) -> bool {
        matches!(self, SessionState::Active | SessionState::Ending)
    }
}
