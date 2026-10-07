//! 文件管理器窗口几何。macOS 的 C 桥保持 `repr(C)`；宿主使用这份拥有值。

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HostFrame {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub screen_left: f64,
    pub screen_top: f64,
    pub screen_right: f64,
    pub screen_bottom: f64,
    pub window_id: u64,
    /// 1 = 文件管理器在前台，2 = 输入条在前台，0 = 其他。
    pub foreground: i32,
    pub has_window: bool,
    pub mouse_down: bool,
    /// Windows 坐标为物理像素；其它平台保持原有坐标，比例为 1。
    pub scale: f64,
}
