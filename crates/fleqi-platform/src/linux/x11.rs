//! X11 文件管理器窗口。只接受能对应到真实目录的窗口，不把桌面名当成 Finder。
//!
//! 几何来自 `xwininfo`，类名与标题来自 `xprop`。目录只在标题本身是绝对路径，
//! 或进程参数里有与标题同名的现有目录时成立。用户在文件管理器里再次导航后，
//! 若标题不再能对上那个参数，就报告没有目录，而不是沿用过期路径。

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::frame::HostFrame;

const CACHE_TTL: Duration = Duration::from_millis(200);
const MIN_WINDOW: f64 = 80.0;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ObservedDesktop {
    pub frame: HostFrame,
    pub directory: Option<PathBuf>,
    pub manager: Option<String>,
    pub title: Option<String>,
}

pub fn observe() -> ObservedDesktop {
    if std::env::var_os("DISPLAY").is_none() {
        return ObservedDesktop::default();
    }
    let mut cache = cache().lock().expect("x11 cache");
    if let Some((at, observed)) = cache.as_ref()
        && at.elapsed() < CACHE_TTL
    {
        return observed.clone();
    }
    let observed = probe();
    *cache = Some((Instant::now(), observed.clone()));
    observed
}

fn cache() -> &'static std::sync::Mutex<Option<(Instant, ObservedDesktop)>> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<Option<(Instant, ObservedDesktop)>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(None))
}

fn probe() -> ObservedDesktop {
    let root = run(
        "xprop",
        &[
            "-root",
            "_NET_ACTIVE_WINDOW",
            "_NET_CLIENT_LIST",
            "_NET_WORKAREA",
        ],
    );
    let Some(root) = root else {
        return ObservedDesktop::default();
    };
    let active = parse_active_window(&root);
    let work = parse_workarea(&root).unwrap_or((0.0, 0.0, 0.0, 0.0));
    let mut clients = parse_client_list(&root);
    if let Some(id) = active {
        clients.retain(|candidate| *candidate != id);
        clients.insert(0, id);
    }
    for id in clients {
        let Some(props) = run(
            "xprop",
            &[
                "-id",
                &format!("{id:#x}"),
                "WM_CLASS",
                "_NET_WM_NAME",
                "_NET_WM_PID",
            ],
        ) else {
            continue;
        };
        let Some((instance, class)) = parse_wm_class(&props) else {
            continue;
        };
        if !is_file_manager(&instance, &class) {
            continue;
        }
        let Some(geometry) =
            run("xwininfo", &["-id", &format!("{id:#x}")]).and_then(|text| parse_xwininfo(&text))
        else {
            continue;
        };
        if geometry.2 < MIN_WINDOW || geometry.3 < MIN_WINDOW {
            continue;
        }
        let title = parse_wm_name(&props).unwrap_or_default();
        let pid = parse_wm_pid(&props);
        let args = pid.map(process_args).unwrap_or_default();
        let directory = directory_from_manager(&title, &args, Path::is_dir);
        let manager = if class.is_empty() { instance } else { class };
        return ObservedDesktop {
            frame: HostFrame {
                x: geometry.0,
                y: geometry.1,
                width: geometry.2,
                height: geometry.3,
                screen_left: work.0,
                screen_top: work.1,
                screen_right: work.0 + work.2,
                screen_bottom: work.1 + work.3,
                window_id: id,
                foreground: if active == Some(id) { 1 } else { 0 },
                has_window: true,
                mouse_down: false,
                scale: 1.0,
            },
            directory,
            manager: Some(manager),
            title: Some(title),
        };
    }
    ObservedDesktop::default()
}

fn run(program: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

fn process_args(pid: u32) -> Vec<String> {
    let Ok(bytes) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
        return Vec::new();
    };
    bytes
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect()
}

pub fn parse_active_window(text: &str) -> Option<u64> {
    let line = text
        .lines()
        .find(|line| line.contains("_NET_ACTIVE_WINDOW"))?;
    parse_window_ids(line).into_iter().find(|id| *id != 0)
}

pub fn parse_client_list(text: &str) -> Vec<u64> {
    text.lines()
        .find(|line| line.contains("_NET_CLIENT_LIST"))
        .map(parse_window_ids)
        .unwrap_or_default()
}

