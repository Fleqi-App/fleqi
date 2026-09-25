//! M3 IPC（architecture.md §12.5）：Run 提交/确认/取消/查询、规则、收藏、
//! 输入历史与模型端点探测。全部先核对调用窗口与本地 origin。

use fleqi_application::dto::{AppError, AppResult};
use fleqi_domain::revision::Revision;
use std::sync::Arc;
use tauri::{Runtime, State, Webview};

use crate::state::{AppState, InstallJob};
use crate::windows::WindowRole;

use crate::commands::{authorize, blocking, invalid};

#[tauri::command]
pub async fn capability_form<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    capability_id: String,
    context_id: Option<String>,
) -> AppResult<fleqi_application::capability_service::CapabilityForm> {
    authorize(&webview)?;
    let context_service = Arc::clone(&state.context);
    let context = blocking(move || match context_id {
        Some(id) => context_service.get(Some(&id)),
        None => Ok(context_service.refresh()),
    })
    .await??;
    fleqi_application::capability_service::form_with_settings(
        &capability_id,
        context,
        &state.settings.snapshot().settings,
    )
}

#[tauri::command]
pub async fn capability_submit<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    request_id: String,
    session_id: String,
    capability_id: String,
    context_id: String,
    parameters: std::collections::BTreeMap<String, String>,
) -> AppResult<fleqi_application::run_service::RunRecord> {
    authorize(&webview)?;
    let context_service = Arc::clone(&state.context);
    let context = blocking(move || context_service.validate_current(&context_id)).await??;
    let directory_ref = context
        .directory_ref
        .as_ref()
        .ok_or_else(|| AppError::unavailable("请先选择工作文件夹"))?;
    let directory = state
        .paths
        .resolve(&directory_ref.id)
        .ok_or_else(|| AppError::not_found("目录引用已失效"))?;
    let form = fleqi_application::capability_service::form_with_settings(
        &capability_id,
        context,
        &state.settings.snapshot().settings,
    )?;
    let mut submit = fleqi_application::capability_service::plan(
        &form,
        &parameters,
        session_id,
        directory,
        state.settings.snapshot().settings.ai_policy,
    )?;
    if let Some(entry) = crate::catalog::builtin_catalog()
        .into_iter()
        .find(|entry| entry.id == capability_id)
    {
        submit.prompt = if form.minimum_inputs == 0 && form.context.selected_items.is_empty() {
            entry.title
        } else {
            format!(
                "{} · {} 项输入",
                entry.title,
                form.context.selected_items.len()
            )
        };
    }
    let mut sealed = parameters.clone();
    for field in &form.fields {
        if field.kind == "password"
            && let Some(value) = sealed.get_mut(&field.key)
            && !value.is_empty()
        {
            *value = fleqi_application::secrets::store(std::mem::take(value));
        }
    }
    if form.fields.iter().any(|field| field.kind == "password") {
        let args = serde_json::to_string(&sealed)
            .map_err(|error| AppError::internal(error.to_string()))?;
        for step in &mut submit.plan.steps {
            step.args = vec![args.clone()];
        }
        submit.plan.source_fingerprint = fleqi_application::fingerprint::fingerprint(&sealed);
    }
    fleqi_application::capability_service::bind_output_settings(
        &mut submit,
        &state.settings.snapshot().settings,
        &state.paths,
    )?;
    let cleanup = submit.plan.clone();
    let runs = state.runs.clone();
    let result = blocking(move || runs.submit(&request_id, submit)).await;
    if !matches!(&result, Ok(Ok(_))) {
        fleqi_application::secrets::release_plan(&cleanup);
    }
    result?
}

