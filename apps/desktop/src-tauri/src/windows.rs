//! 按需窗口（architecture.md §12.2）：控制台与设置各有角色；关闭只销毁视图，宿主常驻。

use std::sync::{Arc, atomic::Ordering};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// 面板向主条上方扩展，并保持主条底边。只调整当前窗口，不触发表面状态机。
pub fn layout_composer(app: &AppHandle, extra_height: u32) -> Result<(), String> {
    let window = app
        .get_webview_window("composer")
        .ok_or("输入条窗口不存在")?;
    let state = app.state::<Arc<crate::state::AppState>>().inner().clone();
    app.run_on_main_thread(move || {
        if let Ok(native) = window.ns_window() {
            // SAFETY: the window is alive; read and update its frame in the same
            // AppKit turn so queued expansion requests cannot reuse stale bounds.
            unsafe {
                fleqi_platform::macos::windows::layout_composer(native, f64::from(extra_height))
            };
            state
                .composer_extra_height
                .store(extra_height, Ordering::Relaxed);
        }
    })
    .map_err(|error| error.to_string())
}

fn set_composer_frame(
    app: &AppHandle,
    window: tauri::WebviewWindow,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    app.run_on_main_thread(move || {
        if let Ok(native) = window.ns_window() {
            // SAFETY: the Tauri window is alive and AppKit is only used on its main thread.
            unsafe { fleqi_platform::macos::windows::set_frame(native, x, y, width, height) };
        }
    })
    .map_err(|error| error.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowRole {
    Console,
    Settings,
    /// M2 输入条窗口（贴附 Finder 的两行操作栏）。
    Composer,
}

impl WindowRole {
    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "console" => Some(WindowRole::Console),
            "settings" => Some(WindowRole::Settings),
            "composer" => Some(WindowRole::Composer),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            WindowRole::Console => "console",
            WindowRole::Settings => "settings",
            WindowRole::Composer => "composer",
        }
    }

    fn title(self) -> &'static str {
        match self {
            WindowRole::Console => "Fleqi 控制台",
            WindowRole::Settings => "Fleqi 设置",
            WindowRole::Composer => "Fleqi",
        }
    }

    /// 尺寸基线（ui-design.md §12.2、§4.1）：工作区 1080×760，设置 880×690，
    /// 输入条两行条 72 逻辑像素高、与 Finder 窗口等宽（最小 560）。
    fn size(self) -> (f64, f64, f64, f64) {
        match self {
            WindowRole::Console => (1080.0, 760.0, 640.0, 520.0),
            WindowRole::Settings => (880.0, 690.0, 640.0, 520.0),
            WindowRole::Composer => (760.0, 72.0, 560.0, 72.0),
        }
    }

    fn url(self, page: Option<&str>) -> WebviewUrl {
        let hash = match page {
            Some(p) => format!("index.html#/{}/{}", self.label(), p),
            None => format!("index.html#/{}", self.label()),
        };
        WebviewUrl::App(hash.into())
    }
}

/// 各窗口允许的页面路由（packages/ui/src/router.ts）；防止任意字符串进入 eval。
fn allowed_pages(role: WindowRole) -> &'static [&'static str] {
    match role {
        WindowRole::Console => &[
            "overview",
            "runs",
            "library",
            "permissions",
            "tools",
            "about",
        ],
        WindowRole::Settings => &[
            "general",
            "appearance",
            "models",
            "permissions",
            "files",
            "tasks",
            "about",
        ],
        WindowRole::Composer => &[],
    }
}

