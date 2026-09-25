//! NSOpenPanel 目录选择（architecture.md §12.4）：只选真实目录；取消不改快照。
//! AppKit 必须在主线程运行：宿主注入 MainThreadExecutor（Tauri run_on_main_thread）。

use fleqi_application::ports::{DirectoryPick, RawPath};
use fleqi_domain::context::PathKind;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSModalResponseOK, NSOpenPanel};
use objc2_foundation::NSString;
use std::path::PathBuf;
use std::sync::mpsc::channel;

/// 把闭包投递到 AppKit 主线程执行。
pub trait MainThreadExecutor: Send + Sync {
    fn run(&self, job: Box<dyn FnOnce() + Send>);
}

/// 在主线程运行模态面板并等待结果。
pub fn pick_directory_blocking(main: &dyn MainThreadExecutor) -> DirectoryPick {
    let (sender, receiver) = channel();
    main.run(Box::new(move || {
        let result = match MainThreadMarker::new() {
            Some(mtm) => run_panel(mtm),
            None => DirectoryPick::Failed("目录选择未在主线程执行".into()),
        };
        let _ = sender.send(result);
    }));
    receiver
        .recv()
        .unwrap_or_else(|_| DirectoryPick::Failed("目录选择面板未返回结果".into()))
}

fn run_panel(mtm: MainThreadMarker) -> DirectoryPick {
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseDirectories(true);
    panel.setCanChooseFiles(false);
    panel.setAllowsMultipleSelection(false);
    panel.setCanCreateDirectories(true);
    panel.setResolvesAliases(true);
    panel.setMessage(Some(&NSString::from_str("选择 Fleqi 的工作文件夹")));
    panel.setPrompt(Some(&NSString::from_str("选择")));
    let response = panel.runModal();
    if response != NSModalResponseOK {
        return DirectoryPick::Cancelled;
    }
    let urls = panel.URLs();
    let Some(url) = urls.firstObject() else {
        return DirectoryPick::Failed("面板未返回目录".into());
    };
    let Some(path) = url.path() else {
        return DirectoryPick::Failed("所选 URL 不是文件路径".into());
    };
    let native = PathBuf::from(path.to_string());
    if !native.is_dir() {
        return DirectoryPick::Failed(format!("所选项不是目录：{}", native.display()));
    }
    DirectoryPick::Selected(RawPath {
        native,
        kind: PathKind::Directory,
    })
}