fn parse_window_ids(line: &str) -> Vec<u64> {
    line.split(|ch: char| !ch.is_ascii_hexdigit() && ch != 'x')
        .filter(|token| token.starts_with("0x") || token.starts_with("0X"))
        .filter_map(|token| {
            u64::from_str_radix(token.trim_start_matches("0x").trim_start_matches("0X"), 16).ok()
        })
        .collect()
}

pub fn parse_workarea(text: &str) -> Option<(f64, f64, f64, f64)> {
    let line = text.lines().find(|line| line.contains("_NET_WORKAREA"))?;
    let numbers = line
        .split(|ch: char| !ch.is_ascii_digit())
        .filter(|token| !token.is_empty())
        .filter_map(|token| token.parse::<f64>().ok())
        .collect::<Vec<_>>();
    if numbers.len() < 4 {
        return None;
    }
    Some((numbers[0], numbers[1], numbers[2], numbers[3]))
}

pub fn parse_wm_class(text: &str) -> Option<(String, String)> {
    let line = text.lines().find(|line| line.contains("WM_CLASS"))?;
    let mut quoted = quoted_strings(line);
    let instance = quoted.next()?;
    let class = quoted.next().unwrap_or_else(|| instance.clone());
    Some((instance, class))
}

pub fn parse_wm_name(text: &str) -> Option<String> {
    let line = text
        .lines()
        .find(|line| line.contains("_NET_WM_NAME") || line.contains("WM_NAME"))?;
    quoted_strings(line).next()
}

pub fn parse_wm_pid(text: &str) -> Option<u32> {
    let line = text.lines().find(|line| line.contains("_NET_WM_PID"))?;
    line.split(|ch: char| !ch.is_ascii_digit())
        .filter(|token| !token.is_empty())
        .filter_map(|token| token.parse().ok())
        .next()
}

pub fn parse_xwininfo(text: &str) -> Option<(f64, f64, f64, f64)> {
    let mut x = None;
    let mut y = None;
    let mut width = None;
    let mut height = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("Absolute upper-left X:") {
            x = value.trim().parse().ok();
        } else if let Some(value) = trimmed.strip_prefix("Absolute upper-left Y:") {
            y = value.trim().parse().ok();
        } else if let Some(value) = trimmed.strip_prefix("Width:") {
            width = value.trim().parse().ok();
        } else if let Some(value) = trimmed.strip_prefix("Height:") {
            height = value.trim().parse().ok();
        }
    }
    Some((x?, y?, width?, height?))
}

pub fn is_file_manager(instance: &str, class: &str) -> bool {
    const NAMES: &[&str] = &[
        "pcmanfm", "thunar", "nautilus", "nemo", "dolphin", "caja", "spacefm",
    ];
    let instance = instance.to_ascii_lowercase();
    let class = class.to_ascii_lowercase();
    NAMES.iter().any(|name| instance == *name || class == *name)
}

/// 标题是绝对目录，或某个参数的最后一段与标题相同且该参数是现存目录。
pub fn directory_from_manager(
    title: &str,
    args: &[String],
    is_dir: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let title = title.trim();
    if title.is_empty() || title.contains('\0') {
        return None;
    }
    let direct = PathBuf::from(title);
    if direct.is_absolute() && is_dir(&direct) && !path_has_parent(&direct) {
        return Some(direct);
    }
    args.iter().find_map(|arg| {
        let path = PathBuf::from(arg);
        if !path.is_absolute() || path_has_parent(&path) || !is_dir(&path) {
            return None;
        }
        let name = path.file_name()?.to_str()?;
        (name == title).then_some(path)
    })
}

fn path_has_parent(path: &Path) -> bool {
    path.components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
}

fn quoted_strings(line: &str) -> impl Iterator<Item = String> + '_ {
    let mut rest = line;
    std::iter::from_fn(move || {
        let start = rest.find('"')?;
        rest = &rest[start + 1..];
        let end = rest.find('"')?;
        let value = rest[..end].to_owned();
        rest = &rest[end + 1..];
        Some(value)
    })
}
