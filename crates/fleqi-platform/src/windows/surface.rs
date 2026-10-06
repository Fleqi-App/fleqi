//! Explorer 窗口状态来自 Win32；坐标为物理像素，DPI 缩放在宿主边界处理。
use crate::frame::HostFrame;

pub fn material_kind(_: bool) -> &'static str {
    "solid"
}

/// # Safety
/// `window` 必须是当前主线程拥有且仍存活的 HWND。
#[cfg(windows)]
pub unsafe fn show_without_activation(window: *mut std::ffi::c_void) {
    use windows::Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{SW_SHOWNOACTIVATE, ShowWindow},
    };
    let _ = unsafe { ShowWindow(HWND(window), SW_SHOWNOACTIVATE) };
}

#[cfg(not(windows))]
pub fn file_manager_frame() -> HostFrame {
    HostFrame::default()
}

#[cfg(windows)]
mod native {
    use super::*;
    use std::sync::atomic::{AtomicIsize, Ordering};
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    use windows::Win32::UI::WindowsAndMessaging::*;
    static LAST_EXPLORER: AtomicIsize = AtomicIsize::new(0);

    pub fn note_foreground(hwnd: HWND) {
        if unsafe { explorer(hwnd) } {
            LAST_EXPLORER.store(hwnd.0 as isize, Ordering::Release);
        }
    }

    pub fn explorer_window() -> Option<HWND> {
        // SAFETY: 只读桌面窗口，使用前每次验证句柄及窗口类，防止已关闭句柄被重用。
        unsafe {
            let foreground = GetForegroundWindow();
            note_foreground(foreground);
            let hwnd = HWND(LAST_EXPLORER.load(Ordering::Acquire) as *mut _);
            (explorer(hwnd) && IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool())
                .then_some(hwnd)
        }
    }
    unsafe fn explorer(hwnd: HWND) -> bool {
        let mut class = [0u16; 128];
        let count = unsafe { GetClassNameW(hwnd, &mut class) };
        count > 0
            && matches!(
                String::from_utf16_lossy(&class[..count as usize]).as_str(),
                "CabinetWClass" | "ExploreWClass"
            )
    }
    pub fn file_manager_frame() -> HostFrame {
        let Some(hwnd) = explorer_window() else {
            return HostFrame::default();
        };
        // SAFETY: 所有 API 只读取经过验证的窗口；关闭/切屏竞争由 API 失败降级为空状态。
        unsafe {
            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_err() {
                return HostFrame::default();
            }
            let mut monitor = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if !GetMonitorInfoW(
                MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST),
                &mut monitor,
            )
            .as_bool()
            {
                return HostFrame::default();
            }
            let foreground = GetForegroundWindow();
            let mut pid = 0;
            GetWindowThreadProcessId(foreground, Some(&mut pid));
            HostFrame {
                x: f64::from(rect.left),
                y: f64::from(rect.top),
                width: f64::from(rect.right - rect.left),
                height: f64::from(rect.bottom - rect.top),
                screen_left: f64::from(monitor.rcWork.left),
                screen_top: f64::from(monitor.rcWork.top),
                screen_right: f64::from(monitor.rcWork.right),
                screen_bottom: f64::from(monitor.rcWork.bottom),
                window_id: hwnd.0 as u64,
                foreground: if foreground == hwnd {
                    1
                } else if pid == std::process::id() {
                    2
                } else {
                    0
                },
                has_window: true,
                mouse_down: GetAsyncKeyState(1) < 0,
                scale: f64::from(GetDpiForWindow(hwnd).max(96)) / 96.0,
            }
        }
    }
}
#[cfg(windows)]
pub use native::{explorer_window, file_manager_frame, note_foreground};
