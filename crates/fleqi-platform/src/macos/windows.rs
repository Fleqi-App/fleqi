//! AppKit presentation and WindowServer observation. Native objects never cross IPC.
use std::ffi::c_void;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct FinderFrame {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub screen_left: f64,
    pub screen_top: f64,
    pub screen_right: f64,
    pub screen_bottom: f64,
    pub window_id: u64,
    pub foreground: i32,
    pub has_window: bool,
    pub mouse_down: bool,
}

unsafe extern "C" {
    fn fleqi_finder_frame() -> FinderFrame;
    fn fleqi_material_kind(transparency: bool) -> i32;
    fn fleqi_window_material(window: *mut c_void, composer: bool, transparency: bool, theme: i32);
    fn fleqi_window_frame(window: *mut c_void, x: f64, y: f64, width: f64, height: f64);
    fn fleqi_composer_layout(window: *mut c_void, extra: f64);
    fn fleqi_window_present(
        window: *mut c_void,
        visible: bool,
        focus: bool,
        return_to_finder: bool,
        reduce_motion: bool,
    );
}

pub fn finder_frame() -> FinderFrame {
    // SAFETY: returns an owned C value; the bridge manages its autorelease pool.
    unsafe { fleqi_finder_frame() }
}

pub fn material_kind(transparency: bool) -> &'static str {
    // SAFETY: reads platform capability and accessibility preferences only.
    match unsafe { fleqi_material_kind(transparency) } {
        2 => "glass",
        1 => "vibrancy",
        _ => "solid",
    }
}

/// # Safety
/// `window` is a live NSWindow supplied by Tauri; caller must be on the main thread.
pub unsafe fn apply_material(window: *mut c_void, composer: bool, transparency: bool, theme: i32) {
    // SAFETY: upheld by the caller; no native reference escapes this call.
    unsafe { fleqi_window_material(window, composer, transparency, theme) }
}

/// Update an undecorated composer's logical frame without an intermediate jump.
///
/// # Safety
/// `window` is a live NSWindow supplied by Tauri; caller must be on the main thread.
pub unsafe fn set_frame(window: *mut c_void, x: f64, y: f64, width: f64, height: f64) {
    // SAFETY: upheld by the caller; no native reference escapes this call.
    unsafe { fleqi_window_frame(window, x, y, width, height) }
}

/// # Safety
/// `window` must be a live composer NSWindow; call only on the AppKit main thread.
pub unsafe fn layout_composer(window: *mut c_void, extra: f64) {
    // SAFETY: guaranteed by the caller; the bridge reads and changes one native frame.
    unsafe { fleqi_composer_layout(window, extra) }
}

/// # Safety
/// `window` is a live NSWindow supplied by Tauri; caller must be on the main thread.
pub unsafe fn present(
    window: *mut c_void,
    visible: bool,
    focus: bool,
    return_to_finder: bool,
    reduce_motion: bool,
) {
    // SAFETY: upheld by the caller; AppKit retains the window for animation completion.
    unsafe { fleqi_window_present(window, visible, focus, return_to_finder, reduce_motion) }
}
