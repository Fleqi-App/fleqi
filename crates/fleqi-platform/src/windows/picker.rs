//! 文件夹对话框。取消与失败的判定是纯函数；对话框本身只在 Windows 主线程打开。

use fleqi_application::ports::{DirectoryPick, RawPath};
use fleqi_domain::context::PathKind;
use std::path::PathBuf;

use crate::scheduling::MainThreadExecutor;

/// 固定脚本：System.Windows.Forms.FolderBrowserDialog。路径只从标准输出读取。
pub const FOLDER_DIALOG_SCRIPT: &str = r#"
$OutputEncoding = [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false
Add-Type -AssemblyName System.Windows.Forms
$dialog = New-Object System.Windows.Forms.FolderBrowserDialog
$dialog.Description = 'Select a working folder'
$dialog.ShowNewFolderButton = $true
$result = $dialog.ShowDialog()
if ($result -eq [System.Windows.Forms.DialogResult]::OK -and $dialog.SelectedPath) {
    Write-Output $dialog.SelectedPath
}
exit 0
"#;

pub fn pick_directory_blocking(main: &dyn MainThreadExecutor) -> DirectoryPick {
    #[cfg(target_os = "windows")]
    {
        run_on_main(main)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = main;
        DirectoryPick::Failed("文件夹对话框仅在 Windows 上打开".into())
    }
}

/// `cancelled` 或成功但没有路径都是取消。存在的目录才是选中；其它非空结果失败。
pub fn classify_picker_output(cancelled: bool, raw: &str, exists_and_dir: bool) -> DirectoryPick {
    if cancelled || raw.trim().is_empty() {
        return DirectoryPick::Cancelled;
    }
    let path = raw.trim();
    if exists_and_dir {
        DirectoryPick::Selected(RawPath {
            native: PathBuf::from(path),
            kind: PathKind::Directory,
        })
    } else {
        DirectoryPick::Failed(format!("所选项不是目录：{path}"))
    }
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn run_on_main(main: &dyn MainThreadExecutor) -> DirectoryPick {
    let (sender, receiver) = std::sync::mpsc::channel();
    main.run(Box::new(move || {
        let _ = sender.send(run_folder_dialog());
    }));
    receiver
        .recv()
        .unwrap_or_else(|_| DirectoryPick::Failed("文件夹对话框未返回结果".into()))
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn run_folder_dialog() -> DirectoryPick {
    let output = match super::process::run_fixed_script(FOLDER_DIALOG_SCRIPT, &[], true, false) {
        Ok(output) => output,
        Err(error) => return DirectoryPick::Failed(format!("无法打开文件夹对话框：{error}")),
    };
    if !output.success {
        return DirectoryPick::Failed("文件夹对话框没有成功打开".into());
    }
    let trimmed = output.stdout.trim();
    let exists_and_dir = !trimmed.is_empty() && std::path::Path::new(trimmed).is_dir();
    classify_picker_output(false, &output.stdout, exists_and_dir)
}
