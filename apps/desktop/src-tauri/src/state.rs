//! 宿主装配（architecture.md §12.2）：单实例（main.rs 插件）→ 路径与脱敏日志 →
//! 数据库/迁移/设置 → 用例与原生适配 → IPC/菜单/按需窗口 → 无提示异步自检。
//! 本模块只创建与连接模块，不承载业务行为。

use fleqi_adapters::logging::SafeLogger;
use fleqi_adapters::storage::{Database, SqliteSettingsStore};
use fleqi_application::context_service::ContextService;
use fleqi_application::dto::{
    AppBootstrap, AppEvent, BuildInfo, CredentialStoreStatus, DiagnosticsSnapshot, StorageState,
    StorageStatus,
};
use fleqi_application::lifecycle::HostLifecycle;
use fleqi_application::paths::PathRegistry;
use fleqi_application::permission_service::PermissionService;
use fleqi_application::platform_caps::derive_capabilities;
use fleqi_application::ports::{
    CredentialPort, EventSink, SessionStore, StorageError, SystemClock, ThreadSpawner,
};
use fleqi_application::session_service::SessionService;
use fleqi_application::settings_service::SettingsService;
use fleqi_application::surface_service::{SurfaceDeps, SurfaceService};
use fleqi_application::terminal_service::TerminalService;
use fleqi_domain::lifecycle::HostState;
use fleqi_domain::revision::Revision;
use fleqi_domain::settings::FieldAllowlist;
use fleqi_platform::credentials::self_test;
use fleqi_platform::scheduling::MainThreadExecutor;

#[cfg(target_os = "linux")]
use fleqi_platform::linux::context::LinuxContextPort;
#[cfg(target_os = "linux")]
use fleqi_platform::linux::credentials::LinuxCredentials;
#[cfg(target_os = "linux")]
use fleqi_platform::linux::permissions::LinuxPermissions;
#[cfg(target_os = "macos")]
use fleqi_platform::macos::finder::MacContextPort;
#[cfg(target_os = "macos")]
use fleqi_platform::macos::keychain::KeychainCredentials;
#[cfg(target_os = "macos")]
use fleqi_platform::macos::permissions::MacPermissions;
#[cfg(target_os = "windows")]
use fleqi_platform::windows::context::WindowsContextPort;
#[cfg(target_os = "windows")]
use fleqi_platform::windows::credentials::WindowsCredentials;
#[cfg(target_os = "windows")]
use fleqi_platform::windows::permissions::WindowsPermissions;

#[cfg(target_os = "macos")]
type ActivationObserver = fleqi_platform::macos::observer::FinderActivationObserver;
#[cfg(target_os = "linux")]
type ActivationObserver = fleqi_platform::linux::observer::FileManagerActivationObserver;
#[cfg(target_os = "windows")]
type ActivationObserver = fleqi_platform::windows::observer::FileManagerActivationObserver;

#[cfg(target_os = "macos")]
type PlatformPermissions = MacPermissions;
#[cfg(target_os = "linux")]
type PlatformPermissions = LinuxPermissions;
#[cfg(target_os = "windows")]
type PlatformPermissions = WindowsPermissions;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};

#[cfg(feature = "desktop-test")]
pub const CREDENTIAL_NAMESPACE: &str = "app.fleqi.desktop.test";
#[cfg(not(feature = "desktop-test"))]
pub const CREDENTIAL_NAMESPACE: &str = "app.fleqi.desktop";

pub struct TauriEventSink {
    handle: AppHandle,
}

impl EventSink for TauriEventSink {
    fn emit(&self, event: AppEvent) {
        if let Err(error) = self.handle.emit(event.name(), &event) {
            // 事件是窗口重拉快照的信号；发送失败必须可见，不能静默。
            eprintln!("[fleqi] 事件 {} 发送失败：{error}", event.name());
        }
    }
}

pub struct TauriMainThread {
    handle: AppHandle,
}

impl MainThreadExecutor for TauriMainThread {
    fn run(&self, job: Box<dyn FnOnce() + Send>) {
        let _ = self.handle.run_on_main_thread(job);
    }
}

/// 在途工具安装任务：进度单元 + 取消标志（tools_install_status/cancel 消费）。
pub struct InstallJob {
    pub cancel: Arc<std::sync::atomic::AtomicBool>,
    pub progress: Arc<Mutex<fleqi_application::ports::InstallProgress>>,
}

