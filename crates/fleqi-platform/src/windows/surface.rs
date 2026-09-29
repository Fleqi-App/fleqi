//! Windows 不报告资源管理器窗口几何，材质保持不透明。
//! 这些函数是与其它平台对齐的空操作，不读取指针。

use std::ffi::c_void;

use crate::frame::HostFrame;

pub fn file_manager_frame() -> HostFrame {
    HostFrame::default()
}

pub fn material_kind(transparency: bool) -> &'static str {
    let _ = transparency;
    "solid"
}

/// # Safety
/// `window` 与 `composer` 由宿主在 UI 线程传入。本实现不解引用。
pub unsafe fn apply_material(window: *mut c_void, composer: bool, transparency: bool, theme: i32) {
    let _ = (window, composer, transparency, theme);
}

/// # Safety
/// `window` 由宿主在 UI 线程传入。本实现不改窗口框，也不解引用。
pub unsafe fn set_frame(window: *mut c_void, x: f64, y: f64, width: f64, height: f64) {
    let _ = (window, x, y, width, height);
}

/// # Safety
/// `window` 由宿主在 UI 线程传入。本实现不布局合成器，也不解引用。
pub unsafe fn layout_composer(window: *mut c_void, extra: f64) {
    let _ = (window, extra);
}

/// # Safety
/// `window` 由宿主在 UI 线程传入。本实现不显示窗口，也不解引用。
pub unsafe fn present(
    window: *mut c_void,
    visible: bool,
    focus: bool,
    return_to_finder: bool,
    reduce_motion: bool,
) {
    let _ = (window, visible, focus, return_to_finder, reduce_motion);
}
