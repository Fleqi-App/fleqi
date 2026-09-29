//! 资源管理器目录。LocationURL 的解析是纯函数；COM 读取只在 Windows 上执行。

use fleqi_application::ports::{ContextPort, DirectoryPick, RawContext, RawPath};
use fleqi_domain::context::{ContextAvailability, PathKind, ViewKind};
use std::path::PathBuf;
use std::sync::Arc;

use crate::scheduling::MainThreadExecutor;

use super::picker::pick_directory_blocking;

/// 固定脚本：枚举 Shell.Application 窗口并逐行打印 LocationURL。不含运行期插值。
pub const EXPLORER_LOCATIONS_SCRIPT: &str = r#"
$OutputEncoding = [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false
$ErrorActionPreference = 'Stop'
$shell = New-Object -ComObject Shell.Application
foreach ($window in @($shell.Windows())) {
    try {
        $location = $window.LocationURL
        if ($location) {
            Write-Output $location
        }
    } catch {
    }
}
exit 0
"#;

pub struct WindowsContextPort {
    main: Arc<dyn MainThreadExecutor>,
}

impl WindowsContextPort {
    pub fn new(main: Arc<dyn MainThreadExecutor>) -> Self {
        Self { main }
    }
}

/// 把资源管理器 LocationURL 转成 Windows 路径。
///
/// 接受 `file:///C:/dir/My%20Folder`（以及 localhost 形式）。拒绝 http(s)、`..` 段和空路径。
pub fn location_to_path(text: &str) -> Option<PathBuf> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let (scheme, rest) = text.split_once(':')?;
    if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") {
        return None;
    }
    if !scheme.eq_ignore_ascii_case("file") {
        return None;
    }
    let rest = rest.strip_prefix("//")?;
    let (host, path) = match rest.split_once('/') {
        Some((host, path)) => (host, path),
        None => (rest, ""),
    };
    let path = path.split(['?', '#']).next().unwrap_or(path);
    let decoded = percent_decode(path)?;
    if decoded.chars().any(char::is_control) {
        return None;
    }
    let local = host.is_empty() || host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1";
    if local {
        drive_path(&decoded)
    } else {
        unc_path(host, &decoded)
    }
}

/// 解析脚本标准输出。第一个有效目录即视为前台窗口；选区留空。
pub fn context_from_explorer_output(stdout: &str) -> RawContext {
    match stdout.lines().find_map(location_to_path) {
        Some(native) => RawContext {
            source_window_id: None,
            directory: Some(RawPath {
                native,
                kind: PathKind::Directory,
            }),
            selection: Vec::new(),
            view_kind: Some(ViewKind::Physical),
            unavailable: None,
        },
        None => RawContext {
            unavailable: Some(ContextAvailability::NoDirectory {
                reason: "资源管理器没有打开文件夹".into(),
            }),
            ..RawContext::default()
        },
    }
}

fn drive_path(decoded: &str) -> Option<PathBuf> {
    let segments = clean_segments(decoded)?;
    let drive = *segments.first()?;
    if !is_drive(drive) {
        return None;
    }
    let mut path = String::from(drive);
    if segments.len() == 1 {
        path.push('\\');
    } else {
        for segment in &segments[1..] {
            path.push('\\');
            path.push_str(segment);
        }
    }
    Some(PathBuf::from(path))
}

fn unc_path(host: &str, decoded: &str) -> Option<PathBuf> {
    if host.is_empty()
        || host == "."
        || host == ".."
        || host
            .chars()
            .any(|ch| ch.is_control() || ch == '\\' || ch == '/')
    {
        return None;
    }
    let segments = clean_segments(decoded)?;
    let mut path = String::from("\\\\");
    path.push_str(host);
    for segment in segments {
        path.push('\\');
        path.push_str(segment);
    }
    Some(PathBuf::from(path))
}

fn clean_segments(decoded: &str) -> Option<Vec<&str>> {
    if decoded.is_empty() {
        return None;
    }
    let mut segments: Vec<&str> = decoded.split(['/', '\\']).collect();
    if segments.last().copied() == Some("") {
        segments.pop();
    }
    if segments.is_empty()
        || segments
            .iter()
            .any(|segment| segment.is_empty() || *segment == "." || *segment == "..")
    {
        return None;
    }
    Some(segments)
}

fn is_drive(segment: &str) -> bool {
    let bytes = segment.as_bytes();
    bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
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
            let high = hex_nibble(bytes[index + 1])?;
            let low = hex_nibble(bytes[index + 2])?;
            out.push((high << 4) | low);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

impl ContextPort for WindowsContextPort {
    fn capture(&self) -> RawContext {
        #[cfg(target_os = "windows")]
        {
            let _ = self;
            capture_explorer()
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = self;
            RawContext {
                unavailable: Some(ContextAvailability::NoDirectory {
                    reason: "资源管理器仅在 Windows 上读取".into(),
                }),
                ..RawContext::default()
            }
        }
    }

    fn pick_directory(&self) -> DirectoryPick {
        pick_directory_blocking(self.main.as_ref())
    }
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn capture_explorer() -> RawContext {
    match super::process::run_fixed_script(EXPLORER_LOCATIONS_SCRIPT, &[], false, true) {
        Ok(output) if output.success => context_from_explorer_output(&output.stdout),
        Ok(_) => failed_context("无法读取资源管理器窗口"),
        Err(_) => failed_context("无法启动资源管理器读取"),
    }
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn failed_context(message: &str) -> RawContext {
    RawContext {
        unavailable: Some(ContextAvailability::Failed {
            message: message.to_owned(),
        }),
        ..RawContext::default()
    }
}