pub struct PlanningJob {
    pub session_id: String,
    pub cancel: Arc<AtomicBool>,
}

pub struct AppState {
    pub paths: Arc<PathRegistry>,
    pub lifecycle: Arc<HostLifecycle>,
    pub settings: SettingsService,
    pub permissions: Arc<PermissionService>,
    pub context: std::sync::Arc<ContextService>,
    pub logger: SafeLogger,
    pub database: Option<Arc<Database>>,
    pub storage: Mutex<StorageStatus>,
    pub credentials: Mutex<CredentialStoreStatus>,
    pub log_dir: PathBuf,
    pub quitting: AtomicBool,
    pub sessions: Arc<SessionService>,
    pub terminal: Arc<TerminalService>,
    pub surface: Arc<SurfaceService>,
    /// Finder 激活观察：Finder 激活（去抖）→ 上下文刷新 → 显示状态机重估。
    pub finder_observer: Arc<ActivationObserver>,
    pub runs: Arc<fleqi_application::run_service::RunService>,
    pub collections: std::sync::Arc<fleqi_application::collection_service::CollectionService>,
    /// 工具设施（M3.3）：探测/受管安装/所有权卸载。
    pub tools: std::sync::Arc<fleqi_application::tool_service::ToolService>,
    /// 模型端点（M3.2）：保存/列表/删除与规划用默认端点。
    pub providers: std::sync::Arc<fleqi_application::provider_service::ProviderService>,
    /// 规划闭环（M3.2）：模型 → 计划 → RunService。
    pub planning: std::sync::Arc<fleqi_application::planning_service::PlanningService>,
    /// Keychain 端口（自检与端点密钥共用；密钥不回前端）。
    pub keychain: Arc<dyn CredentialPort>,
    /// 当前生效的全局快捷键（宿主注册成功后才写入）。
    pub hotkey: Mutex<Option<String>>,
    pub hotkey_down: AtomicBool,
    pub composer_focus_requested: AtomicBool,
    pub composer_attached: AtomicBool,
    pub composer_extra_height: std::sync::atomic::AtomicU32,
    pub _launch_at_login: Mutex<bool>,
    /// 在途工具安装任务表（requestId → 进度/取消）。
    pub install_jobs: Mutex<std::collections::HashMap<String, InstallJob>>,
    /// 在途规划请求取消表（requestId → 标志）。
    pub planning_cancels: Arc<Mutex<std::collections::HashMap<String, PlanningJob>>>,
}