#[allow(non_snake_case)]
fn invalid_field(field: &str, message: impl Into<String>) -> fleqi_application::dto::AppError {
    invalid(field, message)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanWire {
    pub revision: String,
    pub context_id: String,
    pub scripts: Vec<String>,
    /// 载荷效果：read | create | modify | delete | install | unknown | …
    pub effects: Vec<String>,
    pub preview_complete: Option<bool>,
}

fn plan_from_wire(
    wire: PlanWire,
) -> Result<fleqi_domain::execution::ExecutionPlan, fleqi_application::dto::AppError> {
    use fleqi_domain::execution::*;
    let revision = parse_revision(&wire.revision)?;
    let effects = wire
        .effects
        .iter()
        .map(|name| Effect {
            kind: match name.as_str() {
                "read" => EffectKind::Read,
                "create" => EffectKind::Create,
                "modify" | "rename" | "move" => EffectKind::Modify,
                "delete" => EffectKind::Delete,
                "install" => EffectKind::Install,
                _ => EffectKind::Unknown,
            },
            source_ref: None,
            destination_ref: None,
            explanation: name.clone(),
        })
        .collect();
    let steps = wire
        .scripts
        .into_iter()
        .map(|script| ExecutionStep {
            kind: StepKind::Script,
            operation: String::new(),
            executable_ref: None,
            script: Some(script),
            args: vec![],
            cwd_ref: None,
            env_refs: vec![],
            input_refs: vec![],
            expected_outputs: vec![],
        })
        .collect();
    Ok(ExecutionPlan {
        id: "plan-inline".into(),
        revision,
        capability_id: None,
        context_id: wire.context_id,
        steps,
        required_tools: vec![],
        effects,
        preview_completeness: if wire.preview_complete.unwrap_or(false) {
            PlanPreviewCompleteness::Complete
        } else {
            PlanPreviewCompleteness::Unknown
        },
        source_fingerprint: String::new(),
    })
}

fn parse_revision(text: &str) -> Result<Revision, fleqi_application::dto::AppError> {
    serde_json::from_str(&format!("\"{text}\""))
        .map_err(|_| invalid_field("revision", "版本号必须是十进制字符串"))
}

#[tauri::command]
pub async fn run_submit<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    request_id: String,
    session_id: String,
    prompt: String,
    plan: PlanWire,
) -> AppResult<fleqi_application::run_service::RunRecord> {
    authorize(&webview)?;
    if prompt.trim().is_empty() {
        return Err(invalid_field("prompt", "提示不能为空"));
    }
    let policy = state.settings.snapshot().settings.ai_policy;
    let context = state.context.get(Some(&plan.context_id))?;
    let directory_ref = context
        .directory_ref
        .as_ref()
        .ok_or_else(|| AppError::unavailable("请选择有效工作目录"))?;
    let working_directory = state
        .paths
        .resolve(&directory_ref.id)
        .ok_or_else(|| AppError::unavailable("工作目录引用已失效"))?;
    let submit = fleqi_application::run_service::RunSubmit {
        working_directory,
        session_id,
        prompt,
        plan: plan_from_wire(plan)?,
        policy,
    };
    let runs = Arc::clone(&state.runs);
    blocking(move || runs.submit(&request_id, submit)).await?
}

#[tauri::command]
pub async fn run_approve<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    request_id: String,
    run_id: String,
    plan_revision: String,
) -> AppResult<fleqi_application::run_service::RunRecord> {
    authorize(&webview)?;
    let revision = parse_revision(&plan_revision)?;
    let runs = Arc::clone(&state.runs);
    blocking(move || runs.approve(&request_id, &run_id, revision)).await?
}

#[tauri::command]
pub async fn run_cancel<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    run_id: String,
) -> AppResult<fleqi_application::run_service::RunRecord> {
    authorize(&webview)?;
    let runs = Arc::clone(&state.runs);
    blocking(move || runs.cancel(&run_id)).await?
}

#[tauri::command]
pub async fn run_get<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    run_id: String,
) -> AppResult<fleqi_application::run_service::RunRecord> {
    authorize(&webview)?;
    let runs = Arc::clone(&state.runs);
    blocking(move || runs.get(&run_id)).await?
}

#[tauri::command]
pub async fn run_plan_get<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    run_id: String,
) -> AppResult<fleqi_domain::execution::ExecutionPlan> {
    authorize(&webview)?;
    let runs = Arc::clone(&state.runs);
    blocking(move || runs.plan(&run_id)).await?
}

