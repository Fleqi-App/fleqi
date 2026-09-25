//! 平台能力状态（architecture.md §4）：每项能力的支持/权限/暂不可用/不支持，与构建平台名分离。

use crate::revision::Revision;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum CapabilityState {
    Supported,
    PermissionRequired,
    TemporarilyUnavailable,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct PlatformCapability {
    /// 稳定标识，如 `finderContext`、`accessibilityGeometry`、`directoryPicker`、`credentialStore`。
    pub id: String,
    pub state: CapabilityState,
    pub reason: Option<String>,
    pub recovery: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct PlatformCapabilities {
    pub revision: Revision,
    pub items: Vec<PlatformCapability>,
}