impl AppState {
    /// 按合同顺序装配；存储失败进入降级而非中止。
    pub fn assemble(handle: &AppHandle) -> Result<Arc<Self>, String> {
        let data_dir = resolve_data_dir(handle)?;
        let log_dir = data_dir.join("logs");
        let logger = SafeLogger::open(&log_dir).map_err(|e| format!("无法打开日志目录：{e}"))?;
        logger.log("info", "host.starting", &[("component", "desktop")]);

        let events: Arc<dyn EventSink> = Arc::new(TauriEventSink {
            handle: handle.clone(),
        });
        let clock = Arc::new(SystemClock);
        let lifecycle = Arc::new(HostLifecycle::new(events.clone()));

        let (database, storage_status) = match Database::open(&data_dir) {
            Ok(db) => {
                let status = StorageStatus {
                    state: StorageState::Ready,
                    data_dir_display: data_dir.to_string_lossy().into_owned(),
                    schema_version: db.schema_version(),
                    message: None,
                    last_backup_display: db.last_backup().map(|p| p.to_string_lossy().into_owned()),
                };
                logger.log(
                    "info",
                    "storage.ready",
                    &[("schemaVersion", &db.schema_version().to_string())],
                );
                (Some(Arc::new(db)), status)
            }
            Err(error) => {
                logger.log(
                    "error",
                    "storage.degraded",
                    &[("message", &error.to_string())],
                );
                (
                    None,
                    StorageStatus {
                        state: StorageState::Degraded,
                        data_dir_display: data_dir.to_string_lossy().into_owned(),
                        schema_version: 0,
                        message: Some(error.to_string()),
                        last_backup_display: None,
                    },
                )
            }
        };

        let store: Arc<dyn fleqi_application::ports::SettingsStore> = match &database {
            Some(db) => Arc::new(SqliteSettingsStore::new(db.clone())),
            None => Arc::new(UnavailableStore),
        };
        let store_for_runs = Arc::clone(&store);
        // M3 起全部字段开放（aiPolicy 已由 Run/Planning 真实消费；providers 等密钥
        // 相关配置走 provider_save，不经 settings_update）。
        let (settings, _settings_state, _message) = SettingsService::load(
            store,
            clock.clone(),
            events.clone(),
            lifecycle.clone(),
            FieldAllowlist::All,
        );

        #[cfg(feature = "desktop-test")]
        let mac_permissions = Arc::new(PlatformPermissions::default());
        #[cfg(not(feature = "desktop-test"))]
        let mac_permissions = Arc::new(PlatformPermissions::for_installed_build(&data_dir));
        if let Some(error) = mac_permissions.reset_error() {
            logger.log(
                "warn",
                "permissions.update-reset-failed",
                &[("message", &error)],
            );
        }
        let permissions = PermissionService::new(
            mac_permissions.clone(),
            clock.clone(),
            events.clone(),
            lifecycle.clone(),
            Arc::new(ThreadSpawner),
        );
        let main_thread: Arc<dyn MainThreadExecutor> = Arc::new(TauriMainThread {
            handle: handle.clone(),
        });
        #[cfg(target_os = "macos")]
        let context_port = Arc::new(MacContextPort::new(mac_permissions, main_thread));
        #[cfg(target_os = "linux")]
        let context_port = Arc::new(LinuxContextPort::new(main_thread));
        #[cfg(target_os = "windows")]
        let context_port = Arc::new(WindowsContextPort::new(main_thread));
        let paths = Arc::new(PathRegistry::new());
        let context = Arc::new(ContextService::new(
            context_port,
            clock.clone(),
            events.clone(),
            Arc::clone(&paths),
        ));

        // M2：会话/终端/显示服务；事件路由（上下文→显示、设置→显示）由宿主完成。
        let session_store: Arc<dyn fleqi_application::ports::SessionStore> = match &database {
            Some(db) => Arc::new(fleqi_adapters::storage::SqliteSessionStore::new(db.clone())),
            None => Arc::new(UnavailableSessionStore),
        };
        let ids: Arc<dyn fleqi_application::ports::IdGenerator> =
            Arc::new(fleqi_application::ports::SequenceIds::new());
        let sessions =
            match SessionService::load(session_store, clock.clone(), ids.clone(), events.clone()) {
                Ok(service) => Arc::new(service),
                Err(error) => {
                    logger.log(
                        "error",
                        "sessions.load-failed",
                        &[("message", &error.to_string())],
                    );
                    Arc::new(
                        SessionService::load(
                            Arc::new(UnavailableSessionStore),
                            clock.clone(),
                            ids.clone(),
                            events.clone(),
                        )
                        .map_err(|e| e.to_string())?,
                    )
                }
            };
        let terminal_port = Arc::new(fleqi_adapters::terminal::PtyTerminalPort {
            data_root: data_dir.clone(),
            max_persisted_bytes: fleqi_domain::settings::limits::SESSION_OUTPUT_BYTES,
        });
        let terminal = TerminalService::new(
            terminal_port,
            Arc::clone(&sessions),
            Arc::clone(&paths),
            clock.clone(),
            ids,
            events.clone(),
            Arc::new(ThreadSpawner),
        );
        let collection_ids: Arc<dyn fleqi_application::ports::IdGenerator> =
            Arc::new(fleqi_application::ports::SequenceIds::new());
        let run_ids: Arc<dyn fleqi_application::ports::IdGenerator> =
            Arc::new(fleqi_application::ports::SequenceIds::new());
        // Finder 激活 → 刷新上下文并路由到显示状态机（followFinder 自动显示）。
        // 与手动刷新共用 refresh_context_routed 管线；state 在 manage 后可用。
        let app_for_observer = handle.clone();
        let finder_observer = ActivationObserver::new(Box::new(move |finder_active| {
            if let Some(state) = app_for_observer.try_state::<Arc<AppState>>() {
                if finder_active {
                    let state = Arc::clone(&state);
                    let app = app_for_observer.clone();
                    std::thread::spawn(move || {
                        let snapshot = state.refresh_context_routed(&app);
                        if crate::windows::finder_interaction_active(&app) {
                            state.surface.on_context_changed(&snapshot);
                        }
                    });
                } else if state.composer_attached.load(Ordering::Acquire) {
                    state.surface.system_hide();
                }
            }
        }));
        let surface = SurfaceService::new(
            &settings.snapshot().settings,
            false,
            SurfaceDeps {
                sessions: Arc::clone(&sessions),
                terminal: Arc::clone(&terminal),
                context: Arc::clone(&context),
                paths: Arc::clone(&paths),
                events: events.clone(),
            },
        );

        // M3：Run 编排与规则/收藏/历史。
        let session_store_for_runs: Arc<dyn fleqi_application::ports::SessionStore> =
            match &database {
                Some(db) => Arc::new(fleqi_adapters::storage::SqliteSessionStore::new(db.clone())),
                None => Arc::new(UnavailableSessionStore),
            };
        let run_store: Arc<dyn fleqi_application::ports::RunStore> = match &database {
            Some(db) => Arc::new(fleqi_adapters::storage::SqliteRunStore::new(db.clone())),
            None => Arc::new(UnavailableRunStore),
        };
        let collection_store: Arc<dyn fleqi_application::ports::CollectionStore> = match &database {
            Some(db) => Arc::new(fleqi_adapters::storage::SqliteCollectionStore::new(
                db.clone(),
            )),
            None => Arc::new(UnavailableCollectionStore),
        };
        let process_port: Arc<dyn fleqi_application::ports::ProcessPort> =
            Arc::new(fleqi_adapters::process::ProcessRunnerPort);
        let process_port_for_tools = Arc::clone(&process_port);
        let runs = fleqi_application::run_service::RunService::new(
            run_store,
            session_store_for_runs,
            store_for_runs,
            process_port,
            clock.clone(),
            run_ids,
            events.clone(),
            paths.clone(),
        );
        let native = Arc::new(fleqi_adapters::native_steps::NativeSteps::new(
            paths.clone(),
        ));
        let directory_context = Arc::downgrade(&context);
        let directory_surface = Arc::downgrade(&surface);
        let directory_terminal = Arc::downgrade(&terminal);
        native.set_directory_status(Arc::new(move || {
            let context = directory_context.upgrade().ok_or("宿主正在退出")?;
            let surface = directory_surface.upgrade().ok_or("宿主正在退出")?;
            let terminal = directory_terminal.upgrade().ok_or("宿主正在退出")?;
            let finder = context.latest();
            let session = surface.visible_session();
            let snapshot = session.as_ref().and_then(|s| terminal.snapshot(&s.id).ok());
            Ok(serde_json::json!({ "finderTarget": finder.as_ref().and_then(|c| c.directory_ref.as_ref()).map(|p| &p.display_path), "sessionId": session.as_ref().map(|s| &s.id), "terminalCwd": snapshot.as_ref().map(|s| &s.current_directory), "pendingDirectory": snapshot.as_ref().and_then(|s| s.pending_directory.as_ref()), "directorySync": snapshot.as_ref().map(|s| s.directory_sync), "terminalStarted": snapshot.is_some() }).to_string())
        })).map_err(|error| error.to_string())?;
        runs.set_native_executor(native.clone())
            .map_err(|error| error.message)?;
        if database.is_some() {
            runs.restore().map_err(|error| error.message)?;
        }
        let planning_cancels: Arc<Mutex<std::collections::HashMap<String, PlanningJob>>> =
            Arc::new(Mutex::new(std::collections::HashMap::new()));
        let runs_for_end = runs.clone();
        let plans_for_end = planning_cancels.clone();
        sessions
            .set_end_hook(Arc::new(move |session_id| {
                for job in plans_for_end.lock().expect("planning jobs").values() {
                    if job.session_id == session_id {
                        job.cancel.store(true, Ordering::Release);
                    }
                }
                runs_for_end.cancel_session(session_id)
            }))
            .map_err(|error| error.message)?;
        let collections = std::sync::Arc::new(
            fleqi_application::collection_service::CollectionService::new(
                collection_store,
                clock.clone(),
                collection_ids,
                events.clone(),
            ),
        );
        // M3.3：工具根目录在应用数据目录 tools/ 下；系统工具仅探测登记。
        // 受管目录来源：数据目录 tools/managed-catalog.json（可选；受管条目
        // 的版本/来源/校验和平台映射由该目录确定，AC-COMMON-007）。
        let tool_store: Arc<dyn fleqi_application::ports::ToolStore> = match &database {
            Some(db) => Arc::new(fleqi_adapters::storage::SqliteToolStore::new(db.clone())),
            None => Arc::new(UnavailableToolStore),
        };
        let tool_facility: Arc<dyn fleqi_application::ports::ToolFacility> = Arc::new(
            fleqi_adapters::tools::ToolManager::new(data_dir.join("tools"), process_port_for_tools),
        );
        let mut tool_service = fleqi_application::tool_service::ToolService::new(
            tool_store,
            tool_facility,
            clock.clone(),
            events.clone(),
        );
        let managed_catalog_path = data_dir.join("tools").join("managed-catalog.json");
        match std::fs::read_to_string(&managed_catalog_path) {
            Ok(text) => match serde_json::from_str::<Vec<fleqi_domain::tools::ToolManifest>>(&text)
            {
                Ok(manifests) => {
                    let count = manifests.len();
                    if let Err(error) = tool_service.register_manifests(manifests) {
                        logger.log(
                            "warn",
                            "tools.catalog-rejected",
                            &[("message", &error.message)],
                        );
                    } else if count > 0 {
                        logger.log(
                            "info",
                            "tools.catalog-loaded",
                            &[("count", &count.to_string())],
                        );
                    }
                }
                Err(error) => {
                    let message = error.to_string();
                    logger.log("warn", "tools.catalog-invalid", &[("message", &message)]);
                }
            },
            Err(_) => {
                // 无受管目录文件：只有内建系统探测登记（常见安装形态）。
            }
        }
        let tools = std::sync::Arc::new(tool_service);
        let executable_tools = Arc::downgrade(&tools);
        fleqi_adapters::image_operations::set_tool_resolver(Arc::new(move |name| {
            executable_tools.upgrade().map_or(Ok(None), |tools| {
                tools
                    .resolve_executable(name)
                    .map_err(|error| error.message)
            })
        }))?;
        // M3.2：模型端点 + 规划闭环（模型 → 计划 → RunService；密钥只在 Rust 侧转发）。
        #[cfg(target_os = "macos")]
        let keychain: Arc<dyn CredentialPort> =
            Arc::new(KeychainCredentials::new(CREDENTIAL_NAMESPACE));
        #[cfg(target_os = "linux")]
        let keychain: Arc<dyn CredentialPort> =
            Arc::new(LinuxCredentials::new(CREDENTIAL_NAMESPACE));
        #[cfg(target_os = "windows")]
        let keychain: Arc<dyn CredentialPort> =
            Arc::new(WindowsCredentials::new(CREDENTIAL_NAMESPACE));
        let provider_store: Arc<dyn fleqi_application::ports::ProviderStore> = match &database {
            Some(db) => Arc::new(fleqi_adapters::storage::SqliteProviderStore::new(
                db.clone(),
            )),
            None => Arc::new(UnavailableProviderStore),
        };
        let providers =
            std::sync::Arc::new(fleqi_application::provider_service::ProviderService::new(
                provider_store,
                Arc::clone(&keychain) as Arc<dyn fleqi_application::ports::CredentialPort>,
                clock.clone(),
                events.clone(),
            ));
        let model_gateway: Arc<dyn fleqi_application::ports::ModelGateway> =
            Arc::new(fleqi_adapters::model::PlanningModelGateway);
        let settings_store_for_planning: Arc<dyn fleqi_application::ports::SettingsStore> =
            match &database {
                Some(db) => Arc::new(SqliteSettingsStore::new(db.clone())),
                None => Arc::new(UnavailableStore),
            };
        let planning =
            std::sync::Arc::new(fleqi_application::planning_service::PlanningService::new(
                Arc::clone(&providers),
                model_gateway.clone(),
                settings_store_for_planning.clone(),
                Arc::clone(&runs),
                clock.clone(),
            ));
        native.set_summary(Arc::new(
            fleqi_application::summary_service::SummaryService {
                providers: providers.clone(),
                models: model_gateway,
                settings: settings_store_for_planning,
            },
        ))?;
        Ok(Arc::new(Self {
            paths,
            lifecycle,
            settings,
            permissions,
            context,
            logger,
            database,
            storage: Mutex::new(storage_status),
            credentials: Mutex::new(CredentialStoreStatus {
                available: false,
                namespace: CREDENTIAL_NAMESPACE.into(),
                message: Some("尚未自检".into()),
            }),
            log_dir,
            quitting: AtomicBool::new(false),
            sessions,
            terminal,
            surface,
            finder_observer,
            runs,
            collections,
            tools,
            providers,
            planning,
            keychain,
            hotkey: Mutex::new(None),
            hotkey_down: AtomicBool::new(false),
            composer_focus_requested: AtomicBool::new(false),
            composer_attached: AtomicBool::new(false),
            _launch_at_login: Mutex::new(false),
            composer_extra_height: std::sync::atomic::AtomicU32::new(0),
            install_jobs: Mutex::new(std::collections::HashMap::new()),
            planning_cancels,
        }))
    }

