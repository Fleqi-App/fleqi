//! M1 IPC（architecture.md §12.2）。每个命令先核对调用窗口身份与本地 origin，
//! 阻塞工作放到 blocking 线程；不接受页面自报角色。

use tauri::Manager;

use fleqi_application::dto::{
    AppBootstrap, AppError, AppResult, BuildInfo, DiagnosticsSnapshot, DirectoryPickResult,
    PermissionOperation, PermissionSnapshot, SettingsSnapshot, SettingsUpdateRequest,
};
use fleqi_domain::context::ContextSnapshot;
use fleqi_domain::permissions::Permission;
use fleqi_domain::settings::FieldError;
use std::sync::Arc;
use tauri::{AppHandle, Runtime, State, Url, Webview};

use crate::state::AppState;
use crate::windows::{WindowRole, open_window};

/// 只接受宿主自己创建的窗口，且页面来源必须是打包的本地应用 origin
/// （开发模式允许 127.0.0.1:1420 的 Vite 服务）。
pub fn is_local_app_url(url: &Url) -> bool {
    match url.scheme() {
        "tauri" => url.host_str() == Some("localhost"),
        "http" => {
            url.host_str() == Some("tauri.localhost")
                || (cfg!(debug_assertions)
                    && url.host_str() == Some("127.0.0.1")
                    && url.port() == Some(1420))
        }
        _ => false,
    }
}

pub(crate) fn authorize<R: Runtime>(webview: &Webview<R>) -> AppResult<WindowRole> {
    let role = authorize_local(webview)?;
    if crate::updates::is_installing(webview.app_handle()) {
        return Err(AppError::unavailable("正在安装更新，请稍候"));
    }
    Ok(role)
}

pub(crate) fn authorize_local<R: Runtime>(webview: &Webview<R>) -> AppResult<WindowRole> {
    let role = WindowRole::from_label(webview.label())
        .ok_or_else(|| AppError::forbidden(format!("窗口 {} 无权调用宿主命令", webview.label())))?;
    match webview.url() {
        Ok(url) if is_local_app_url(&url) => Ok(role),
        Ok(url) => Err(AppError::forbidden(format!(
            "非本地应用来源：{}",
            url.origin().ascii_serialization()
        ))),
        Err(error) => Err(AppError::forbidden(format!("无法核对来源 URL：{error}"))),
    }
}

pub(crate) fn invalid(field: &str, message: impl Into<String>) -> AppError {
    AppError::validation(vec![FieldError {
        field: field.into(),
        code: "invalid".into(),
        message: message.into(),
    }])
}

pub(crate) async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| AppError::internal(format!("阻塞任务失败：{e}")))
}

#[tauri::command]
pub fn app_build_info<R: Runtime>(webview: Webview<R>) -> AppResult<BuildInfo> {
    authorize(&webview)?;
    Ok(BuildInfo::current())
}

#[tauri::command]
pub fn app_bootstrap<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
) -> AppResult<AppBootstrap> {
    let role = authorize(&webview)?;
    Ok(state.bootstrap(role.label()))
}

#[tauri::command]
pub fn diagnostics_get<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
) -> AppResult<DiagnosticsSnapshot> {
    authorize(&webview)?;
    Ok(state.diagnostics())
}

#[tauri::command]
pub fn app_open_window<R: Runtime>(
    webview: Webview<R>,
    app: AppHandle,
    role: String,
    page: Option<String>,
) -> AppResult<()> {
    authorize(&webview)?;
    let role = WindowRole::from_label(&role)
        .ok_or_else(|| invalid("role", format!("未知窗口角色 {role}")))?;
    open_window(&app, role, page.as_deref()).map_err(AppError::internal)
}