#[tauri::command]
pub async fn run_list<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    session_id: String,
) -> AppResult<Vec<fleqi_application::run_service::RunRecord>> {
    authorize(&webview)?;
    let runs = Arc::clone(&state.runs);
    blocking(move || runs.list(&session_id)).await?
}

#[tauri::command]
pub async fn run_retry<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    request_id: String,
    run_id: String,
) -> AppResult<fleqi_application::run_service::RunRecord> {
    authorize(&webview)?;
    let runs = Arc::clone(&state.runs);
    if let Some(previous) = runs.retry_receipt(&request_id, &run_id)? {
        return Ok(previous);
    }
    let context = state.context.get(None)?;
    let directory_ref = context
        .directory_ref
        .as_ref()
        .ok_or_else(|| AppError::unavailable("请选择有效工作目录"))?;
    let directory = state
        .paths
        .resolve(&directory_ref.id)
        .ok_or_else(|| AppError::unavailable("工作目录引用已失效"))?;
    let sessions = state.sessions.clone();
    blocking(move || {
        let original = runs.get(&run_id)?;
        if !original.state.is_terminal() {
            return Err(AppError::conflict("请先等待任务结束或取消", None));
        }
        if original
            .plan
            .as_ref()
            .is_some_and(|plan| plan.capability_id.is_some())
        {
            return Err(AppError::unavailable("请从能力表单确认当前输入后重新提交"));
        }
        let source = sessions.get(&original.session_id)?;
        let session_id = if source.state.is_active() {
            None
        } else {
            Some(
                sessions
                    .continue_request(
                        &format!("retry-session/{request_id}"),
                        &source.id,
                        Some(&context),
                    )?
                    .id,
            )
        };
        runs.retry_in_session(&request_id, &run_id, &context.id, directory, session_id)
    })
    .await?
}

#[tauri::command]
pub async fn rules_create<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    name: String,
    content: String,
    scope: Option<fleqi_application::collection_service::RuleScope>,
) -> AppResult<fleqi_application::collection_service::Rule> {
    authorize(&webview)?;
    let collections = state.collections.clone();
    blocking(move || collections.create_rule(name, content, scope)).await?
}

#[tauri::command]
pub async fn rules_list<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
) -> AppResult<Vec<fleqi_application::collection_service::Rule>> {
    authorize(&webview)?;
    let collections = state.collections.clone();
    blocking(move || collections.list_rules()).await?
}

#[tauri::command]
pub async fn rules_update<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    rule_id: String,
    name: Option<String>,
    enabled: Option<bool>,
    content: Option<String>,
) -> AppResult<fleqi_application::collection_service::Rule> {
    authorize(&webview)?;
    let collections = state.collections.clone();
    blocking(move || collections.update_rule(&rule_id, name, enabled, content)).await?
}

#[tauri::command]
pub async fn rules_delete<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    rule_id: String,
) -> AppResult<()> {
    authorize(&webview)?;
    let collections = state.collections.clone();
    blocking(move || collections.delete_rule(&rule_id)).await?
}

#[tauri::command]
pub async fn favorites_create<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    name: String,
    content: String,
    kind: String,
) -> AppResult<fleqi_application::collection_service::Favorite> {
    authorize(&webview)?;
    let collections = state.collections.clone();
    blocking(move || collections.create_favorite(name, content, kind)).await?
}

#[tauri::command]
pub async fn favorites_list<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
) -> AppResult<Vec<fleqi_application::collection_service::Favorite>> {
    authorize(&webview)?;
    let collections = state.collections.clone();
    blocking(move || collections.list_favorites()).await?
}

#[tauri::command]
pub async fn favorites_update<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    favorite_id: String,
    name: Option<String>,
    tags: Option<Vec<String>>,
) -> AppResult<fleqi_application::collection_service::Favorite> {
    authorize(&webview)?;
    let collections = state.collections.clone();
    blocking(move || collections.update_favorite(&favorite_id, name, tags)).await?
}

