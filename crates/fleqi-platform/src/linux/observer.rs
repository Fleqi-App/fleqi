//! 文件管理器激活观察器。
//!
//! Linux 这里没有 NSWorkspace 通知流。`install` 只记录已安装并返回成功，
//! 不伪造一次激活。测试通过 `note_activation` 注入事件，250ms 内合并为一次。

use fleqi_application::dto::AppError;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const DEBOUNCE: Duration = Duration::from_millis(250);

pub type ContextRefresher = Box<dyn Fn(bool) + Send + Sync>;

pub struct FileManagerActivationObserver {
    refresher: ContextRefresher,
    installed: AtomicBool,
    last_refresh: Mutex<Option<Instant>>,
}

impl FileManagerActivationObserver {
    pub fn new(refresher: ContextRefresher) -> Arc<Self> {
        Arc::new(Self {
            refresher,
            installed: AtomicBool::new(false),
            last_refresh: Mutex::new(None),
        })
    }

    /// 幂等。不订阅桌面总线，也不调用 refresher。
    pub fn install(self: &Arc<Self>) -> Result<(), AppError> {
        self.installed.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// 注入一次激活。首次立即回调；250ms 内的后续调用被丢弃。
    pub fn note_activation(&self, active: bool) {
        let now = Instant::now();
        let mut last = self.last_refresh.lock().expect("file manager observer");
        if let Some(previous) = *last
            && now.saturating_duration_since(previous) < DEBOUNCE
        {
            return;
        }
        *last = Some(now);
        drop(last);
        (self.refresher)(active);
    }
}