#[tauri::command]
pub fn app_quit<R: Runtime>(
    webview: Webview<R>,
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> AppResult<()> {
    authorize(&webview)?;
    state.begin_shutdown();
    app.exit(0);
    Ok(())
}

#[tauri::command]
pub async fn settings_update<R: Runtime>(
    webview: Webview<R>,
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    request: SettingsUpdateRequest,
) -> AppResult<SettingsSnapshot> {
    authorize(&webview)?;
    let worker = Arc::clone(&state);
    let snapshot = blocking(move || {
        worker.logger.log(
            "info",
            "settings.update",
            &[("requestId", &request.request_id)],
        );
        worker.settings.update(request)
    })
    .await??;
    apply_settings_side_effects(&app, &state);
    Ok(snapshot)
}

#[tauri::command]
pub fn permissions_get<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
) -> AppResult<PermissionSnapshot> {
    authorize(&webview)?;
    Ok(state.permissions.snapshot())
}

#[tauri::command]
pub async fn permissions_check<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
) -> AppResult<PermissionSnapshot> {
    authorize(&webview)?;
    let state = Arc::clone(&state);
    blocking(move || state.permissions.check_all()).await
}

#[tauri::command]
pub fn permissions_request<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    request_id: String,
    permission: Permission,
) -> AppResult<PermissionOperation> {
    authorize(&webview)?;
    if request_id.trim().is_empty() {
        return Err(invalid("requestId", "requestId 不能为空"));
    }
    state.logger.log(
        "info",
        "permissions.request",
        &[
            ("requestId", &request_id),
            ("permission", &format!("{permission:?}")),
        ],
    );
    state.permissions.request(permission)
}

#[tauri::command]
pub fn permissions_open_settings<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    permission: Permission,
) -> AppResult<()> {
    authorize(&webview)?;
    state.permissions.open_system_settings(permission)
}

#[tauri::command]
pub async fn context_get<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    context_id: Option<String>,
) -> AppResult<ContextSnapshot> {
    authorize(&webview)?;
    let state = Arc::clone(&state);
    blocking(move || state.context.get(context_id.as_deref())).await?
}

#[tauri::command]
pub async fn context_refresh<R: Runtime>(
    webview: Webview<R>,
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> AppResult<ContextSnapshot> {
    authorize(&webview)?;
    let state = Arc::clone(&state);
    // 与观察器共用同一条管线：刷新后路由到显示状态机（目录同步/自动显示）。
    let snapshot = blocking(move || state.refresh_context_routed(&app)).await?;
    Ok(snapshot)
}

#[tauri::command]
pub async fn context_pick_directory<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    request_id: String,
) -> AppResult<DirectoryPickResult> {
    authorize(&webview)?;
    if request_id.trim().is_empty() {
        return Err(invalid("requestId", "requestId 不能为空"));
    }
    let state = Arc::clone(&state);
    blocking(move || {
        state
            .logger
            .log("info", "context.pick", &[("requestId", &request_id)]);
        state.context.pick_directory()
    })
    .await
}

// ---------- M2：输入条、会话与终端 ----------

use fleqi_application::session_service::SessionGroup;
use fleqi_application::terminal_service::SubmitOutcome;
use fleqi_domain::composer::ComposerMode;
use fleqi_domain::session::Session;
use fleqi_domain::surface::Visibility;
use tauri::ipc::Channel;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceView {
    pub visibility: &'static str,
    pub visible_session_id: Option<String>,
    pub auto_show_suppressed: bool,
    pub bar_enabled: bool,
    pub activation: String,
}

fn surface_view(state: &AppState) -> SurfaceView {
    let settings = state.settings.snapshot();
    SurfaceView {
        visibility: match state.surface.visibility() {
            Visibility::Visible => "visible",
            Visibility::TemporarilyHidden => "temporarilyHidden",
            Visibility::UserHidden => "userHidden",
        },
        visible_session_id: state.surface.visible_session().map(|s| s.id),
        auto_show_suppressed: state.surface.auto_show_suppressed(),
        bar_enabled: settings.settings.bar_enabled,
        activation: match settings.settings.activation {
            fleqi_domain::settings::Activation::Manual => "manual",
            fleqi_domain::settings::Activation::FollowFinder => "followFinder",
        }
        .into(),
    }
}

