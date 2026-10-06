//! Finder 上下文快照（architecture.md §4、§12.4）：不可变；虚拟目录不猜 cwd；
//! 超过选区上限明确超限而不截取；PathRef 显示文本与原生路径分离。

use crate::revision::Revision;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use crate::settings::limits::SELECTION_ITEMS as SELECTION_LIMIT;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum PathKind {
    Directory,
    File,
    Other,
}

/// 前端与模型只引用 `id`；`display_path` 是有损显示文本，不能往返执行。
/// Rust 端由 PathRegistry 绑定原始 PathBuf。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct PathRef {
    pub id: String,
    pub display_path: String,
    pub kind: PathKind,
}

impl PathRef {
    pub fn new(id: impl Into<String>, display_path: impl Into<String>, kind: PathKind) -> Self {
        Self {
            id: id.into(),
            display_path: display_path.into(),
            kind,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum ContextSource {
    Finder,
    Explorer,
    Picker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum ViewKind {
    /// 真实文件夹窗口。
    Physical,
    /// 搜索、智能文件夹、最近项目等没有单一真实目录的视图。
    Virtual,
    Desktop,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum ContextAvailability {
    Available,
    /// 没有有效执行目录（虚拟视图、无 Finder 窗口等）；`reason` 供 UI 说明。
    NoDirectory {
        reason: String,
    },
    PermissionRequired,
    FinderNotRunning,
    SelectionOverLimit {
        count: usize,
        limit: usize,
    },
    Failed {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ContextSnapshot {
    pub id: String,
    pub revision: Revision,
    pub source: ContextSource,
    pub source_window_id: Option<u64>,
    pub directory_ref: Option<PathRef>,
    /// 平台可提供的顺序；超限时为空。
    pub selected_items: Vec<PathRef>,
    pub view_kind: ViewKind,
    pub captured_at: String,
    pub availability: ContextAvailability,
    /// false 表示选区因超限或读取失败而不完整。
    pub selection_complete: bool,
}

impl ContextSnapshot {
    /// Capture time and revision are not input identity. A different window, directory,
    /// selection or incomplete reading must never silently replace a displayed input.
    pub fn same_inputs(&self, other: &Self) -> bool {
        self.source == other.source
            && self.source_window_id == other.source_window_id
            && self.directory_ref == other.directory_ref
            && self.selected_items == other.selected_items
            && self.view_kind == other.view_kind
            && self.availability == other.availability
            && self.selection_complete == other.selection_complete
    }
}

pub struct ContextSnapshotBuilder {
    snapshot: ContextSnapshot,
    selection: Vec<PathRef>,
    forced: Option<ContextAvailability>,
}

impl ContextSnapshotBuilder {
    pub fn new(id: impl Into<String>, revision: Revision, captured_at: impl Into<String>) -> Self {
        Self {
            snapshot: ContextSnapshot {
                id: id.into(),
                revision,
                source: ContextSource::Finder,
                source_window_id: None,
                directory_ref: None,
                selected_items: Vec::new(),
                view_kind: ViewKind::Physical,
                captured_at: captured_at.into(),
                availability: ContextAvailability::Available,
                selection_complete: true,
            },
            selection: Vec::new(),
            forced: None,
        }
    }

    pub fn source(mut self, source: ContextSource) -> Self {
        self.snapshot.source = source;
        self
    }

    pub fn finder_window(mut self, window_id: u64) -> Self {
        self.snapshot.source_window_id = Some(window_id);
        self
    }

    pub fn view_kind(mut self, kind: ViewKind) -> Self {
        self.snapshot.view_kind = kind;
        self
    }

    pub fn directory(mut self, directory: Option<PathRef>) -> Self {
        self.snapshot.directory_ref = directory;
        self
    }

    pub fn selection(mut self, items: Vec<PathRef>) -> Self {
        self.selection = items;
        self
    }

    /// 平台层已判定的不可用原因（权限、Finder 未运行、读取失败）优先于推导规则。
    pub fn unavailable(mut self, availability: ContextAvailability) -> Self {
        self.forced = Some(availability);
        self
    }

    pub fn build(mut self) -> ContextSnapshot {
        let count = self.selection.len();
        if count > SELECTION_LIMIT {
            self.snapshot.selected_items = Vec::new();
            self.snapshot.selection_complete = false;
            self.snapshot.availability = ContextAvailability::SelectionOverLimit {
                count,
                limit: SELECTION_LIMIT,
            };
            return self.snapshot;
        }
        self.snapshot.selected_items = self.selection;
        if let Some(forced) = self.forced {
            self.snapshot.availability = forced;
            self.snapshot.selection_complete = false;
            return self.snapshot;
        }
        self.snapshot.availability = match (&self.snapshot.directory_ref, self.snapshot.view_kind) {
            (Some(_), _) => ContextAvailability::Available,
            (None, ViewKind::Virtual) => ContextAvailability::NoDirectory {
                reason: "当前 Finder 视图（搜索/智能目录）没有单一真实目录".into(),
            },
            (None, ViewKind::Desktop) => ContextAvailability::NoDirectory {
                reason: "桌面未提供目录路径".into(),
            },
            (None, ViewKind::Physical) => ContextAvailability::NoDirectory {
                reason: "没有有效的 Finder 文件夹".into(),
            },
        };
        self.snapshot
    }
}
