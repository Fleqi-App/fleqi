//! Linux 窗口表面。
//!
//! 实际显示与隐藏由 Tauri 完成。这里不编造文件管理器几何，也不提供 macOS 玻璃材质。

use std::ffi::c_void;

use crate::frame::HostFrame;

pub fn file_manager_frame() -> HostFrame {
    super::x11::observe().frame
}

pub fn material_kind(_transparency: bool) -> &'static str {
    "solid"
}

/// # Safety
/// Linux 实现不读取 `window`。指针可以是空的。
pub unsafe fn apply_material(_: *mut c_void, _: bool, _: bool, _: i32) {}

/// # Safety
/// Linux 实现不读取窗口指针，也不改变原生窗口框架。
pub unsafe fn set_frame(_: *mut c_void, _: f64, _: f64, _: f64, _: f64) {}

/// # Safety
/// Linux 实现不读取窗口指针。
pub unsafe fn layout_composer(_: *mut c_void, _: f64) {}

/// # Safety
/// Linux 实现不读取窗口指针，也不显示或隐藏原生窗口。
pub unsafe fn present(_: *mut c_void, _: bool, _: bool, _: bool, _: bool) {}
