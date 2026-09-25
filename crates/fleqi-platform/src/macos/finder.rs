//! Finder 上下文读取（architecture.md §12.4）：固定 AppleScript（不拼接用户路径），
//! 结果按 NSAppleEventDescriptor 结构化解析；读取前先无提示核对自动化权限与 Finder
//! 运行状态，不触发授权弹窗；有界超时；虚拟视图不猜 cwd；超限不截取。

use fleqi_application::ports::{ContextPort, DirectoryPick, RawContext, RawPath};
use fleqi_domain::context::{ContextAvailability, PathKind, SELECTION_LIMIT, ViewKind};
use fleqi_domain::permissions::{Permission, PermissionProcedure, PermissionStatus};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AllocAnyThread, ClassType, msg_send};
use objc2_app_kit::NSRunningApplication;
use objc2_foundation::{NSAppleEventDescriptor, NSAppleScript, NSDictionary, NSString};
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::permissions::MacPermissions;
use super::picker::{MainThreadExecutor, pick_directory_blocking};
use fleqi_application::ports::PermissionPort;

const FINDER_BUNDLE_ID: &str = "com.apple.finder";
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(4);

/// 固定脚本：只读取窗口标识、目标类别、目录与选区；返回 {wid, kind, dir, count, paths}。
/// 选区数超过上限时不导出路径列表（超限不截取）。
const FINDER_SNAPSHOT_SCRIPT: &str = r#"
set desktopPath to POSIX path of (path to desktop folder)
set selLimit to __LIMIT__
tell application "Finder"
    set winCount to count of Finder windows
    set wid to 0
    set kindText to "desktop"
    set dirPath to desktopPath
    if winCount is greater than 0 then
        set w to Finder window 1
        set wid to id of w
        set kindText to "virtual"
        set dirPath to ""
        try
            set t to target of w
            set c to class of t
            if c is folder or c is disk or c is desktop-object then
                set dirPath to POSIX path of (t as alias)
                set kindText to "physical"
            end if
        end try
    end if
    set sel to selection
    set selCount to count of sel
    set selPaths to {}
    if selCount is less than or equal to selLimit then
        repeat with i in sel
            try
                set end of selPaths to POSIX path of (i as alias)
            end try
        end repeat
    end if
    return {wid, kindText, dirPath, selCount, selPaths}
end tell
"#;

/// 脚本返回的结构化结果（与 AppleScript 记录一一对应）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinderReading {
    pub window_id: i32,
    pub kind: String,
    pub directory: String,
    pub selection_count: usize,
    pub selection: Vec<String>,
}

/// 结构化结果 → RawContext（纯函数，可测试）。
pub fn reading_to_raw(reading: FinderReading) -> RawContext {
    let view_kind = match reading.kind.as_str() {
        "physical" => ViewKind::Physical,
        "desktop" => ViewKind::Desktop,
        _ => ViewKind::Virtual,
    };
    let directory = if reading.directory.is_empty() {
        None
    } else {
        Some(RawPath {
            native: PathBuf::from(&reading.directory),
            kind: PathKind::Directory,
        })
    };
    let over_limit = reading.selection_count > SELECTION_LIMIT;
    let incomplete = !over_limit && reading.selection_count != reading.selection.len();
    let selection = if over_limit || incomplete {
        Vec::new()
    } else {
        reading
            .selection
            .iter()
            .map(|p| {
                let native = PathBuf::from(p);
                let kind = if p.ends_with('/') || native.is_dir() {
                    PathKind::Directory
                } else {
                    PathKind::File
                };
                RawPath { native, kind }
            })
            .collect()
    };
    RawContext {
        source_window_id: if reading.window_id > 0 {
            Some(reading.window_id as u64)
        } else {
            None
        },
        directory,
        selection,
        view_kind: Some(view_kind),
        unavailable: if over_limit {
            Some(ContextAvailability::SelectionOverLimit {
                count: reading.selection_count,
                limit: SELECTION_LIMIT,
            })
        } else if incomplete {
            Some(ContextAvailability::Failed {
                message: "Finder 部分选中文件无法读取，请重新选择；未使用不完整选区".into(),
            })
        } else {
            None
        },
    }
}

fn finder_running() -> bool {
    let bundle = NSString::from_str(FINDER_BUNDLE_ID);
    !NSRunningApplication::runningApplicationsWithBundleIdentifier(&bundle).is_empty()
}