    /// 无提示异步自检：权限被动检测 + Keychain 自有测试项；完成后进入 ready/degraded。
    pub fn run_self_checks(self: &Arc<Self>) {
        let state = Arc::clone(self);
        std::thread::Builder::new()
            .name("fleqi-self-check".into())
            .spawn(move || {
                let snapshot = state.permissions.check_all();
                state.logger.log(
                    "info",
                    "permissions.checked",
                    &[("revision", &snapshot.revision.to_string())],
                );
                let stamp = fleqi_application::ports::Clock::now_rfc3339(&SystemClock);
                let credentials = self_test(state.keychain.as_ref(), &stamp);
                state.logger.log(
                    if credentials.available {
                        "info"
                    } else {
                        "warn"
                    },
                    "credentials.selftest",
                    &[(
                        "status",
                        if credentials.available {
                            "available"
                        } else {
                            "unavailable"
                        },
                    )],
                );
                *state.credentials.lock().expect("credentials 锁") = credentials;
                let target =
                    if state.storage.lock().expect("storage 锁").state == StorageState::Ready {
                        HostState::Ready
                    } else {
                        HostState::Degraded
                    };
                let _ = state.lifecycle.transition(target);
                state
                    .logger
                    .log("info", "host.state", &[("state", &format!("{target:?}"))]);
            })
            .expect("启动自检线程");
    }

