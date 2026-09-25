//! PathRef 注册表：前端只见不透明 ID 与显示文本，原始 PathBuf 只留在 Rust。
//! 非 UTF-8 文件名不能通过 displayPath 往返执行（architecture.md §4）。

use crate::ports::{IdGenerator, SequenceIds};
use fleqi_domain::context::{PathKind, PathRef};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Default)]
pub struct PathRegistry {
    ids: SequenceIds,
    inner: Mutex<HashMap<String, PathBuf>>,
}

impl PathRegistry {
    pub fn new() -> Self {
        Self {
            ids: SequenceIds::new(),
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// 登记原始路径，返回可跨 IPC 的引用。
    pub fn register(&self, native: &Path, kind: PathKind) -> PathRef {
        let mut entries = self.inner.lock().expect("PathRegistry 锁");
        if let Some((id, _)) = entries.iter().find(|(_, path)| path.as_path() == native) {
            return PathRef::new(id.clone(), native.to_string_lossy().into_owned(), kind);
        }
        let id = self.ids.next_id("path");
        entries.insert(id.clone(), native.to_path_buf());
        PathRef::new(id, native.to_string_lossy().into_owned(), kind)
    }

    /// 由 ID 取回原始路径；显示文本不参与解析。
    pub fn resolve(&self, id: &str) -> Option<PathBuf> {
        self.inner.lock().expect("PathRegistry 锁").get(id).cloned()
    }

    pub fn len(&self) -> usize {
        self.inner.lock().expect("PathRegistry 锁").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