#[tauri::command]
pub async fn surface_show<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
) -> AppResult<SurfaceView> {
    authorize(&webview)?;
    let state = Arc::clone(&state);
    blocking(move || {
        state
            .composer_focus_requested
            .store(true, std::sync::atomic::Ordering::Release);
        if let Err(error) = state.surface.user_show() {
            state
                .composer_focus_requested
                .store(false, std::sync::atomic::Ordering::Release);
            return Err(error);
        }
        Ok(surface_view(&state))
    })
    .await?
}

#[tauri::command]
pub async fn surface_hide<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
) -> AppResult<SurfaceView> {
    authorize(&webview)?;
    let state = Arc::clone(&state);
    blocking(move || {
        state.surface.user_hide()?;
        Ok(surface_view(&state))
    })
    .await?
}

#[tauri::command]
pub async fn surface_layout<R: Runtime>(
    webview: Webview<R>,
    app: AppHandle,
    extra_height: u32,
) -> AppResult<()> {
    authorize(&webview)?;
    if webview.label() != "composer" || extra_height > 480 {
        return Err(invalid(
            "extraHeight",
            "仅输入条可申请 0–480 像素的面板空间",
        ));
    }
    crate::windows::layout_composer(&app, extra_height)
        .map_err(|message| invalid("extraHeight", message))
}

#[tauri::command]
pub async fn surface_get<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
) -> AppResult<SurfaceView> {
    authorize(&webview)?;
    let state = Arc::clone(&state);
    blocking(move || surface_view(&state)).await
}

#[tauri::command]
pub async fn session_create<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    request_id: String,
) -> AppResult<Session> {
    authorize(&webview)?;
    if request_id.trim().is_empty() {
        return Err(invalid("requestId", "requestId 不能为空"));
    }
    let state = Arc::clone(&state);
    blocking(move || {
        let context = state.context.latest();
        state.sessions.create_request(&request_id, context.as_ref())
    })
    .await?
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionList {
    pub active: Vec<Session>,
    pub history: Vec<Session>,
}

#[tauri::command]
pub async fn session_list<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    offset: Option<usize>,
    limit: Option<usize>,
) -> AppResult<SessionList> {
    authorize(&webview)?;
    let state = Arc::clone(&state);
    blocking(move || SessionList {
        active: state.sessions.list(
            SessionGroup::Active,
            offset.unwrap_or(0),
            limit.unwrap_or(50),
        ),
        history: state.sessions.list(
            SessionGroup::History,
            offset.unwrap_or(0),
            limit.unwrap_or(50),
        ),
    })
    .await
}

#[tauri::command]
pub async fn session_entries<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    session_id: String,
    limit: Option<usize>,
    before: Option<String>,
) -> AppResult<Vec<fleqi_domain::session::ConversationEntry>> {
    authorize(&webview)?;
    let sessions = state.sessions.clone();
    blocking(move || {
        sessions.get(&session_id)?;
        sessions.entries(
            &session_id,
            limit.unwrap_or(100).clamp(1, 200),
            before.as_deref(),
        )
    })
    .await?
}

#[tauri::command]
pub async fn session_select<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    request_id: String,
    session_id: String,
) -> AppResult<Session> {
    authorize(&webview)?;
    let state = Arc::clone(&state);
    blocking(move || {
        state
            .sessions
            .run_request(&request_id, "session_select", &session_id, || {
                state.surface.select_session(&session_id)
            })
    })
    .await?
}