    pub fn platform_capabilities(&self) -> fleqi_domain::platform::PlatformCapabilities {
        let permissions = self.permissions.snapshot();
        let credentials = self.credentials.lock().expect("credentials 锁").clone();
        let capabilities = derive_capabilities(
            Revision::new(permissions.revision.value()),
            &permissions,
            &credentials,
        );
        #[cfg(windows)]
        let capabilities = {
            use fleqi_domain::platform::CapabilityState;
            let mut capabilities = capabilities;
            let explorer = self.context.latest().is_some_and(|context| {
                context.source == fleqi_domain::context::ContextSource::Explorer
                    && matches!(
                        context.availability,
                        fleqi_domain::context::ContextAvailability::Available
                    )
            });
            let geometry = fleqi_platform::host::file_manager_frame().has_window;
            for item in &mut capabilities.items {
                let available = match item.id.as_str() {
                    "finderContext" => explorer,
                    "accessibilityGeometry" => geometry,
                    _ => continue,
                };
                item.state = if available {
                    CapabilityState::Supported
                } else {
                    CapabilityState::TemporarilyUnavailable
                };
                item.reason = (!available).then(|| "当前没有可用的 Explorer 文件夹窗口".into());
                item.recovery =
                    (!available).then(|| "打开资源管理器中的真实文件夹，或手动选择目录".into());
            }
            capabilities
        };
        capabilities
    }