fn execute_script() -> Result<FinderReading, String> {
    let source = NSString::from_str(
        &FINDER_SNAPSHOT_SCRIPT.replace("__LIMIT__", &SELECTION_LIMIT.to_string()),
    );
    let script = NSAppleScript::initWithSource(NSAppleScript::alloc(), &source)
        .ok_or("NSAppleScript 初始化失败")?;
    let mut error: Option<Retained<NSDictionary<NSString, AnyObject>>> = None;
    // SAFETY: 方法在失败时返回 nil 并填充 error；用 msg_send 以 Option 接收避免空指针断言。
    let result: Option<Retained<NSAppleEventDescriptor>> =
        unsafe { msg_send![&script, executeAndReturnError: &mut error] };
    let Some(descriptor) = result else {
        let detail = error
            .and_then(|dict| {
                let key = NSString::from_str("NSAppleScriptErrorMessage");
                dict.objectForKey(&key).map(|v| format!("{v:?}"))
            })
            .unwrap_or_else(|| "未知错误".to_owned());
        return Err(format!("Finder 脚本执行失败：{detail}"));
    };
    parse_descriptor(&descriptor)
}

fn string_at(descriptor: &NSAppleEventDescriptor, index: isize) -> Result<String, String> {
    descriptor
        .descriptorAtIndex(index)
        .and_then(|d| d.stringValue())
        .map(|s| s.to_string())
        .ok_or_else(|| format!("描述符第 {index} 项不是字符串"))
}

fn int_at(descriptor: &NSAppleEventDescriptor, index: isize) -> Result<i32, String> {
    descriptor
        .descriptorAtIndex(index)
        .map(|d| d.int32Value())
        .ok_or_else(|| format!("描述符缺少第 {index} 项"))
}

fn parse_descriptor(descriptor: &NSAppleEventDescriptor) -> Result<FinderReading, String> {
    if descriptor.numberOfItems() < 5 {
        return Err(format!("描述符项数不足：{}", descriptor.numberOfItems()));
    }
    let window_id = int_at(descriptor, 1)?;
    let kind = string_at(descriptor, 2)?;
    let directory = string_at(descriptor, 3)?;
    let selection_count = int_at(descriptor, 4)?.max(0) as usize;
    let list = descriptor
        .descriptorAtIndex(5)
        .ok_or("描述符缺少选区列表")?;
    let mut selection = Vec::with_capacity(list.numberOfItems().max(0) as usize);
    for i in 1..=list.numberOfItems() {
        if let Some(item) = list.descriptorAtIndex(i).and_then(|d| d.stringValue()) {
            selection.push(item.to_string());
        }
    }
    Ok(FinderReading {
        window_id,
        kind,
        directory,
        selection_count,
        selection,
    })
}

pub struct MacContextPort {
    permissions: Arc<MacPermissions>,
    main: Arc<dyn MainThreadExecutor>,
    capture_lock: Mutex<()>,
}

impl MacContextPort {
    pub fn new(permissions: Arc<MacPermissions>, main: Arc<dyn MainThreadExecutor>) -> Self {
        Self {
            permissions,
            main,
            capture_lock: Mutex::new(()),
        }
    }

    fn capture_inner(&self) -> Result<RawContext, ContextAvailability> {
        if !finder_running() {
            return Err(ContextAvailability::FinderNotRunning);
        }
        match self
            .permissions
            .check(Permission::FinderAutomation, PermissionProcedure::Passive)
            .status
        {
            PermissionStatus::Allowed => {}
            PermissionStatus::TargetNotRunning => {
                return Err(ContextAvailability::FinderNotRunning);
            }
            PermissionStatus::Failed => {
                return Err(ContextAvailability::Failed {
                    message: "权限探测失败".into(),
                });
            }
            _ => return Err(ContextAvailability::PermissionRequired),
        }
        let Ok(_guard) = self.capture_lock.try_lock() else {
            return Err(ContextAvailability::Failed {
                message: "上一次 Finder 读取仍在进行".into(),
            });
        };
        let (sender, receiver) = channel();
        std::thread::Builder::new()
            .name("fleqi-finder-snapshot".into())
            .spawn(move || {
                let _ = sender.send(execute_script());
            })
            .map_err(|e| ContextAvailability::Failed {
                message: format!("无法启动读取线程：{e}"),
            })?;
        match receiver.recv_timeout(CAPTURE_TIMEOUT) {
            Ok(Ok(reading)) => Ok(reading_to_raw(reading)),
            Ok(Err(message)) => Err(ContextAvailability::Failed { message }),
            Err(_) => Err(ContextAvailability::Failed {
                message: format!("Finder 未在 {} 秒内响应", CAPTURE_TIMEOUT.as_secs()),
            }),
        }
    }
}