/// 已存在则聚焦；否则创建。`page` 在白名单内时直接落到对应页面路由。
pub fn open_window(app: &AppHandle, role: WindowRole, page: Option<&str>) -> Result<(), String> {
    if role != WindowRole::Composer {
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    }
    if let Some(window) = app.get_webview_window(role.label()) {
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
        return match page {
            Some(p) => navigate_window(app, role, p),
            None => Ok(()),
        };
    }
    if let Some(p) = page
        && !allowed_pages(role).contains(&p.split('?').next().unwrap_or(""))
    {
        return Err(format!("窗口 {} 不支持页面 {p}", role.label()));
    }
    let (width, height, min_width, min_height) = role.size();
    let mut builder = WebviewWindowBuilder::new(app, role.label(), role.url(page))
        .title(role.title())
        .inner_size(width, height)
        .min_inner_size(min_width, min_height)
        .center()
        .decorations(true)
        .transparent(true)
        .visible(false)
        .on_navigation(crate::commands::is_local_app_url)
        .on_page_load(move |window, payload| {
            if matches!(payload.event(), tauri::webview::PageLoadEvent::Finished) {
                apply_window_material(window.app_handle(), window.label());
                present_window(window.app_handle(), window.label(), true, true, false);
            }
        });
    #[cfg(target_os = "macos")]
    {
        builder = builder
            .title_bar_style(tauri::TitleBarStyle::Overlay)
            .hidden_title(true)
            .traffic_light_position(tauri::LogicalPosition::new(16.0, 18.0));
    }
    let window = builder
        .build()
        .map_err(|e| format!("创建窗口 {} 失败：{e}", role.label()))?;
    window.on_window_event({
        let app = app.clone();
        move |event| {
            if matches!(event, tauri::WindowEvent::Destroyed)
                && !["console", "settings"]
                    .iter()
                    .any(|label| *label != role.label() && app.get_webview_window(label).is_some())
            {
                let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }
        }
    });
    apply_window_material(app, role.label());
    Ok(())
}

/// 已存在的窗口内导航到指定页面（白名单校验）；用于"引导跳转"场景。
pub fn navigate_window(app: &AppHandle, role: WindowRole, page: &str) -> Result<(), String> {
    if !allowed_pages(role).contains(&page.split('?').next().unwrap_or("")) {
        return Err(format!("窗口 {} 不支持页面 {page}", role.label()));
    }
    let window = app
        .get_webview_window(role.label())
        .ok_or_else(|| format!("窗口 {} 未创建", role.label()))?;
    window.show().map_err(|e| e.to_string())?;
    let _ = window.set_focus();
    window
        .eval(format!(
            "window.location.hash = {};",
            serde_json::to_string(&format!("#/{}/{}", role.label(), page))
                .map_err(|e| e.to_string())?
        ))
        .map_err(|e| format!("导航窗口 {} 失败：{e}", role.label()))
}

/// 贴附范围（ui-design.md §4.1）：与 Finder 窗口等宽，最小 560。
const COMPOSER_MIN_WIDTH: f64 = 560.0;
/// 贴附间隙（ui-design.md §4.1"外侧下方 4"）：贴 Finder 窗口下缘外侧。
const COMPOSER_ATTACH_GAP: f64 = 4.0;

/// 计算 Finder 贴附几何：与 Finder 等宽（最小 560），外侧下方贴附；
/// 屏幕下缘放不下时贴 Finder 内侧底部（ui-design.md §4.1"内侧贴底"）。
/// 返回 (x, y, width)；无 Finder 窗口时返回 None。
fn composer_attach_geometry(height: f64) -> Option<(f64, f64, f64)> {
    let frame = fleqi_platform::macos::windows::finder_frame();
    if !frame.has_window {
        return None;
    }
    let x1 = frame.x;
    let y2 = frame.y + frame.height;
    let width = frame
        .width
        .max(COMPOSER_MIN_WIDTH)
        .min((frame.screen_right - frame.screen_left - 16.0).max(COMPOSER_MIN_WIDTH));
    let outer_y = y2 + COMPOSER_ATTACH_GAP;
    let fits_below = outer_y + height <= frame.screen_bottom - 8.0;
    let y = if fits_below {
        outer_y
    } else {
        y2 - COMPOSER_ATTACH_GAP - height
    };
    Some((
        x1.clamp(
            frame.screen_left + 8.0,
            (frame.screen_right - width - 8.0).max(frame.screen_left + 8.0),
        ),
        y.max(frame.screen_top + 8.0),
        width,
    ))
}