    /// 刷新 Finder 上下文并路由到显示状态机：目录同步与 followFinder 自动显示
    /// 都走这一条管线（观察器与手动刷新命令共用，行为一致）。
    pub fn refresh_context_routed(
        self: &Arc<Self>,
        app: &AppHandle,
    ) -> fleqi_domain::context::ContextSnapshot {
        let snapshot = self.context.refresh();
        let _ = app;
        snapshot
    }

    pub fn bootstrap(&self, window_role: &str) -> AppBootstrap {
        AppBootstrap {
            build_info: BuildInfo::current(),
            host_state: self.lifecycle.state(),
            window_role: window_role.to_owned(),
            storage: self.storage.lock().expect("storage 锁").clone(),
            settings: self.settings.snapshot(),
            permissions: self.permissions.snapshot(),
            platform: self.platform_capabilities(),
            context: self.context.latest(),
        }
    }

    pub fn diagnostics(&self) -> DiagnosticsSnapshot {
        DiagnosticsSnapshot {
            build_info: BuildInfo::current(),
            host_state: self.lifecycle.state(),
            generation: self.lifecycle.generation().value().to_string(),
            storage: self.storage.lock().expect("storage 锁").clone(),
            log_dir_display: self.log_dir.to_string_lossy().into_owned(),
            applied_migrations: self
                .database
                .as_ref()
                .map(|db| db.applied_migrations().to_vec())
                .unwrap_or_default(),
            permissions: self.permissions.snapshot(),
            platform: self.platform_capabilities(),
            credential_store: self.credentials.lock().expect("credentials 锁").clone(),
        }
    }

