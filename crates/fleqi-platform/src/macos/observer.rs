//! Finder 观察器（architecture.md §12.4）：NSWorkspace 通知驱动——Finder 激活触发
//! 上下文刷新（去抖）；App 激活/失焦与窗口移动事件路由到 SurfaceService
//! （自动显示/暂隐/恢复）随 M4 贴附定位一并交付。

use fleqi_application::dto::AppError;
use objc2::rc::Retained;
use objc2_foundation::{NSNotification, NSNotificationName, NSString};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const FINDER_BUNDLE_ID: &str = "com.apple.finder";
const DEBOUNCE: Duration = Duration::from_millis(250);

pub type ContextRefresher = Box<dyn Fn(bool) + Send + Sync>;

struct ObserverState {
    last_refresh: Instant,
}

pub struct FinderActivationObserver {
    state: Mutex<ObserverState>,
    refresher: ContextRefresher,
    installed: AtomicBool,
}

impl FinderActivationObserver {
    pub fn new(refresher: ContextRefresher) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(ObserverState {
                last_refresh: Instant::now(),
            }),
            refresher,
            installed: AtomicBool::new(false),
        })
    }

    /// 启动观察（幂等）。Finder 激活（去抖 250ms）触发 refresher。
    pub fn install(self: &Arc<Self>) -> Result<(), AppError> {
        if self.installed.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        let weak = Arc::downgrade(self);
        let block = block2::RcBlock::new(move |notification: std::ptr::NonNull<NSNotification>| {
            let Some(observer) = weak.upgrade() else {
                return;
            };
            observer.on_notification(unsafe { notification.as_ref() });
        });
        // SAFETY: 宿主在主线程 setup 时调用；block 生命周期由本函数持有。
        // NSWorkspace 通知走 workspace 自己的通知中心（AppKit），不在默认中心。
        unsafe {
            objc2_app_kit::NSWorkspace::sharedWorkspace()
                .notificationCenter()
                .addObserverForName_object_queue_usingBlock(
                    Some(&*NSNotificationName::from_str(
                        "NSWorkspaceDidActivateApplicationNotification",
                    )),
                    None,
                    None,
                    &block,
                );
        }
        Ok(())
    }

    /// 供测试直接注入通知处理（不依赖真实 Finder）。
    pub fn on_notification(&self, notification: &NSNotification) {
        let name = notification.name().to_string();
        if name != "NSWorkspaceDidActivateApplicationNotification" {
            return;
        }
        // SAFETY: userInfo 由框架提供；读取 NSWorkspaceApplicationKey 的 bundleIdentifier。
        // SAFETY: userInfo 与该键的值（NSRunningApplication）由框架提供；
        // bundleIdentifier 读取经 msg_send 校验返回值。
        let bundle = notification
            .userInfo()
            .and_then(|dict| {
                let key = NSString::from_str("NSWorkspaceApplicationKey");
                dict.objectForKey(&key)
            })
            .and_then(|value| {
                // SAFETY: 该键的值为 NSRunningApplication。
                let app: Retained<objc2_app_kit::NSRunningApplication> =
                    unsafe { Retained::cast_unchecked(value) };
                if app.processIdentifier() == std::process::id() as i32 {
                    Some("__fleqi_self__".to_owned())
                } else {
                    app.bundleIdentifier().map(|s| s.to_string())
                }
            });
        if bundle.as_deref() != Some(FINDER_BUNDLE_ID) {
            if bundle.as_deref() != Some("__fleqi_self__") {
                (self.refresher)(false);
            }
            return;
        }
        let now = Instant::now();
        let mut state = self.state.lock().expect("observer");
        if now.saturating_duration_since(state.last_refresh) >= DEBOUNCE {
            state.last_refresh = now;
            drop(state);
            (self.refresher)(true);
        }
    }
}

/// 供测试确认常量未随运行期变化。
pub fn finder_bundle_id() -> &'static str {
    FINDER_BUNDLE_ID
}
