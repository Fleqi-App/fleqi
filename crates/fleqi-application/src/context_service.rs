//! ContextService：把平台原始读取转成不可变 ContextSnapshot；目录选择取消不改快照。

use fleqi_domain::context::{ContextSnapshot, ContextSnapshotBuilder, ContextSource, ViewKind};
use fleqi_domain::revision::Revision;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::dto::{AppError, AppEvent, AppResult, DirectoryPickResult};
use crate::paths::PathRegistry;
use crate::ports::{Clock, ContextPort, DirectoryPick, EventSink, RawContext};

const HISTORY: usize = 8;

struct State {
    latest: Option<ContextSnapshot>,
    history: VecDeque<ContextSnapshot>,
    revision: Revision,
    counter: u64,
}

pub struct ContextService {
    port: Arc<dyn ContextPort>,
    clock: Arc<dyn Clock>,
    events: Arc<dyn EventSink>,
    paths: Arc<PathRegistry>,
    ids: crate::ports::SequenceIds,
    state: Mutex<State>,
    capture: Mutex<()>,
}

impl ContextService {
    pub fn new(
        port: Arc<dyn ContextPort>,
        clock: Arc<dyn Clock>,
        events: Arc<dyn EventSink>,
        paths: Arc<PathRegistry>,
    ) -> Self {
        Self {
            port,
            clock,
            events,
            paths,
            ids: crate::ports::SequenceIds::new(),
            capture: Mutex::new(()),
            state: Mutex::new(State {
                latest: None,
                history: VecDeque::new(),
                revision: Revision::new(0),
                counter: 0,
            }),
        }
    }

    pub fn latest(&self) -> Option<ContextSnapshot> {
        self.state.lock().expect("context 锁").latest.clone()
    }

    /// 无 ID 返回最新快照（尚无则真实刷新一次）；有 ID 只返回仍保留的同一快照。
    pub fn get(&self, id: Option<&str>) -> AppResult<ContextSnapshot> {
        match id {
            None => Ok(self.latest().unwrap_or_else(|| self.refresh())),
            Some(id) => {
                let state = self.state.lock().expect("context 锁");
                state
                    .history
                    .iter()
                    .rev()
                    .find(|s| s.id == id)
                    .cloned()
                    .ok_or_else(|| AppError::not_found(format!("上下文 {id} 已不在保留范围")))
            }
        }
    }

    pub fn refresh(&self) -> ContextSnapshot {
        let _capture = self.capture.lock().expect("context capture");
        let raw = self.port.capture();
        if self.port.source() == ContextSource::Explorer
            && raw.directory.is_none()
            && let Some(picked) = self.latest()
            && picked.source == ContextSource::Picker
            && picked
                .directory_ref
                .as_ref()
                .and_then(|path| self.paths.resolve(&path.id))
                .is_some_and(|path| path.is_dir())
        {
            return picked;
        }
        self.publish(raw, self.port.source())
    }

    /// Re-read Finder before submitting a displayed snapshot. Return the displayed
    /// inputs, never substitute the latest selection into an already composed action.
    pub fn validate_current(&self, id: &str) -> AppResult<ContextSnapshot> {
        let expected = self.get(Some(id))?;
        if expected.source == ContextSource::Picker {
            return Ok(expected);
        }
        let current = self.refresh();
        if !matches!(
            current.availability,
            fleqi_domain::context::ContextAvailability::Available
        ) || !current.selection_complete
        {
            return Err(AppError::unavailable(
                "无法核对文件管理器当前选区，请重新读取后再提交",
            ));
        }
        if !expected.same_inputs(&current) {
            return Err(AppError::conflict(
                "文件管理器选区或目录已变化，请核对当前文件后重新提交",
                Some(current.revision),
            ));
        }
        Ok(expected)
    }

    pub fn pick_directory(&self) -> DirectoryPickResult {
        let _capture = self.capture.lock().expect("context capture");
        match self.port.pick_directory() {
            DirectoryPick::Selected(path) => {
                let raw = RawContext {
                    directory: Some(path),
                    view_kind: Some(ViewKind::Physical),
                    ..RawContext::default()
                };
                DirectoryPickResult::Selected {
                    snapshot: self.publish(raw, ContextSource::Picker),
                }
            }
            DirectoryPick::Cancelled => DirectoryPickResult::Cancelled,
            DirectoryPick::Failed(message) => DirectoryPickResult::Failed { message },
        }
    }

    fn publish(&self, raw: RawContext, source: ContextSource) -> ContextSnapshot {
        let (id, revision) = {
            let mut state = self.state.lock().expect("context 锁");
            state.counter += 1;
            state.revision = state.revision.next();
            (
                crate::ports::IdGenerator::next_id(&self.ids, "ctx"),
                state.revision,
            )
        };
        let mut builder =
            ContextSnapshotBuilder::new(id.clone(), revision, self.clock.now_rfc3339())
                .source(source)
                .view_kind(raw.view_kind.unwrap_or(ViewKind::Physical))
                .directory(
                    raw.directory
                        .as_ref()
                        .map(|d| self.paths.register(&d.native, d.kind)),
                )
                .selection(
                    raw.selection
                        .iter()
                        .map(|s| self.paths.register(&s.native, s.kind))
                        .collect(),
                );
        if let Some(window) = raw.source_window_id {
            builder = builder.finder_window(window);
        }
        if let Some(unavailable) = raw.unavailable {
            builder = builder.unavailable(unavailable);
        }
        let snapshot = builder.build();
        {
            let mut state = self.state.lock().expect("context 锁");
            if let Some(current) = &state.latest
                && current.same_inputs(&snapshot)
            {
                return current.clone();
            }
            state.latest = Some(snapshot.clone());
            state.history.push_back(snapshot.clone());
            while state.history.len() > HISTORY {
                state.history.pop_front();
            }
        }
        self.events.emit(AppEvent::ContextChanged {
            context_id: id,
            revision,
        });
        snapshot
    }
}
