//! 当前编译目标的文件管理器几何与材质名称。
//!
//! macOS 读取 Finder；Windows / Linux 使用各自的 surface 模块。没有窗口时
//! `has_window` 为 false，宿主回退到居中输入条，不编造贴附坐标。

use crate::frame::HostFrame;

#[cfg(target_os = "macos")]
pub fn file_manager_frame() -> HostFrame {
    let frame = crate::macos::windows::finder_frame();
    HostFrame {
        x: frame.x,
        y: frame.y,
        width: frame.width,
        height: frame.height,
        screen_left: frame.screen_left,
        screen_top: frame.screen_top,
        screen_right: frame.screen_right,
        screen_bottom: frame.screen_bottom,
        window_id: frame.window_id,
        foreground: frame.foreground,
        has_window: frame.has_window,
        mouse_down: frame.mouse_down,
    }
}

#[cfg(target_os = "linux")]
pub fn file_manager_frame() -> HostFrame {
    crate::linux::surface::file_manager_frame()
}

#[cfg(target_os = "windows")]
pub fn file_manager_frame() -> HostFrame {
    crate::windows::surface::file_manager_frame()
}

#[cfg(target_os = "macos")]
pub fn material_kind(transparency: bool) -> &'static str {
    crate::macos::windows::material_kind(transparency)
}

#[cfg(target_os = "linux")]
pub fn material_kind(transparency: bool) -> &'static str {
    crate::linux::surface::material_kind(transparency)
}

#[cfg(target_os = "windows")]
pub fn material_kind(transparency: bool) -> &'static str {
    crate::windows::surface::material_kind(transparency)
}