/// 显示输入条并贴附最前 Finder 窗口：与 Finder 等宽、贴窗口下缘外侧 4px，
/// 屏幕下缘放不下改为 Finder 内侧贴底；找不到 Finder 窗口时回退居中。
/// `focus=false` 为自动显示路径：不抢键盘焦点（ui-design.md §4 状态表）。
pub fn show_composer_attached(app: &AppHandle, focus: bool) -> Result<(), String> {
    if crate::updates::is_installing(app) {
        return Ok(());
    }
    if !focus && !finder_interaction_active(app) {
        app.state::<Arc<crate::state::AppState>>()
            .surface
            .system_hide();
        return Ok(());
    }
    let geometry = composer_attach_geometry(WindowRole::Composer.size().1);
    app.state::<Arc<crate::state::AppState>>()
        .composer_attached
        .store(geometry.is_some(), Ordering::Release);
    let (_, height, min_width, min_height) = WindowRole::Composer.size();
    if let Some(window) = app.get_webview_window(WindowRole::Composer.label()) {
        let extra = app
            .state::<Arc<crate::state::AppState>>()
            .composer_extra_height
            .load(Ordering::Relaxed);
        if let Some((x, y, width)) = geometry {
            let scale = window.scale_factor().map_err(|e| e.to_string())?;
            let current_size = window
                .inner_size()
                .map_err(|e| e.to_string())?
                .to_logical::<f64>(scale);
            let current_position = window
                .outer_position()
                .map_err(|e| e.to_string())?
                .to_logical::<f64>(scale);
            let full_height = height + f64::from(extra);
            let top = y - f64::from(extra);
            if (current_size.width - width).abs() > 0.5
                || (current_size.height - full_height).abs() > 0.5
                || (current_position.x - x).abs() > 0.5
                || (current_position.y - top).abs() > 0.5
            {
                set_composer_frame(app, window.clone(), x, top, width, full_height)?;
            }
        }
        present_window(app, "composer", true, focus, false);
        return Ok(());
    }
    let mut builder = WebviewWindowBuilder::new(
        app,
        WindowRole::Composer.label(),
        WindowRole::Composer.url(None),
    )
    .title(WindowRole::Composer.title())
    .min_inner_size(min_width, min_height)
    .skip_taskbar(true)
    .always_on_top(true)
    .focused(focus)
    .visible(false)
    .decorations(false)
    .transparent(true)
    .on_navigation(crate::commands::is_local_app_url)
    .on_page_load(move |window, payload| {
        if matches!(payload.event(), tauri::webview::PageLoadEvent::Finished) {
            apply_window_material(window.app_handle(), window.label());
            let state = window.app_handle().state::<Arc<crate::state::AppState>>();
            if state.surface.visibility() == fleqi_domain::surface::Visibility::Visible {
                present_window(window.app_handle(), window.label(), true, focus, false);
            }
        }
    });
    builder = match geometry {
        Some((x, y, width)) => builder.inner_size(width, height).position(x, y),
        None => builder.inner_size(760.0, height).center(),
    };
    builder
        .build()
        .map_err(|e| format!("创建输入条窗口失败：{e}"))?;
    apply_window_material(app, "composer");
    Ok(())
}

/// 隐藏输入条窗口（surface 状态与窗口表现同步；隐藏不销毁，避免重开丢会话状态）。
pub fn hide_composer(app: &AppHandle) {
    app.state::<Arc<crate::state::AppState>>()
        .composer_focus_requested
        .store(false, Ordering::Release);
    let user_hidden = app
        .state::<Arc<crate::state::AppState>>()
        .surface
        .visibility()
        == fleqi_domain::surface::Visibility::UserHidden;
    present_window(app, "composer", false, false, user_hidden);
}

pub fn finder_interaction_active(app: &AppHandle) -> bool {
    interaction_active(
        app,
        fleqi_platform::macos::windows::finder_frame().foreground,
    )
}