#[tauri::command]
pub async fn favorites_delete<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    favorite_id: String,
) -> AppResult<()> {
    authorize(&webview)?;
    let collections = state.collections.clone();
    blocking(move || collections.delete_favorite(&favorite_id)).await?
}

#[tauri::command]
pub async fn history_append<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    entry: String,
) -> AppResult<()> {
    authorize(&webview)?;
    let collections = state.collections.clone();
    blocking(move || collections.append_history(&entry)).await?
}

#[tauri::command]
pub async fn history_list<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
) -> AppResult<Vec<String>> {
    authorize(&webview)?;
    let collections = state.collections.clone();
    blocking(move || collections.list_history()).await?
}

#[tauri::command]
pub async fn history_clear<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
) -> AppResult<()> {
    authorize(&webview)?;
    let collections = state.collections.clone();
    blocking(move || collections.clear_history()).await?
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderProbeResult {
    pub ok: bool,
    pub models: Vec<String>,
    pub error: Option<String>,
}

/// provider_probe：只验证用户配置（FR-AI-003）；密钥经 Rust 直发用户端点。
#[tauri::command]
pub async fn provider_probe<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    base_url: String,
    api_key: Option<String>,
    timeout_ms: Option<u64>,
) -> AppResult<ProviderProbeResult> {
    authorize(&webview)?;
    if base_url.trim().is_empty() {
        return Err(invalid_field("baseUrl", "端点地址不能为空"));
    }
    fleqi_application::provider_service::ProviderService::validate_base_url(&base_url)
        .map_err(|message| invalid_field("baseUrl", &message))?;
    let config = fleqi_adapters::model::ProviderConfig {
        id: "probe".into(),
        display_name: "探测".into(),
        base_url,
        api_key,
        model: "probe".into(),
        timeout_ms: timeout_ms.unwrap_or(10_000),
    };
    let adapter = fleqi_adapters::model::OpenAiCompatibleAdapter::new(config);
    let _ = state;
    let probe = blocking(move || <fleqi_adapters::model::OpenAiCompatibleAdapter as fleqi_adapters::model::ModelAdapter>::probe(&adapter)).await?;
    Ok(match probe {
        fleqi_adapters::model::ProbeOutcome::Ok { models } => ProviderProbeResult {
            ok: true,
            models,
            error: None,
        },
        fleqi_adapters::model::ProbeOutcome::AuthFailed => ProviderProbeResult {
            ok: false,
            models: vec![],
            error: Some("认证失败：密钥无效或已过期".into()),
        },
        fleqi_adapters::model::ProbeOutcome::Network { message } => ProviderProbeResult {
            ok: false,
            models: vec![],
            error: Some(message),
        },
    })
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub id: String,
    pub category: String,
    pub title: String,
    pub description: String,
    pub inputs: String,
    pub dependencies: Vec<String>,
}

/// catalog_query：六类 30 项基础能力目录（能力台账 §2；参数化数据）。
#[tauri::command]
pub async fn catalog_query<R: Runtime>(webview: Webview<R>) -> AppResult<Vec<CatalogEntry>> {
    authorize(&webview)?;
    let _role: Option<WindowRole> = None;
    Ok(crate::catalog::builtin_catalog())
}

/// tools_list：检测真实状态（系统 PATH/受管目录 + 检测参数真实执行；FR-TOOLS-001）。
#[tauri::command]
pub async fn tools_list<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
) -> AppResult<Vec<fleqi_application::tool_service::ToolEntry>> {
    authorize(&webview)?;
    let tools = Arc::clone(&state.tools);
    blocking(move || tools.list()).await?
}

#[tauri::command]
pub fn tools_prepare<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
) -> AppResult<fleqi_application::tool_service::ToolPreparation> {
    authorize(&webview)?;
    Ok(state.tools.prepare())
}

#[tauri::command]
pub fn tools_prepare_status<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
) -> AppResult<fleqi_application::tool_service::ToolPreparation> {
    authorize(&webview)?;
    Ok(state.tools.preparation())
}

