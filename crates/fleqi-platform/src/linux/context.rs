//! Linux 上下文端口。
//!
//! 只根据会话环境变量区分 X11 / Wayland，不读取文件管理器窗口，也不编造目录。

pub use fleqi_application::ports::{ContextPort, DirectoryPick, RawContext, RawPath};
pub use fleqi_domain::context::{ContextAvailability, PathKind, ViewKind};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use crate::scheduling::MainThreadExecutor;

use super::picker::pick_directory_blocking;

/// 当前桌面会话种类。不是 Finder 窗口分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    X11,
    Wayland,
    Unknown,
}

pub struct LinuxContextPort {
    main: Arc<dyn MainThreadExecutor>,
}

impl LinuxContextPort {
    pub fn new(main: Arc<dyn MainThreadExecutor>) -> Self {
        Self { main }
    }
}

impl ContextPort for LinuxContextPort {
    fn capture(&self) -> RawContext {
        let observed = super::x11::observe();
        if let Some(directory) = observed.directory.filter(|path| path.is_dir()) {
            return RawContext {
                source_window_id: Some(observed.frame.window_id),
                directory: Some(RawPath {
                    native: directory,
                    kind: PathKind::Directory,
                }),
                view_kind: Some(ViewKind::Physical),
                unavailable: None,
                ..RawContext::default()
            };
        }
        let kind = session_kind(
            env_value("XDG_SESSION_TYPE").as_deref(),
            env_value("WAYLAND_DISPLAY").as_deref(),
            env_value("DISPLAY").as_deref(),
        );
        let reason = match (&observed.manager, &observed.title) {
            (Some(manager), Some(title)) if !title.is_empty() => format!(
                "已看到 {manager} 窗口「{title}」，但标题对不上一个现存目录。此桌面不会被当作 Finder；请选择文件夹。"
            ),
            (Some(manager), _) => {
                format!("已看到 {manager} 窗口，但读不到可执行目录。请选择文件夹。")
            }
            _ => no_directory_reason(kind),
        };
        RawContext {
            source_window_id: observed
                .frame
                .has_window
                .then_some(observed.frame.window_id),
            unavailable: Some(ContextAvailability::NoDirectory { reason }),
            ..RawContext::default()
        }
    }

    fn pick_directory(&self) -> DirectoryPick {
        pick_directory_blocking(self.main.as_ref())
    }
}

/// 从会话环境变量分类桌面。`XDG_SESSION_TYPE` 为 `x11` 或 `wayland` 时优先；
/// `tty` 视为未知。否则非空的 `WAYLAND_DISPLAY` 优先于 `DISPLAY`。
pub fn session_kind(
    xdg_session_type: Option<&str>,
    wayland_display: Option<&str>,
    display: Option<&str>,
) -> SessionKind {
    match normalized(xdg_session_type).as_deref() {
        Some("wayland") => SessionKind::Wayland,
        Some("x11") => SessionKind::X11,
        Some("tty") => SessionKind::Unknown,
        _ => {
            if present(wayland_display) {
                SessionKind::Wayland
            } else if present(display) {
                SessionKind::X11
            } else {
                SessionKind::Unknown
            }
        }
    }
}

fn no_directory_reason(kind: SessionKind) -> String {
    let kind_text = match kind {
        SessionKind::X11 => "当前会话是 X11",
        SessionKind::Wayland => "当前会话是 Wayland",
        SessionKind::Unknown => "当前会话类型未知",
    };
    format!("{kind_text}。此桌面的文件管理器不会被当作 Finder 读取。")
}

fn env_value(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn normalized(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
}

fn present(value: Option<&str>) -> bool {
    value.map(str::trim).is_some_and(|value| !value.is_empty())
}

/// 把 `file://` URL 或绝对路径变成路径。
///
/// 百分号解码在检查 `..` 之前进行。拒绝非 `file` 协议、主机名不是空或
/// `localhost` 的 URL，以及解码后仍含 `..` 的路径。没有实时文件管理器时，
/// `capture` 不会调用本函数去猜测目录。
pub fn location_to_path(text: &str) -> Option<PathBuf> {
    let text = text.trim();
    if text.is_empty() || text.contains('\0') {
        return None;
    }
    let owned;
    let path_text = if has_file_scheme(text) {
        owned = file_url_to_absolute(text)?;
        owned.as_str()
    } else if text.contains("://") {
        return None;
    } else {
        text
    };
    if path_text.contains('\0') || path_escapes(path_text) {
        return None;
    }
    let path = PathBuf::from(path_text);
    if path.is_absolute() { Some(path) } else { None }
}

fn has_file_scheme(text: &str) -> bool {
    text.len() >= 7 && text.as_bytes()[..7].eq_ignore_ascii_case(b"file://")
}

fn file_url_to_absolute(url: &str) -> Option<String> {
    let rest = &url[7..];
    let raw = if let Some(path) = rest.strip_prefix('/') {
        format!("/{path}")
    } else {
        let (host, path) = rest.split_once('/')?;
        if !host.eq_ignore_ascii_case("localhost") {
            return None;
        }
        format!("/{path}")
    };
    let raw = raw.split(['?', '#']).next().unwrap_or(raw.as_str());
    percent_decode(raw)
}

fn path_escapes(text: &str) -> bool {
    Path::new(text)
        .components()
        .any(|component| matches!(component, Component::ParentDir))
}

fn percent_decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return None;
            }
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok()?;
            let value = u8::from_str_radix(hex, 16).ok()?;
            if value == 0 {
                return None;
            }
            out.push(value);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}
