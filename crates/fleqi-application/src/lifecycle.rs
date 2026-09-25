//! 宿主生命周期服务：状态机 + 运行代际（architecture.md §12.2）。

use fleqi_domain::lifecycle::{Generation, HostState};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::dto::{AppError, AppEvent};
use crate::ports::EventSink;

pub struct HostLifecycle {
    state: Mutex<HostState>,
    generation: AtomicU64,
    events: Arc<dyn EventSink>,
}

impl HostLifecycle {
    pub fn new(events: Arc<dyn EventSink>) -> Self {
        Self {
            state: Mutex::new(HostState::Starting),
            generation: AtomicU64::new(Generation::initial().value()),
            events,
        }
    }

    pub fn state(&self) -> HostState {
        *self.state.lock().expect("lifecycle 锁")
    }

    pub fn generation(&self) -> Generation {
        Generation::from_value(self.generation.load(Ordering::SeqCst))
    }

    /// 非法转换返回 Err 并保持原状态。进入 stopping 时递增代际，迟到回执失效。
    pub fn transition(&self, next: HostState) -> Result<HostState, HostState> {
        let mut guard = self.state.lock().expect("lifecycle 锁");
        if !guard.can_transition_to(next) {
            return Err(*guard);
        }
        *guard = next;
        if next == HostState::Stopping {
            self.generation.fetch_add(1, Ordering::SeqCst);
        }
        drop(guard);
        self.events.emit(AppEvent::HostStateChanged { state: next });
        Ok(next)
    }

    /// 变更类请求的统一门禁。
    pub fn ensure_accepts_changes(&self) -> Result<(), AppError> {
        let state = self.state();
        if state.accepts_changes() {
            Ok(())
        } else {
            Err(AppError::unavailable(format!(
                "宿主状态 {state:?} 不接纳变更"
            )))
        }
    }
}