#[tauri::command]
pub fn tools_prepare_cancel<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
) -> AppResult<()> {
    authorize(&webview)?;
    state.tools.cancel_preparation();
    Ok(())
}

/// tools_install：复用已有系统工具或安装固定清单；进度/取消经 install_jobs 共享。
#[tauri::command]
pub async fn tools_install<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    request_id: String,
    tool_id: String,
) -> AppResult<fleqi_application::tool_service::ToolEntry> {
    authorize(&webview)?;
    if request_id.trim().is_empty() {
        return Err(invalid_field("requestId", "requestId 不能为空"));
    }
    let tools = Arc::clone(&state.tools);
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let progress: Arc<std::sync::Mutex<fleqi_application::ports::InstallProgress>> = Arc::new(
        std::sync::Mutex::new(fleqi_application::ports::InstallProgress::Verifying),
    );
    state.install_jobs.lock().expect("install jobs").insert(
        request_id.clone(),
        InstallJob {
            cancel: Arc::clone(&cancel),
            progress: Arc::clone(&progress),
        },
    );
    let result = blocking(move || {
        tools.install(&tool_id, &cancel, &|stage| {
            *progress.lock().expect("install progress") = stage;
        })
    })
    .await;
    state
        .install_jobs
        .lock()
        .expect("install jobs")
        .remove(&request_id);
    result?
}

/// tools_install_status：轮询在途安装进度（未知 requestId 返回 null）。
#[tauri::command]
pub fn tools_install_status<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    request_id: String,
) -> AppResult<Option<fleqi_application::ports::InstallProgress>> {
    authorize(&webview)?;
    Ok(state
        .install_jobs
        .lock()
        .expect("install jobs")
        .get(&request_id)
        .map(|job| job.progress.lock().expect("install progress").clone()))
}

/// tools_install_cancel：取消在途安装（下载分块间生效）；无在途任务返回 false。
#[tauri::command]
pub fn tools_install_cancel<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    request_id: String,
) -> AppResult<bool> {
    authorize(&webview)?;
    Ok(state
        .install_jobs
        .lock()
        .expect("install jobs")
        .get(&request_id)
        .map(|job| {
            job.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        })
        .is_some())
}

/// tools_remove：只卸载应用拥有的受管包（FR-TOOLS-003）。
#[tauri::command]
pub async fn tools_remove<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    tool_id: String,
) -> AppResult<()> {
    authorize(&webview)?;
    let tools = Arc::clone(&state.tools);
    blocking(move || tools.remove(&tool_id)).await?
}

/// provider_save：端点配置入库；密钥只写入系统凭据服务，返回掩码视图（FR-AI-003）。
#[tauri::command]
pub async fn provider_save<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    request: fleqi_application::provider_service::ProviderSaveRequest,
) -> AppResult<fleqi_application::provider_service::ProviderView> {
    authorize(&webview)?;
    let providers = Arc::clone(&state.providers);
    blocking(move || providers.save(request)).await?
}

/// provider_list：端点列表（记录 + 密钥已配置标志；不回传密钥）。
#[tauri::command]
pub async fn provider_list<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
) -> AppResult<Vec<fleqi_application::provider_service::ProviderView>> {
    authorize(&webview)?;
    let providers = Arc::clone(&state.providers);
    blocking(move || providers.list()).await?
}

/// provider_delete：删除端点记录并清理对应密钥。
#[tauri::command]
pub async fn provider_delete<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    provider_id: String,
) -> AppResult<()> {
    authorize(&webview)?;
    let providers = Arc::clone(&state.providers);
    blocking(move || providers.delete(&provider_id)).await?
}

