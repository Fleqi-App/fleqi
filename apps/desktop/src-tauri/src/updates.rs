//! Signed Tauri updates. The renderer cannot choose the feed, key or package URL.
use crate::{commands::authorize, state::AppState, windows::WindowRole};
use fleqi_application::dto::{AppError, AppResult, AppUpdatePhase, AppUpdateStatus};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tauri::{AppHandle, Manager, Runtime, State, Webview};
use tauri_plugin_updater::{Update, UpdaterExt};

#[derive(Default)]
pub struct Updates {
    status: Mutex<AppUpdateStatus>,
    pending: Mutex<Option<Update>>,
    working: AtomicBool,
    pub installing: AtomicBool,
}

impl Updates {
    fn snapshot(&self) -> AppUpdateStatus {
        self.status.lock().expect("update status").clone()
    }
    fn fail(&self, message: String) -> AppError {
        let mut status = self.status.lock().expect("update status");
        status.phase = AppUpdatePhase::Failed;
        status.error = Some(message.clone());
        AppError::unavailable(message)
    }
}

struct Operation<'a>(&'a Updates);
struct PauseSessions(Arc<fleqi_application::session_service::SessionService>);
impl Drop for PauseSessions {
    fn drop(&mut self) {
        self.0.resume_creation_after_update();
    }
}
pub fn is_installing<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.try_state::<Updates>()
        .is_some_and(|updates| updates.installing.load(Ordering::Acquire))
}
impl Drop for Operation<'_> {
    fn drop(&mut self) {
        self.0.installing.store(false, Ordering::Release);
        self.0.working.store(false, Ordering::Release);
    }
}

pub async fn check<R: Runtime>(app: &AppHandle<R>) -> AppResult<AppUpdateStatus> {
    if cfg!(windows) {
        return Err(AppError::unavailable(
            "Windows 自动更新尚未配置，请使用安装包更新",
        ));
    }
    let updates = app.state::<Updates>();
    if updates.working.swap(true, Ordering::AcqRel) {
        return Ok(updates.snapshot());
    }
    let _operation = Operation(&updates);
    *updates.pending.lock().expect("pending update") = None;
    *updates.status.lock().expect("update status") = AppUpdateStatus {
        phase: AppUpdatePhase::Checking,
        ..Default::default()
    };
    let result = async {
        app.updater_builder()
            .timeout(std::time::Duration::from_secs(25))
            .build()?
            .check()
            .await
    }
    .await;
    match result {
        Ok(update) => {
            let status = AppUpdateStatus {
                phase: if update.is_some() {
                    AppUpdatePhase::Available
                } else {
                    AppUpdatePhase::Current
                },
                version: update.as_ref().map(|entry| entry.version.clone()),
                notes: update.as_ref().and_then(|entry| entry.body.clone()),
                ..Default::default()
            };
            *updates.pending.lock().expect("pending update") = update;
            *updates.status.lock().expect("update status") = status.clone();
            Ok(status)
        }
        Err(error) => Err(updates.fail(format!("检查更新失败，请检查网络后重试：{error}"))),
    }
}

#[tauri::command]
pub fn app_update_status<R: Runtime>(
    webview: Webview<R>,
    updates: State<'_, Updates>,
) -> AppResult<AppUpdateStatus> {
    crate::commands::authorize_local(&webview)?;
    Ok(updates.snapshot())
}

#[tauri::command]
pub async fn app_update_check<R: Runtime>(
    webview: Webview<R>,
    app: AppHandle<R>,
) -> AppResult<AppUpdateStatus> {
    authorize(&webview)?;
    check(&app).await
}

fn ensure_idle(state: &AppState) -> AppResult<()> {
    if !state.sessions.active_ids().is_empty()
        || state.tools.preparation().running
        || !state.install_jobs.lock().expect("install jobs").is_empty()
        || !state
            .planning_cancels
            .lock()
            .expect("planning jobs")
            .is_empty()
    {
        return Err(AppError::unavailable(
            "请先结束所有会话，并等待工具准备完成，再安装更新；任务和终端不会被自动中断。",
        ));
    }
    Ok(())
}

#[tauri::command]
pub async fn app_update_install<R: Runtime>(
    webview: Webview<R>,
    app: AppHandle<R>,
    state: State<'_, Arc<AppState>>,
) -> AppResult<()> {
    if !matches!(
        authorize(&webview)?,
        WindowRole::Console | WindowRole::Settings
    ) {
        return Err(AppError::forbidden("请在关于与更新中安装新版本"));
    }
    ensure_idle(&state)?;
    let updates = app.state::<Updates>();
    if updates.working.swap(true, Ordering::AcqRel) {
        return Err(AppError::unavailable("正在处理更新，请稍候"));
    }
    let _operation = Operation(&updates);
    let update = updates
        .pending
        .lock()
        .expect("pending update")
        .clone()
        .ok_or_else(|| AppError::unavailable("请先检查更新"))?;
    {
        let mut status = updates.status.lock().expect("update status");
        status.phase = AppUpdatePhase::Downloading;
        status.error = None;
        status.downloaded_bytes = 0;
        status.total_bytes = None;
    }
    // download() verifies the signature before returning bytes. A failed download
    // never reaches install() and never resets the current app's permissions.
    let bytes = update
        .download(
            |chunk, total| {
                let mut status = updates.status.lock().expect("update status");
                status.downloaded_bytes += chunk as u64;
                status.total_bytes = total;
            },
            || {},
        )
        .await
        .map_err(|error| updates.fail(format!("更新下载或签名校验失败：{error}")))?;
    updates.installing.store(true, Ordering::Release);
    state
        .sessions
        .pause_creation_for_update()
        .map_err(|error| updates.fail(error.message))?;
    let _sessions = PauseSessions(Arc::clone(&state.sessions));
    ensure_idle(&state).map_err(|error| updates.fail(error.message))?;
    updates.status.lock().expect("update status").phase = AppUpdatePhase::Installing;
    let installed = tauri::async_runtime::spawn_blocking(move || {
        #[cfg(target_os = "macos")]
        {
            fleqi_platform::macos::update_install::install_verified(&bytes, &update.version)
        }
        #[cfg(not(target_os = "macos"))]
        {
            update.install(bytes).map_err(|error| error.to_string())
        }
    })
    .await
    .map_err(|error| updates.fail(format!("安装更新失败：{error}")))?;
    installed.map_err(|error| updates.fail(format!("安装更新失败，当前版本仍可使用：{error}")))?;
    // The new installed bundle resets TCC before initializing permission services.
    state.begin_shutdown();
    app.restart();
}
