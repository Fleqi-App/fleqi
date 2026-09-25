//! Fixed Homebrew packages for first-launch preparation. Existing tools stay owned
//! by the user/package manager; Fleqi never removes or upgrades them implicitly.
use crate::tools::{download_to, file_sha256, lookup_on_path};
use fleqi_application::ports::{InstallProgress, ProcessEvent, ProcessPort};
use fleqi_domain::tools::{ToolManifest, system_package};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

const BREW_INSTALLER_URL: &str =
    "https://github.com/Homebrew/brew/releases/download/7.0.6/Homebrew.pkg";
const BREW_INSTALLER_SHA256: &str =
    "29e8a9bd1c665ec0f9c07e6e3f6f9d663a8a5ed1d5575a2ab62c956b2e0c11e0";

pub fn install(
    root: &Path,
    process: &Arc<dyn ProcessPort>,
    manifest: &ToolManifest,
    cancel: &AtomicBool,
    progress: &dyn Fn(InstallProgress),
) -> Result<(), String> {
    let package = system_package(&manifest.id).ok_or("没有登记此系统工具的安装来源")?;
    if !cfg!(target_os = "macos") {
        return Err("当前平台尚无自动安装映射".into());
    }
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let brew = if let Some(path) = lookup_on_path("brew") {
        path
    } else {
        if !cfg!(target_arch = "aarch64") {
            return Err(
                "这台 Mac 需要先按 Homebrew 官方说明安装包管理器，然后点击继续工具准备".into(),
            );
        }
        let installer = root.join("Homebrew-7.0.6.pkg");
        if !installer.is_file()
            || file_sha256(&installer).map_err(|e| e.to_string())? != BREW_INSTALLER_SHA256
        {
            let staging = root.join("Homebrew-7.0.6.pkg.download");
            let result =
                download_to(BREW_INSTALLER_URL, &staging, cancel, progress).and_then(|_| {
                    progress(InstallProgress::Verifying);
                    if file_sha256(&staging).map_err(|e| e.to_string())? != BREW_INSTALLER_SHA256 {
                        return Err("Homebrew 安装包完整性校验失败".into());
                    }
                    std::fs::rename(&staging, &installer).map_err(|e| e.to_string())
                });
            if result.is_err() {
                let _ = std::fs::remove_file(&staging);
            }
            result?;
        }
        run(
            process,
            "/usr/sbin/pkgutil",
            &["--check-signature".into(), installer.display().to_string()],
            root,
            cancel,
            &|_| {},
        )?;
        progress(InstallProgress::Installing {
            message:
                "请在系统安装器中完成 Homebrew 安装，完成后将自动继续；管理员密码只在系统窗口输入。"
                    .into(),
        });
        run(
            process,
            "/usr/bin/open",
            &[installer.display().to_string()],
            root,
            cancel,
            &|_| {},
        )?;
        let deadline = Instant::now() + Duration::from_secs(900);
        loop {
            if cancel.load(Ordering::Acquire) {
                return Err("已取消工具准备；系统安装器由用户控制".into());
            }
            if let Some(path) = lookup_on_path("brew") {
                break path;
            }
            if Instant::now() > deadline {
                return Err("请完成 Homebrew 系统安装，再点击继续工具准备".into());
            }
            std::thread::sleep(Duration::from_millis(400));
        }
    };
    if cancel.load(Ordering::Acquire) {
        return Err("已取消".into());
    }
    progress(InstallProgress::Installing {
        message: format!("正在安装 {package} 及所需依赖…"),
    });
    run(
        process,
        &brew.to_string_lossy(),
        &["install".into(), "--formula".into(), package.into()],
        root,
        cancel,
        progress,
    )
}

fn run(
    process: &Arc<dyn ProcessPort>,
    executable: &str,
    args: &[String],
    cwd: &Path,
    cancel: &AtomicBool,
    progress: &dyn Fn(InstallProgress),
) -> Result<(), String> {
    let (tx, rx) = mpsc::channel();
    let handle = process.spawn(
        executable,
        args,
        cwd,
        &[
            ("HOMEBREW_NO_AUTO_UPDATE".into(), "1".into()),
            ("HOMEBREW_NO_INSTALL_CLEANUP".into(), "1".into()),
            ("HOMEBREW_NO_ANALYTICS".into(), "1".into()),
            ("NONINTERACTIVE".into(), "1".into()),
        ],
        tx,
    )?;
    let mut last = String::new();
    let deadline = Instant::now() + Duration::from_secs(1800);
    loop {
        if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
            handle.cancel();
            return Err(if cancel.load(Ordering::Acquire) {
                "已取消"
            } else {
                "安装超时，可在工具页重试"
            }
            .into());
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(ProcessEvent::Output { bytes, .. }) => {
                let text = String::from_utf8_lossy(&bytes);
                if let Some(line) = text.lines().rev().find(|line| !line.trim().is_empty()) {
                    last = line.chars().take(240).collect();
                    progress(InstallProgress::Installing {
                        message: last.clone(),
                    });
                }
            }
            Ok(ProcessEvent::Exited { status: Some(0) }) => return Ok(()),
            Ok(ProcessEvent::Exited { status }) => {
                return Err(format!("安装命令未成功（{status:?}）：{last}"));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(format!("安装进程意外结束：{last}"));
            }
        }
    }
}
