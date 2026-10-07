//! Explorer 激活来自 WinEvent；退出时释放钩子，不合成虚假的激活回执。
use fleqi_application::dto::AppError;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
pub type ContextRefresher = Box<dyn Fn(bool) + Send + Sync>;

pub struct FileManagerActivationObserver {
    #[cfg(windows)]
    refresher: ContextRefresher,
    installed: AtomicBool,
}
impl FileManagerActivationObserver {
    pub fn new(refresher: ContextRefresher) -> Arc<Self> {
        #[cfg(not(windows))]
        let _ = refresher;
        Arc::new(Self {
            #[cfg(windows)]
            refresher,
            installed: AtomicBool::new(false),
        })
    }
    pub fn install(self: &Arc<Self>) -> Result<(), AppError> {
        if self.installed.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        #[cfg(windows)]
        {
            let weak = Arc::downgrade(self);
            let (ready, receive) = std::sync::mpsc::channel();
            std::thread::spawn(move || watch(weak, ready));
            if receive.recv_timeout(std::time::Duration::from_secs(2)).ok() != Some(true) {
                self.installed.store(false, Ordering::Release);
                return Err(AppError::unavailable("无法注册 Explorer 前台观察"));
            }
            Ok(())
        }
        #[cfg(not(windows))]
        {
            self.installed.store(false, Ordering::Release);
            Err(AppError::unavailable("Explorer 观察仅在 Windows 上可用"))
        }
    }
}

#[cfg(windows)]
fn watch(
    owner: std::sync::Weak<FileManagerActivationObserver>,
    ready: std::sync::mpsc::Sender<bool>,
) {
    use std::sync::atomic::AtomicU64;
    use windows::Win32::UI::{Accessibility::*, WindowsAndMessaging::*};
    static CHANGES: AtomicU64 = AtomicU64::new(0);
    unsafe extern "system" fn changed(
        _: HWINEVENTHOOK,
        _: u32,
        hwnd: windows::Win32::Foundation::HWND,
        _: i32,
        _: i32,
        _: u32,
        _: u32,
    ) {
        super::surface::note_foreground(hwnd);
        CHANGES.fetch_add(1, Ordering::Release);
    }
    // SAFETY: 线程保留消息泵和回调，钩子释放后才退出；回调不访问 COM 或 UI。
    unsafe {
        let hook = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(changed),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        );
        if hook.0.is_null() {
            let _ = ready.send(false);
            return;
        }
        if ready.send(true).is_err() {
            let _ = UnhookWinEvent(hook);
            return;
        }
        let mut seen = u64::MAX;
        loop {
            let mut message = MSG::default();
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            let Some(observer) = owner.upgrade() else {
                break;
            };
            let current = CHANGES.load(Ordering::Acquire);
            if current != seen {
                seen = current;
                let frame = super::surface::file_manager_frame();
                (observer.refresher)(frame.has_window && matches!(frame.foreground, 1 | 2));
            }
            drop(observer);
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let _ = UnhookWinEvent(hook);
    }
}
