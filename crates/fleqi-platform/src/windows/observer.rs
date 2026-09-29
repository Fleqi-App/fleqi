//! 文件管理器激活观察。Windows 首版不伪造资源管理器激活事件；
//! `note_activation` 供宿主在真实前台变化时去抖刷新。

use fleqi_application::dto::AppError;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const DEBOUNCE: Duration = Duration::from_millis(250);

pub type ContextRefresher = Box<dyn Fn(bool) + Send + Sync>;

pub struct FileManagerActivationObserver {
    last_refresh: Mutex<Option<Instant>>,
    refresher: ContextRefresher,
    installed: AtomicBool,
}

impl FileManagerActivationObserver {
    pub fn new(refresher: ContextRefresher) -> Arc<Self> {
        Arc::new(Self {
            last_refresh: Mutex::new(None),
            refresher,
            installed: AtomicBool::new(false),
        })
    }

    /// 幂等注册。不合成资源管理器激活，因此不会调用 refresher。
    pub fn install(self: &Arc<Self>) -> Result<(), AppError> {
        self.installed.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// 第一次立即刷新；250ms 内的后续调用丢弃。
    pub fn note_activation(&self, active: bool) {
        let now = Instant::now();
        let mut last = self.last_refresh.lock().expect("observer");
        if last.is_some_and(|previous| now.saturating_duration_since(previous) < DEBOUNCE) {
            return;
        }
        *last = Some(now);
        drop(last);
        (self.refresher)(active);
    }
}