#[tauri::command]
pub async fn session_end<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    request_id: String,
    session_id: String,
) -> AppResult<Session> {
    authorize(&webview)?;
    let state = Arc::clone(&state);
    blocking(move || {
        state
            .sessions
            .run_request(&request_id, "session_end", &session_id, || {
                let current = state.sessions.get(&session_id)?;
                if !current.state.is_active() {
                    return Ok(current);
                }
                state.sessions.begin_end(&session_id)?;
                state.terminal.end_session(&session_id);
                state.sessions.mark_ended(&session_id, false)
            })
    })
    .await?
}

#[tauri::command]
pub async fn session_end_all<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    request_id: String,
) -> AppResult<usize> {
    authorize(&webview)?;
    let state = Arc::clone(&state);
    blocking(move || {
        state
            .sessions
            .run_request(&request_id, "session_end_all", &(), || {
                let ids = state.sessions.active_ids();
                for id in &ids {
                    state.sessions.begin_end(id)?;
                    state.terminal.end_session(id);
                    state.sessions.mark_ended(id, false)?;
                }
                Ok(ids.len())
            })
    })
    .await?
}

#[tauri::command]
pub async fn session_delete<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    request_id: String,
    session_id: String,
    expected_revision: String,
) -> AppResult<()> {
    authorize(&webview)?;
    let expected: fleqi_domain::revision::Revision =
        serde_json::from_str(&format!("\"{expected_revision}\""))
            .map_err(|_| invalid("expectedRevision", "版本号必须是十进制字符串"))?;
    let state = Arc::clone(&state);
    blocking(move || {
        state
            .sessions
            .delete_request(&request_id, &session_id, expected, |id| {
                state.sessions.begin_end(id)?;
                state.terminal.end_session(id);
                state.sessions.mark_ended(id, false)?;
                Ok(())
            })
    })
    .await?
}

#[tauri::command]
pub async fn session_pin<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    session_id: String,
    pinned: bool,
    expected_revision: String,
) -> AppResult<Session> {
    authorize(&webview)?;
    let expected: fleqi_domain::revision::Revision =
        serde_json::from_str(&format!("\"{expected_revision}\""))
            .map_err(|_| invalid("expectedRevision", "版本号必须是十进制字符串"))?;
    let state = Arc::clone(&state);
    blocking(move || {
        state
            .sessions
            .set_pinned(&session_id, pinned, Some(expected))
    })
    .await?
}

#[tauri::command]
pub async fn session_continue<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    request_id: String,
    history_session_id: String,
) -> AppResult<Session> {
    authorize(&webview)?;
    let state = Arc::clone(&state);
    blocking(move || {
        let context = state.context.latest();
        state
            .sessions
            .continue_request(&request_id, &history_session_id, context.as_ref())
    })
    .await?
}

#[tauri::command]
pub async fn terminal_open<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    session_id: String,
    cols: Option<u16>,
    rows: Option<u16>,
) -> AppResult<String> {
    authorize(&webview)?;
    let state = Arc::clone(&state);
    blocking(move || {
        let context = state.context.latest();
        state.terminal.open(
            &session_id,
            context.as_ref(),
            cols.unwrap_or(100),
            rows.unwrap_or(30),
        )
    })
    .await?
}

#[tauri::command]
pub async fn terminal_snapshot<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    session_id: String,
) -> AppResult<fleqi_domain::terminal::TerminalSnapshot> {
    authorize(&webview)?;
    let state = Arc::clone(&state);
    blocking(move || state.terminal.snapshot(&session_id)).await?
}

#[tauri::command]
pub async fn terminal_input<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    session_id: String,
    lease: Option<String>,
    input: Vec<u8>,
) -> AppResult<()> {
    authorize(&webview)?;
    let state = Arc::clone(&state);
    blocking(move || state.terminal.input(&session_id, lease.as_deref(), &input)).await?
}

#[tauri::command]
pub async fn terminal_acquire_lease<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    session_id: String,
    owner: String,
) -> AppResult<String> {
    authorize(&webview)?;
    let state = Arc::clone(&state);
    blocking(move || state.terminal.acquire_lease(&session_id, &owner)).await?
}