impl ContextPort for MacContextPort {
    fn capture(&self) -> RawContext {
        match self.capture_inner() {
            Ok(raw) => raw,
            Err(unavailable) => RawContext {
                unavailable: Some(unavailable),
                ..RawContext::default()
            },
        }
    }

    fn pick_directory(&self) -> DirectoryPick {
        pick_directory_blocking(self.main.as_ref())
    }
}

/// 供不依赖 Finder 的测试确认脚本文本是固定常量（不含运行期插值）。
pub fn snapshot_script_is_constant() -> bool {
    !FINDER_SNAPSHOT_SCRIPT.contains("__DIR__") && FINDER_SNAPSHOT_SCRIPT.matches("__").count() == 2
}

/// 固定脚本：最前 Finder 窗口的 bounds {x1, y1, x2, y2}（AppleScript 点坐标，
/// 左上原点）；无窗口返回全零。
const FINDER_BOUNDS_SCRIPT: &str = r#"
tell application "Finder"
    if (count of Finder windows) is greater than 0 then
        return bounds of Finder window 1
    end if
    return {0, 0, 0, 0}
end tell
"#;

/// 固定脚本：桌面窗口边界即整屏（菜单栏不计入，Dock 区域计入；用于判断
/// 输入条外侧下方放不放得下）。桌面窗口不存在时返回全零。
const SCREEN_BOUNDS_SCRIPT: &str = r#"
tell application "Finder"
    if (count of desktop windows) is greater than 0 then
        return bounds of desktop window of desktop
    end if
    return {0, 0, 0, 0}
end tell
"#;

/// 运行只读 bounds 脚本并解析为 [x1, y1, x2, y2]；失败或全零返回 None。
fn run_bounds_script(script: &str, worker: &str) -> Option<(f64, f64, f64, f64)> {
    let (sender, receiver) = channel();
    let script = script.to_owned();
    let spawned = std::thread::Builder::new()
        .name(worker.into())
        .spawn(move || {
            let source = NSString::from_str(&script);
            let Some(script) = NSAppleScript::initWithSource(NSAppleScript::alloc(), &source)
            else {
                return;
            };
            let mut error: Option<Retained<NSDictionary<NSString, AnyObject>>> = None;
            // SAFETY：失败返回 nil 并填充 error（与快照脚本同一模式）。
            let result: Option<Retained<NSAppleEventDescriptor>> =
                unsafe { msg_send![&script, executeAndReturnError: &mut error] };
            let Some(descriptor) = result else { return };
            if descriptor.numberOfItems() < 4 {
                return;
            }
            let mut values = [0f64; 4];
            for (slot, index) in values.iter_mut().zip(1..=4isize) {
                match descriptor.descriptorAtIndex(index) {
                    Some(item) => slot.clone_from(&item.doubleValue()),
                    None => return,
                }
            }
            if values[2] > values[0] && values[3] > values[1] {
                let _ = sender.send((values[0], values[1], values[2], values[3]));
            }
        });
    if spawned.is_err() {
        return None;
    }
    receiver.recv_timeout(Duration::from_secs(2)).ok()
}

/// 最前 Finder 窗口边界（贴附定位用；best-effort：Finder 未运行、无窗口、脚本
/// 失败或权限不足时返回 None，调用方回退默认位置）。
pub fn finder_window_bounds() -> Option<(f64, f64, f64, f64)> {
    if !finder_running() {
        return None;
    }
    run_bounds_script(FINDER_BOUNDS_SCRIPT, "fleqi-finder-bounds")
}

/// 主显示屏边界（AppleScript 桌面窗口；内侧贴底回退判断用，best-effort）。
pub fn screen_bounds() -> Option<(f64, f64, f64, f64)> {
    if !finder_running() {
        return None;
    }
    run_bounds_script(SCREEN_BOUNDS_SCRIPT, "fleqi-screen-bounds")
}

#[allow(dead_code)]
fn _class_marker() -> &'static objc2::runtime::AnyClass {
    NSAppleScript::class()
}