fn interaction_active(app: &AppHandle, foreground: i32) -> bool {
    // App activation and key-window notifications can arrive in different turns.
    // A focused management window must never qualify as Finder interaction.
    if ["console", "settings"].iter().any(|label| {
        app.get_webview_window(label)
            .is_some_and(|window| window.is_focused().unwrap_or(false))
    }) {
        return false;
    }
    foreground == 1
        || (foreground == 2
            && app
                .get_webview_window("composer")
                .is_some_and(|window| window.is_focused().unwrap_or(false)))
}

pub fn apply_window_material(app: &AppHandle, label: &str) {
    let Some(window) = app.get_webview_window(label) else {
        return;
    };
    let state = app.state::<Arc<crate::state::AppState>>();
    let settings = state.settings.snapshot().settings;
    let composer = label == "composer";
    let _ = app.run_on_main_thread(move || {
        if let Ok(native) = window.ns_window() {
            let theme = match settings.theme {
                fleqi_domain::settings::Theme::Light => 1,
                fleqi_domain::settings::Theme::Dark => 2,
                _ => 0,
            };
            // SAFETY: Tauri owns this live window and this closure runs on AppKit's main thread.
            unsafe {
                fleqi_platform::macos::windows::apply_material(
                    native,
                    composer,
                    settings.transparency,
                    theme,
                );
            }
            let kind = fleqi_platform::macos::windows::material_kind(settings.transparency);
            let _ = window.eval(format!(
                "document.documentElement.dataset.material = '{kind}';"
            ));
        }
    });
}

fn present_window(
    app: &AppHandle,
    label: &str,
    visible: bool,
    focus: bool,
    return_to_finder: bool,
) {
    let Some(window) = app.get_webview_window(label) else {
        return;
    };
    let app_after_present = app.clone();
    // Finder dragging/focus loss is temporary: hide immediately so an old bar
    // cannot fade over a moving window. Explicit user hiding keeps its fade.
    let immediate_hide = !visible && !return_to_finder;
    let reduce = immediate_hide
        || app
            .state::<Arc<crate::state::AppState>>()
            .settings
            .snapshot()
            .settings
            .motion_mode
            == fleqi_domain::settings::MotionMode::Reduce;
    let _ = app.run_on_main_thread(move || {
        if let Ok(native) = window.ns_window() {
            // SAFETY: live Tauri-owned NSWindow, used only on the main thread.
            unsafe {
                fleqi_platform::macos::windows::present(
                    native,
                    visible,
                    focus,
                    return_to_finder,
                    reduce,
                );
            }
            if focus {
                app_after_present
                    .state::<Arc<crate::state::AppState>>()
                    .composer_focus_requested
                    .store(false, Ordering::Release);
            }
        }
    });
}

/// Fast geometry decisions. One owner controls temporary hiding and restoration;
/// returning focus can never restore a user-hidden display cycle.
#[derive(Default)]
pub struct FinderGeometryWatch {
    last: Option<(u64, f64, f64, f64, f64)>,
    moving: bool,
    stable_ticks: u32,
}

#[derive(Debug, PartialEq, Eq)]
enum FollowAction {
    None,
    Hide,
    Restore,
}