#[tauri::command]
pub async fn terminal_resize<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> AppResult<()> {
    authorize(&webview)?;
    let state = Arc::clone(&state);
    blocking(move || state.terminal.resize(&session_id, cols, rows)).await?
}

/// 消费位点回执（流控/诊断；不改变重连与保留合同）。
#[tauri::command]
pub async fn terminal_ack<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    session_id: String,
    cursor: u64,
) -> AppResult<u64> {
    authorize(&webview)?;
    let state = Arc::clone(&state);
    blocking(move || state.terminal.ack(&session_id, cursor)).await?
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TerminalSubmitResult {
    Sent,
    Queued,
}

#[tauri::command]
pub async fn terminal_submit_line<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    request_id: String,
    session_id: String,
    line: String,
    context_revision: String,
    target_display: String,
) -> AppResult<TerminalSubmitResult> {
    authorize(&webview)?;
    if request_id.trim().is_empty() {
        return Err(invalid("requestId", "requestId 不能为空"));
    }
    // 服务端再验证一次模式标记：去掉首个半角 !，其余原样（FR-TERM-002）。
    let (mode, text) = fleqi_domain::composer::parse_composer_input(&line);
    if mode == ComposerMode::Ai {
        return Err(invalid("line", "手动终端命令必须以半角 ! 开头"));
    }
    if !fleqi_domain::composer::manual_command_is_submittable(&text) {
        return Err(invalid("line", "仅有标记和空白时不提交执行"));
    }
    let revision: fleqi_domain::revision::Revision =
        serde_json::from_str(&format!("\"{context_revision}\""))
            .map_err(|_| invalid("contextRevision", "版本号必须是十进制字符串"))?;
    let state = Arc::clone(&state);
    blocking(move || {
        let outcome = state.terminal.submit_line(
            &request_id,
            &session_id,
            &text,
            revision,
            &target_display,
            None,
        )?;
        Ok(match outcome {
            SubmitOutcome::Sent => TerminalSubmitResult::Sent,
            SubmitOutcome::Queued { .. } => TerminalSubmitResult::Queued,
        })
    })
    .await?
}

#[tauri::command]
pub async fn terminal_cancel_queued<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    session_id: String,
) -> AppResult<Option<fleqi_domain::directory_sync::QueuedLine>> {
    authorize(&webview)?;
    let state = Arc::clone(&state);
    blocking(move || state.terminal.cancel_queued(&session_id)).await
}

#[tauri::command]
pub async fn terminal_withdrawn_line<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    session_id: String,
) -> AppResult<Option<fleqi_domain::directory_sync::QueuedLine>> {
    authorize(&webview)?;
    let state = Arc::clone(&state);
    blocking(move || state.terminal.take_withdrawn(&session_id)).await
}

/// 终端流事件（Channel 线格式）：字节 + 游标，或控制事件摘要。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum TerminalStreamEvent {
    Output { bytes: Vec<u8>, cursor: u64 },
    PromptReady { cwd: String },
    Exited,
}

#[tauri::command]
pub async fn terminal_unsubscribe<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    session_id: String,
    subscription: u64,
) -> AppResult<()> {
    authorize(&webview)?;
    state.terminal.unsubscribe(&session_id, subscription)
}

#[tauri::command]
pub async fn terminal_release_lease<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    session_id: String,
    lease: String,
) -> AppResult<()> {
    authorize(&webview)?;
    state.terminal.release_lease(&session_id, &lease);
    Ok(())
}