/// run_plan_submit：规划闭环入口——模型生成结构化计划后按当前 aiPolicy 提交 Run；
/// 两次仍不合法时回退摘要文本（不执行）。取消标志在 M4 接 UI。
#[tauri::command]
pub async fn run_plan_submit<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    request_id: String,
    session_id: String,
    context_id: String,
    prompt: String,
) -> AppResult<fleqi_application::planning_service::PlanOutcome> {
    authorize(&webview)?;
    if request_id.trim().is_empty() {
        return Err(invalid_field("requestId", "requestId 不能为空"));
    }
    if let Some(record) =
        state
            .runs
            .request_record(&request_id, &session_id, &context_id, &prompt)?
    {
        return Ok(fleqi_application::planning_service::PlanOutcome::Execute {
            run: Box::new(record),
        });
    }
    if state.sessions.get(&session_id)?.state != fleqi_domain::session::SessionState::Active {
        return Err(AppError::conflict("会话已结束", None));
    }
    let context_service = Arc::clone(&state.context);
    let expected_context_id = context_id.clone();
    let context =
        blocking(move || context_service.validate_current(&expected_context_id)).await??;
    let directory_ref = context
        .directory_ref
        .as_ref()
        .ok_or_else(|| AppError::unavailable("请选择有效工作目录"))?;
    let working_directory = state
        .paths
        .resolve(&directory_ref.id)
        .ok_or_else(|| AppError::unavailable("工作目录引用已失效"))?;
    let planning = Arc::clone(&state.planning);
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let mut jobs = state.planning_cancels.lock().expect("planning cancels");
        if jobs.contains_key(&request_id) {
            return Err(AppError::conflict("该请求正在规划，请等待原请求", None));
        }
        jobs.insert(
            request_id.clone(),
            crate::state::PlanningJob {
                session_id: session_id.clone(),
                cancel: Arc::clone(&cancel),
            },
        );
    }
    let matched_rules = state
        .collections
        .matching_rules(&directory_ref.display_path, &prompt);
    let planning_context = fleqi_application::planning_service::PlanningContext {
        paths: Some(state.paths.clone()),
        working_directory,
        snapshot: serde_json::to_value(&context)
            .map_err(|error| AppError::internal(error.to_string()))?,
        rules: serde_json::to_value(matched_rules)
            .map_err(|error| AppError::internal(error.to_string()))?,
        capabilities: serde_json::to_value(crate::catalog::builtin_catalog())
            .map_err(|error| AppError::internal(error.to_string()))?,
    };
    let conversation_session = session_id.clone();
    let conversation_prompt = prompt.clone();
    let request_id_inner = request_id.clone();
    let result = blocking(move || {
        planning.plan_and_submit(
            &request_id_inner,
            &session_id,
            &context_id,
            &prompt,
            &cancel,
            planning_context,
        )
    })
    .await;
    state
        .planning_cancels
        .lock()
        .expect("planning cancels")
        .remove(&request_id);
    let outcome = result??;
    state.sessions.append_entry(
        &conversation_session,
        fleqi_domain::session::EntryRole::User,
        &conversation_prompt,
        None,
    )?;
    match &outcome {
        fleqi_application::planning_service::PlanOutcome::Summary { text, .. } => {
            state.sessions.append_entry(
                &conversation_session,
                fleqi_domain::session::EntryRole::Assistant,
                text,
                None,
            )?;
        }
        fleqi_application::planning_service::PlanOutcome::Execute { run } => {
            state.sessions.append_entry(
                &conversation_session,
                fleqi_domain::session::EntryRole::Assistant,
                "已生成执行计划；任务详情保存实际步骤、状态与输出。",
                Some(run.id.clone()),
            )?;
        }
    }
    Ok(outcome)
}

/// run_plan_cancel：取消在途规划（模型块间生效）；无在途请求报 not_found。
#[tauri::command]
pub fn run_plan_cancel<R: Runtime>(
    webview: Webview<R>,
    state: State<'_, Arc<AppState>>,
    request_id: String,
) -> AppResult<()> {
    authorize(&webview)?;
    let cancel = state
        .planning_cancels
        .lock()
        .expect("planning cancels")
        .get(&request_id)
        .map(|job| job.cancel.clone());
    match cancel {
        Some(flag) => {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
            Ok(())
        }
        None => Err(AppError::not_found(format!(
            "没有进行中的规划请求 {request_id}"
        ))),
    }
}