    /// 退出：进入 stopping（拒绝新变更、代际递增），排空数据库写入。
    pub fn begin_shutdown(&self) {
        if self.quitting.swap(true, Ordering::SeqCst) {
            return;
        }
        let _ = self.lifecycle.transition(HostState::Stopping);
        self.tools.cancel_preparation();
        for job in self.install_jobs.lock().expect("install jobs").values() {
            job.cancel.store(true, Ordering::Release);
        }
        for job in self
            .planning_cancels
            .lock()
            .expect("planning jobs")
            .values()
        {
            job.cancel.store(true, Ordering::Release);
        }
        for session_id in self.sessions.active_ids() {
            let _ = self.runs.cancel_session(&session_id);
        }
        self.terminal.end_all();
        self.runs.wait_idle(std::time::Duration::from_secs(3));
        if let Some(db) = &self.database {
            let _ = db.drain();
        }
        self.logger.log("info", "host.stopping", &[]);
    }
}

fn resolve_data_dir(handle: &AppHandle) -> Result<PathBuf, String> {
    #[cfg(feature = "desktop-test")]
    if let Ok(dir) = std::env::var("FLEQI_TEST_DATA_DIR")
        && !dir.is_empty()
    {
        return Ok(PathBuf::from(dir));
    }
    handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法解析应用数据目录：{e}"))
}

/// 存储降级时的占位仓库：load 报告不可用，SettingsService 据此进入未持久化默认值。
struct UnavailableStore;

impl fleqi_application::ports::SettingsStore for UnavailableStore {
    fn load(
        &self,
    ) -> Result<
        Option<fleqi_application::ports::PersistedSettings>,
        fleqi_application::ports::StorageError,
    > {
        Err(fleqi_application::ports::StorageError::Unavailable(
            "数据库未打开".into(),
        ))
    }

    fn commit(
        &self,
        _settings: &fleqi_domain::settings::Settings,
        _expected: Option<Revision>,
        _receipt: &fleqi_domain::idempotency::Receipt,
    ) -> Result<Revision, fleqi_application::ports::StorageError> {
        Err(fleqi_application::ports::StorageError::Unavailable(
            "数据库未打开".into(),
        ))
    }

    fn find_receipt(
        &self,
        _request_id: &str,
    ) -> Result<Option<fleqi_domain::idempotency::Receipt>, fleqi_application::ports::StorageError>
    {
        Err(fleqi_application::ports::StorageError::Unavailable(
            "数据库未打开".into(),
        ))
    }
}

/// 存储降级时的会话仓库占位：不可用，只保留空注册表。
struct UnavailableSessionStore;

impl SessionStore for UnavailableSessionStore {
    fn load_all(&self) -> Result<Vec<fleqi_domain::session::Session>, StorageError> {
        Err(StorageError::Unavailable("数据库未打开".into()))
    }
    fn upsert(&self, _session: &fleqi_domain::session::Session) -> Result<(), StorageError> {
        Err(StorageError::Unavailable("数据库未打开".into()))
    }
    fn delete(&self, _session_id: &str) -> Result<(), StorageError> {
        Err(StorageError::Unavailable("数据库未打开".into()))
    }
    fn append_entry(
        &self,
        _entry: &fleqi_domain::session::ConversationEntry,
    ) -> Result<(), StorageError> {
        Err(StorageError::Unavailable("数据库未打开".into()))
    }
    fn entries(
        &self,
        _session_id: &str,
        _limit: usize,
        _before: Option<&str>,
    ) -> Result<Vec<fleqi_domain::session::ConversationEntry>, StorageError> {
        Err(StorageError::Unavailable("数据库未打开".into()))
    }
}