/// 终端输出订阅：Channel 推送带流游标的字节；旧游标越界返回 conflict（要求先取快照）。
#[tauri::command]
pub async fn terminal_subscribe<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    session_id: String,
    cursor: u64,
    on_event: Channel<TerminalStreamEvent>,
) -> AppResult<u64> {
    authorize(&webview)?;
    let state = Arc::clone(&state);
    blocking(move || {
        let (tx, rx) = std::sync::mpsc::channel();
        let subscription = state.terminal.subscribe(&session_id, cursor, tx)?;
        std::thread::spawn(move || {
            for event in rx {
                let wire = match event {
                    fleqi_application::ports::TerminalEvent::Output { bytes, cursor } => {
                        TerminalStreamEvent::Output { bytes, cursor }
                    }
                    fleqi_application::ports::TerminalEvent::PromptReady { cwd } => {
                        TerminalStreamEvent::PromptReady { cwd }
                    }
                    fleqi_application::ports::TerminalEvent::Exited { .. } => {
                        TerminalStreamEvent::Exited
                    }
                    _ => continue,
                };
                if on_event.send(wire).is_err() {
                    break;
                }
            }
        });
        Ok(subscription)
    })
    .await?
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyState {
    pub registered: Option<String>,
    pub message: Option<String>,
}

#[tauri::command]
pub async fn hotkey_commit<R: Runtime>(
    webview: Webview<R>,
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    request_id: String,
    accelerator: String,
) -> AppResult<HotkeyState> {
    authorize(&webview)?;
    let _ = request_id;
    let state = Arc::clone(&state);
    let hotkey_state: HotkeyState = blocking(move || {
        use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};
        let shortcuts = app.global_shortcut();
        // 先解除当前生效键，再注册候选；失败保留旧值并返回真实原因。
        let current = state.hotkey.lock().expect("hotkey").clone();
        let parsed = match Shortcut::try_from(accelerator.as_str()) {
            Ok(parsed) => parsed,
            Err(error) => {
                return HotkeyState {
                    registered: current,
                    message: Some(format!("无法识别的快捷键：{error}")),
                };
            }
        };
        if let Some(shortcut) = current
            .as_deref()
            .and_then(|old| Shortcut::try_from(old).ok())
        {
            let _ = shortcuts.unregister(shortcut);
        }
        state
            .hotkey_down
            .store(false, std::sync::atomic::Ordering::Release);
        match shortcuts.on_shortcut(parsed, handle_shortcut) {
            Ok(()) => {
                *state.hotkey.lock().expect("hotkey") = Some(accelerator.clone());
                // 只有真实注册成功的候选才成为有效绑定并持久化（FR-SET-002/003）。
                let hotkey = parse_accelerator(&accelerator);
                let revision = state.settings.snapshot().revision;
                let patch = fleqi_domain::settings::SettingsPatch {
                    hotkey: Some(hotkey),
                    ..Default::default()
                };
                let persisted =
                    state
                        .settings
                        .update(fleqi_application::dto::SettingsUpdateRequest {
                            // 幂等 ID 携带调用方 requestId：不同窗口先后提交同一加速键
                            // （expectedRevision 不同）不得被判为"同 ID 不同载荷"冲突。
                            request_id: format!("hotkey-commit-{}-{}", request_id, accelerator),
                            expected_revision: revision,
                            patch,
                        });
                match persisted {
                    Ok(_) => state.surface.set_hotkey_registered(true),
                    Err(error) => {
                        state.logger.log(
                            "error",
                            "hotkey.persist-failed",
                            &[("message", &error.message)],
                        );
                        if let Ok(shortcut) = Shortcut::try_from(accelerator.as_str()) {
                            let _ = shortcuts.unregister(shortcut);
                        }
                        let restored = current
                            .as_deref()
                            .and_then(|value| Shortcut::try_from(value).ok())
                            .is_some_and(|shortcut| {
                                shortcuts.on_shortcut(shortcut, handle_shortcut).is_ok()
                            });
                        let active = if restored { current.clone() } else { None };
                        *state.hotkey.lock().expect("hotkey") = active.clone();
                        state.surface.set_hotkey_registered(restored);
                        return HotkeyState {
                            registered: active,
                            message: Some(format!("快捷键未保存：{}", error.message)),
                        };
                    }
                }
                state
                    .logger
                    .log("info", "hotkey.registered", &[("code", "hotkey")]);
                apply_settings_side_effects(&app, &state);
                HotkeyState {
                    registered: Some(accelerator),
                    message: None,
                }
            }
            Err(error) => {
                let restored = current
                    .as_deref()
                    .and_then(|value| Shortcut::try_from(value).ok())
                    .is_some_and(|shortcut| {
                        shortcuts.on_shortcut(shortcut, handle_shortcut).is_ok()
                    });
                let active = if restored { current } else { None };
                *state.hotkey.lock().expect("hotkey") = active.clone();
                state.surface.set_hotkey_registered(restored);
                HotkeyState {
                    registered: active,
                    message: Some(format!(
                        "注册失败：{error}；{}",
                        if restored {
                            "原快捷键已恢复"
                        } else {
                            "请重新设置快捷键"
                        }
                    )),
                }
            }
        }
    })
    .await?;
    Ok(hotkey_state)
}