impl FinderGeometryWatch {
    fn sample(
        &mut self,
        frame: fleqi_platform::macos::windows::FinderFrame,
        active: bool,
        visibility: fleqi_domain::surface::Visibility,
    ) -> FollowAction {
        use fleqi_domain::surface::Visibility;
        if visibility == Visibility::UserHidden {
            *self = Self::default();
            return FollowAction::None;
        }
        if !active || !frame.has_window {
            *self = Self::default();
            return FollowAction::Hide;
        }
        let current = (frame.window_id, frame.x, frame.y, frame.width, frame.height);
        let moved = self.last.is_some_and(|previous| {
            previous.0 != current.0
                || (previous.1 - current.1).abs() > 0.5
                || (previous.2 - current.2).abs() > 0.5
                || (previous.3 - current.3).abs() > 0.5
                || (previous.4 - current.4).abs() > 0.5
        });
        self.last = Some(current);
        if moved {
            self.moving = true;
            self.stable_ticks = 0;
            return FollowAction::Hide;
        }
        if self.moving {
            if frame.mouse_down {
                self.stable_ticks = 0;
                return FollowAction::Hide;
            }
            self.stable_ticks += 1;
            if self.stable_ticks < 2 {
                return FollowAction::None;
            }
            self.moving = false;
        }
        if visibility == Visibility::TemporarilyHidden {
            FollowAction::Restore
        } else {
            FollowAction::None
        }
    }

    pub fn tick(&mut self, app: &AppHandle, state: &crate::state::AppState) {
        let frame = fleqi_platform::macos::windows::finder_frame();
        #[cfg(feature = "desktop-test")]
        if std::env::var_os("FLEQI_DEBUG_FINDER").is_some() {
            static TRACE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            if TRACE.fetch_add(1, Ordering::Relaxed).is_multiple_of(20) {
                state.logger.log(
                    "debug",
                    "finder.trace",
                    &[(
                        "message",
                        &format!("{frame:?}; surface={:?}", state.surface.visibility()),
                    )],
                );
            }
        }
        let active = state.composer_focus_requested.load(Ordering::Acquire)
            || interaction_active(app, frame.foreground);
        if active
            && frame.has_window
            && state.surface.visibility() == fleqi_domain::surface::Visibility::UserHidden
            && !state.surface.auto_show_suppressed()
            && state.settings.snapshot().settings.activation
                == fleqi_domain::settings::Activation::FollowFinder
            && let Some(snapshot) = state.context.latest()
            && snapshot.source_window_id == Some(frame.window_id)
        {
            // Reevaluate after focus settles even if the context itself did not change.
            state.surface.on_context_changed(&snapshot);
        }
        if !frame.has_window && !state.composer_attached.load(Ordering::Acquire) && active {
            return; // Explicit manual entry may be detached and show a directory/permission guide.
        }
        match self.sample(frame, active, state.surface.visibility()) {
            FollowAction::Hide => state.surface.system_hide(),
            FollowAction::Restore => state.surface.system_restore(),
            FollowAction::None => {}
        }
    }
}

#[cfg(test)]
mod follow_tests {
    use super::*;
    use fleqi_domain::surface::Visibility::*;
    use fleqi_platform::macos::windows::FinderFrame;

    #[test]
    fn movement_hides_immediately_and_restores_after_release_without_stealing_other_apps() {
        let mut watch = FinderGeometryWatch::default();
        let mut frame = FinderFrame {
            has_window: true,
            foreground: 1,
            window_id: 42,
            width: 800.0,
            height: 600.0,
            ..FinderFrame::default()
        };
        assert_eq!(watch.sample(frame, true, Visible), FollowAction::None);
        frame.x = 20.0;
        frame.mouse_down = true;
        assert_eq!(watch.sample(frame, true, Visible), FollowAction::Hide);
        for _ in 0..20 {
            assert_eq!(
                watch.sample(frame, true, TemporarilyHidden),
                FollowAction::Hide
            );
        }
        frame.mouse_down = false;
        assert_eq!(
            watch.sample(frame, true, TemporarilyHidden),
            FollowAction::None
        );
        assert_eq!(
            watch.sample(frame, true, TemporarilyHidden),
            FollowAction::Restore
        );
        assert_eq!(
            watch.sample(frame, false, TemporarilyHidden),
            FollowAction::Hide
        );
        assert_eq!(
            watch.sample(frame, false, TemporarilyHidden),
            FollowAction::Hide
        );
        assert_eq!(watch.sample(frame, true, UserHidden), FollowAction::None);
        frame.has_window = false;
        assert_eq!(watch.sample(frame, true, Visible), FollowAction::Hide);
    }
}
