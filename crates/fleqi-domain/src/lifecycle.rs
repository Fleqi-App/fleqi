//! 宿主生命周期状态与运行代际（architecture.md §12.2）。
//! 退出进入 stopping 后拒绝新变更；新代际使迟到结果失效。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum HostState {
    Starting,
    Ready,
    /// 存储失败等可降级启动：仍可进入权限/诊断，不写业务数据。
    Degraded,
    Stopping,
}

impl HostState {
    pub fn can_transition_to(self, next: HostState) -> bool {
        matches!(
            (self, next),
            (HostState::Starting, HostState::Ready)
                | (HostState::Starting, HostState::Degraded)
                | (HostState::Starting, HostState::Stopping)
                | (HostState::Ready, HostState::Degraded)
                | (HostState::Ready, HostState::Stopping)
                | (HostState::Degraded, HostState::Ready)
                | (HostState::Degraded, HostState::Stopping)
        )
    }

    /// 只有 ready 接纳设置等变更；degraded 允许只读诊断与权限操作。
    pub fn accepts_changes(self) -> bool {
        matches!(self, HostState::Ready)
    }
}

/// 运行代际：每次进入 stopping 或重建服务时递增，旧代际的异步回执一律丢弃。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Generation(u64);

impl Generation {
    pub const fn initial() -> Self {
        Self(1)
    }

    pub const fn from_value(value: u64) -> Self {
        Self(value)
    }

    pub const fn next(self) -> Self {
        Self(self.0 + 1)
    }

    pub const fn is_current(self, observed: Generation) -> bool {
        self.0 == observed.0
    }

    pub const fn value(self) -> u64 {
        self.0
    }
}