#[tauri::command]
pub async fn hotkey_clear<R: Runtime>(
    webview: Webview<R>,
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    request_id: String,
) -> AppResult<HotkeyState> {
    authorize(&webview)?;
    let _ = request_id;
    let state = Arc::clone(&state);
    let cleared: HotkeyState = blocking(move || {
        use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};
        if let Ok(shortcut) = Shortcut::try_from(
            state
                .hotkey
                .lock()
                .expect("hotkey")
                .take()
                .as_deref()
                .unwrap_or_default(),
        ) {
            let _ = app.global_shortcut().unregister(shortcut);
        }
        state.surface.set_hotkey_registered(false);
        HotkeyState {
            registered: None,
            message: None,
        }
    })
    .await?;
    Ok(cleared)
}

#[tauri::command]
pub async fn hotkey_get<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
) -> AppResult<HotkeyState> {
    authorize(&webview)?;
    let state = Arc::clone(&state);
    blocking(move || HotkeyState {
        registered: state.hotkey.lock().expect("hotkey").clone(),
        message: None,
    })
    .await
}

/// 解析 accelerator（如 `CommandOrControl+Shift+F`）为持久化的 Hotkey 结构。
fn parse_accelerator(accelerator: &str) -> Option<fleqi_domain::settings::Hotkey> {
    use fleqi_domain::settings::HotkeyModifier;
    let mut modifiers = Vec::new();
    let mut key = String::new();
    for part in accelerator.split('+') {
        match part {
            "CommandOrControl" | "Command" | "Cmd" | "Meta" | "Super" => {
                modifiers.push(HotkeyModifier::Command)
            }
            "Option" | "Alt" => modifiers.push(HotkeyModifier::Option),
            "Control" | "Ctrl" => modifiers.push(HotkeyModifier::Control),
            "Shift" => modifiers.push(HotkeyModifier::Shift),
            other if !other.is_empty() => key = other.to_owned(),
            _ => {}
        }
    }
    if key.is_empty() {
        None
    } else {
        Some(fleqi_domain::settings::Hotkey { key, modifiers })
    }
}

fn handle_shortcut(
    app: &tauri::AppHandle,
    _shortcut: &tauri_plugin_global_shortcut::Shortcut,
    event: tauri_plugin_global_shortcut::ShortcutEvent,
) {
    if crate::updates::is_installing(app) {
        return;
    }
    let Some(state) = app.try_state::<Arc<AppState>>() else {
        return;
    };
    let pressed = event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed;
    let was_pressed = state
        .hotkey_down
        .swap(pressed, std::sync::atomic::Ordering::AcqRel);
    if !pressed || was_pressed {
        return;
    }
    if state.surface.visibility() == fleqi_domain::surface::Visibility::Visible {
        let _ = state.surface.user_hide();
    } else {
        let state = Arc::clone(&state);
        let app = app.clone();
        std::thread::spawn(move || {
            state.context.refresh();
            state
                .composer_focus_requested
                .store(true, std::sync::atomic::Ordering::Release);
            if state.surface.user_show().is_ok() {
                let _ = crate::windows::show_composer_attached(&app, true);
            } else {
                state
                    .composer_focus_requested
                    .store(false, std::sync::atomic::Ordering::Release);
            }
        });
    }
}