/// 存储降级时的 Run 仓库占位。
struct UnavailableRunStore;

impl fleqi_application::ports::RunStore for UnavailableRunStore {
    fn load_all(&self) -> Result<Vec<fleqi_application::run_service::RunRecord>, StorageError> {
        Err(StorageError::Io("存储不可用".into()))
    }
    fn upsert(&self, _run: &fleqi_application::run_service::RunRecord) -> Result<(), StorageError> {
        Err(StorageError::Unavailable("数据库未打开".into()))
    }
    fn get(
        &self,
        _run_id: &str,
    ) -> Result<Option<fleqi_application::run_service::RunRecord>, StorageError> {
        Err(StorageError::Unavailable("数据库未打开".into()))
    }
    fn list(
        &self,
        _session_id: &str,
    ) -> Result<Vec<fleqi_application::run_service::RunRecord>, StorageError> {
        Err(StorageError::Unavailable("数据库未打开".into()))
    }
    fn append_output(&self, _run_id: &str, _bytes: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::Unavailable("数据库未打开".into()))
    }
}

/// 存储降级时的集合仓库占位。
struct UnavailableCollectionStore;

impl fleqi_application::ports::CollectionStore for UnavailableCollectionStore {
    fn load_rules(&self) -> Result<Vec<fleqi_application::collection_service::Rule>, StorageError> {
        Err(StorageError::Unavailable("数据库未打开".into()))
    }
    fn upsert_rule(
        &self,
        _rule: &fleqi_application::collection_service::Rule,
    ) -> Result<(), StorageError> {
        Err(StorageError::Unavailable("数据库未打开".into()))
    }
    fn delete_rule(&self, _rule_id: &str) -> Result<(), StorageError> {
        Err(StorageError::Unavailable("数据库未打开".into()))
    }
    fn load_favorites(
        &self,
    ) -> Result<Vec<fleqi_application::collection_service::Favorite>, StorageError> {
        Err(StorageError::Unavailable("数据库未打开".into()))
    }
    fn upsert_favorite(
        &self,
        _favorite: &fleqi_application::collection_service::Favorite,
    ) -> Result<(), StorageError> {
        Err(StorageError::Unavailable("数据库未打开".into()))
    }
    fn delete_favorite(&self, _favorite_id: &str) -> Result<(), StorageError> {
        Err(StorageError::Unavailable("数据库未打开".into()))
    }
    fn history_append(&self, _entry: &str) -> Result<(), StorageError> {
        Err(StorageError::Unavailable("数据库未打开".into()))
    }
    fn history_list(&self) -> Result<Vec<String>, StorageError> {
        Err(StorageError::Unavailable("数据库未打开".into()))
    }
    fn history_clear(&self) -> Result<(), StorageError> {
        Err(StorageError::Unavailable("数据库未打开".into()))
    }
}

/// 存储降级时的工具仓库占位。
struct UnavailableToolStore;

impl fleqi_application::ports::ToolStore for UnavailableToolStore {
    fn upsert(&self, _tool: &fleqi_domain::tools::InstalledTool) -> Result<(), StorageError> {
        Err(StorageError::Unavailable("数据库未打开".into()))
    }
    fn load_all(&self) -> Result<Vec<fleqi_domain::tools::InstalledTool>, StorageError> {
        Err(StorageError::Unavailable("数据库未打开".into()))
    }
    fn delete(&self, _tool_id: &str) -> Result<(), StorageError> {
        Err(StorageError::Unavailable("数据库未打开".into()))
    }
}

/// 存储降级时的端点仓库占位。
struct UnavailableProviderStore;

impl fleqi_application::ports::ProviderStore for UnavailableProviderStore {
    fn upsert(
        &self,
        _provider: &fleqi_application::provider_service::ProviderRecord,
    ) -> Result<(), StorageError> {
        Err(StorageError::Unavailable("数据库未打开".into()))
    }
    fn load_all(
        &self,
    ) -> Result<Vec<fleqi_application::provider_service::ProviderRecord>, StorageError> {
        Err(StorageError::Unavailable("数据库未打开".into()))
    }
    fn delete(&self, _provider_id: &str) -> Result<(), StorageError> {
        Err(StorageError::Unavailable("数据库未打开".into()))
    }
}
