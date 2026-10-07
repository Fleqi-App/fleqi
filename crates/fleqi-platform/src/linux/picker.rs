//! 目录选择。优先 `zenity`，其次 `kdialog`。只使用固定 argv，不经 shell。
//! 两个工具都不在 `PATH` 上时失败，不假装用户选中了目录。

pub use fleqi_application::ports::{DirectoryPick, RawPath};
pub use fleqi_domain::context::PathKind;
use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;

use crate::scheduling::MainThreadExecutor;

const TITLE: &str = "选择 Fleqi 的工作文件夹";
const ZENITY_ARGS: &[&str] = &["--file-selection", "--directory", "--title", TITLE];
const KDIALOG_ARGS: &[&str] = &["--getexistingdirectory", "--title", TITLE];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogTool {
    Zenity,
    Kdialog,
}

/// 在主线程执行对话框并等待结果。取消与缺失工具都不会返回目录。
pub fn pick_directory_blocking(main: &dyn MainThreadExecutor) -> DirectoryPick {
    let (sender, receiver) = channel();
    main.run(Box::new(move || {
        let _ = sender.send(run_dialog());
    }));
    receiver
        .recv()
        .unwrap_or_else(|_| DirectoryPick::Failed("目录选择面板未返回结果".into()))
}

/// `zenity` 优先于 `kdialog`。两者都不存在时返回 `None`。
pub fn select_dialog_tool(zenity_on_path: bool, kdialog_on_path: bool) -> Option<DialogTool> {
    if zenity_on_path {
        Some(DialogTool::Zenity)
    } else if kdialog_on_path {
        Some(DialogTool::Kdialog)
    } else {
        None
    }
}

/// 程序名与参数。标题是独立的 argv 元素，不是 `--title=...`。
pub fn dialog_command(tool: DialogTool) -> (&'static str, &'static [&'static str]) {
    match tool {
        DialogTool::Zenity => ("zenity", ZENITY_ARGS),
        DialogTool::Kdialog => ("kdialog", KDIALOG_ARGS),
    }
}

pub fn missing_dialog_tool() -> DirectoryPick {
    DirectoryPick::Failed(
        "未找到目录选择工具。请安装 zenity 或 kdialog 后再选择工作文件夹。".into(),
    )
}

/// 把对话框退出码和标准输出分成取消、选中目录或失败。
///
/// `is_directory` 由调用方对修剪后的路径做真实检查。空输出，以及 zenity/kdialog
/// 的取消退出码（1，以及 zenity 超时 5），都是取消。
pub fn classify_dialog_output(exit_code: i32, stdout: &str, is_directory: bool) -> DirectoryPick {
    let trimmed = stdout.trim();
    if trimmed.is_empty() || is_cancel_code(exit_code) {
        return DirectoryPick::Cancelled;
    }
    if exit_code != 0 {
        return DirectoryPick::Failed(format!("目录选择失败，退出码 {exit_code}"));
    }
    let native = PathBuf::from(trimmed);
    if is_directory {
        DirectoryPick::Selected(RawPath {
            native,
            kind: PathKind::Directory,
        })
    } else {
        DirectoryPick::Failed(format!("所选项不是目录：{}", native.display()))
    }
}

fn is_cancel_code(exit_code: i32) -> bool {
    matches!(exit_code, 1 | 5)
}

fn run_dialog() -> DirectoryPick {
    let Some(tool) =
        select_dialog_tool(executable_on_path("zenity"), executable_on_path("kdialog"))
    else {
        return missing_dialog_tool();
    };
    let (program, args) = dialog_command(tool);
    match std::process::Command::new(program).args(args).output() {
        Ok(output) => {
            let code = output.status.code().unwrap_or(128);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let trimmed = stdout.trim();
            let is_directory = Path::new(trimmed).is_dir();
            classify_dialog_output(code, &stdout, is_directory)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => missing_dialog_tool(),
        Err(error) => DirectoryPick::Failed(format!("无法启动目录选择工具：{error}")),
    }
}

fn executable_on_path(name: &str) -> bool {
    let Some(path_var) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path_var).any(|dir| is_executable(&dir.join(name)))
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .map(|meta| path.is_file() && meta.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}