/// Stored shortcuts must be registered on every process start, not only when edited.
pub fn restore_hotkey(app: &AppHandle, state: &AppState) {
    use fleqi_domain::settings::HotkeyModifier;
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};
    let Some(hotkey) = state.settings.snapshot().settings.hotkey else {
        return;
    };
    let mut parts: Vec<String> = hotkey
        .modifiers
        .iter()
        .map(|modifier| {
            match modifier {
                HotkeyModifier::Command => "Command",
                HotkeyModifier::Control => "Control",
                HotkeyModifier::Option => "Alt",
                HotkeyModifier::Shift => "Shift",
            }
            .to_owned()
        })
        .collect();
    parts.push(hotkey.key);
    let accelerator = parts.join("+");
    let result = Shortcut::try_from(accelerator.as_str())
        .map_err(|error| error.to_string())
        .and_then(|shortcut| {
            app.global_shortcut()
                .on_shortcut(shortcut, handle_shortcut)
                .map_err(|error| error.to_string())
        });
    match result {
        Ok(()) => {
            *state.hotkey.lock().expect("hotkey") = Some(accelerator);
            state.surface.set_hotkey_registered(true);
        }
        Err(message) => {
            state
                .logger
                .log("warn", "hotkey.restore-failed", &[("message", &message)]);
            state.surface.set_hotkey_registered(false);
        }
    }
}

/// 设置保存后同步宿主侧显示条件与热键/自启动状态（由 settings_update 命令调用）。
pub fn apply_settings_side_effects(_app: &AppHandle, state: &AppState) {
    for label in ["console", "settings", "composer"] {
        crate::windows::apply_window_material(_app, label);
    }
    let snapshot = state.settings.snapshot();
    state.surface.set_settings_for_interaction(
        &snapshot.settings,
        crate::windows::finder_interaction_active(_app)
            && fleqi_platform::host::file_manager_frame().has_window,
    );
    // 热键：设置中的 hotkey 与宿主注册状态一致才视为有效绑定（比较解析后的键与修饰符）。
    let registered = state
        .hotkey
        .lock()
        .expect("hotkey")
        .clone()
        .and_then(|a| parse_accelerator(&a));
    let matches_setting = match (&snapshot.settings.hotkey, &registered) {
        (Some(setting), Some(active)) => {
            setting.key == active.key && setting.modifiers == active.modifiers
        }
        (None, None) => true,
        _ => false,
    };
    state
        .surface
        .set_hotkey_registered(matches_setting && snapshot.settings.hotkey.is_some());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str) -> Url {
        Url::parse(text).expect("测试 URL 合法")
    }

    #[test]
    fn accepts_packaged_local_origins() {
        assert!(is_local_app_url(&url("tauri://localhost/index.html")));
        assert!(is_local_app_url(&url("http://tauri.localhost/")));
    }

    #[test]
    fn rejects_external_and_file_origins() {
        assert!(!is_local_app_url(&url("https://example.com/")));
        assert!(!is_local_app_url(&url("http://localhost:1420/")));
        assert!(!is_local_app_url(&url("file:///Users/someone/index.html")));
        assert!(!is_local_app_url(&url("tauri://evil/")));
        assert!(!is_local_app_url(&url("javascript:alert(1)")));
    }

    #[test]
    fn dev_server_only_in_debug_builds() {
        assert_eq!(
            is_local_app_url(&url("http://127.0.0.1:1420/")),
            cfg!(debug_assertions)
        );
        assert!(!is_local_app_url(&url("http://127.0.0.1:1421/")));
    }
}
