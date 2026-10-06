//! Fleqi 桌面宿主入口（M1：宿主与权限底座）。
//!
//! 顺序（architecture.md §12.2）：单实例保护 → 路径与脱敏日志 → 数据库/迁移/设置 →
//! 用例与原生适配 → IPC/菜单/按需窗口 → 无提示异步自检。首次启动不显示产品输入条，
//! 不启动 shell/模型，不自动弹授权窗；关闭窗口保留宿主，退出经 app_quit 或菜单。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod catalog;
mod commands;
mod commands_m3;
mod state;
mod updates;
mod windows;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use tauri::Listener;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, RunEvent};

use state::AppState;
use windows::{WindowRole, open_window};

fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let open_composer = MenuItem::with_id(app, "open_composer", "显示输入条", true, None::<&str>)?;
    let open_console = MenuItem::with_id(app, "open_console", "打开控制台", true, None::<&str>)?;
    let open_settings = MenuItem::with_id(app, "open_settings", "设置…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 Fleqi", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &open_composer,
            &open_console,
            &open_settings,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| tauri::Error::AssetNotFound("default window icon".into()))?;
    TrayIconBuilder::with_id("fleqi-tray")
        .icon(icon)
        .icon_as_template(false)
        .tooltip("Fleqi")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open_composer" => {
                if updates::is_installing(app) {
                    return;
                }
                // 显式显示：走显示状态机（manual 模式需要已注册快捷键，菜单入口据此引导）。
                if let Some(state) = app.try_state::<Arc<AppState>>() {
                    state
                        .composer_focus_requested
                        .store(true, Ordering::Release);
                    let refusal = state.surface.user_show().err();
                    if let Some(error) = &refusal {
                        state
                            .composer_focus_requested
                            .store(false, Ordering::Release);
                        state.logger.log(
                            "warn",
                            "surface.show-refused",
                            &[("message", &error.message)],
                        );
                    }
                    if refusal.is_none() {
                        // 显式入口抢焦点（用户动作）。
                        let _ = windows::show_composer_attached(app, true);
                    } else {
                        let _ = open_window(app, WindowRole::Settings, None);
                    }
                }
            }
            "open_console" => {
                let _ = open_window(app, WindowRole::Console, None);
            }
            "open_settings" => {
                let _ = open_window(app, WindowRole::Settings, None);
            }
            "quit" => {
                if let Some(state) = app.try_state::<Arc<AppState>>() {
                    state.begin_shutdown();
                }
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;
    Ok(())
}

fn main() {
    let builder = tauri::Builder::default()
        // 单实例先于一切初始化：第二个实例只把已有控制台带到前台。
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            let _ = open_window(app, WindowRole::Console, None);
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(updates::Updates::default())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .invoke_handler(tauri::generate_handler![
            commands::app_build_info,
            updates::app_update_status,
            updates::app_update_check,
            updates::app_update_install,
            commands::app_bootstrap,
            commands::diagnostics_get,
            commands::app_open_window,
            commands::app_quit,
            commands::settings_update,
            commands::permissions_get,
            commands::permissions_check,
            commands::permissions_request,
            commands::permissions_open_settings,
            commands::context_get,
            commands::context_refresh,
            commands::context_pick_directory,
            commands::surface_show,
            commands::surface_hide,
            commands::surface_get,
            commands::surface_layout,
            commands::session_create,
            commands::session_list,
            commands::session_entries,
            commands::session_select,
            commands::session_end,
            commands::session_end_all,
            commands::session_delete,
            commands::session_pin,
            commands::session_continue,
            commands::terminal_open,
            commands::terminal_snapshot,
            commands::terminal_input,
            commands::terminal_acquire_lease,
            commands::terminal_resize,
            commands::terminal_ack,
            commands::terminal_submit_line,
            commands::terminal_cancel_queued,
            commands::terminal_withdrawn_line,
            commands::terminal_subscribe,
            commands::terminal_unsubscribe,
            commands::terminal_release_lease,
            commands::hotkey_commit,
            commands::hotkey_clear,
            commands::hotkey_get,
            commands_m3::run_submit,
            commands_m3::run_approve,
            commands_m3::run_cancel,
            commands_m3::run_get,
            commands_m3::run_plan_get,
            commands_m3::run_list,
            commands_m3::run_retry,
            commands_m3::rules_create,
            commands_m3::rules_list,
            commands_m3::rules_update,
            commands_m3::rules_delete,
            commands_m3::favorites_create,
            commands_m3::favorites_list,
            commands_m3::favorites_update,
            commands_m3::favorites_delete,
            commands_m3::history_append,
            commands_m3::history_list,
            commands_m3::history_clear,
            commands_m3::provider_probe,
            commands_m3::catalog_query,
            commands_m3::capability_form,
            commands_m3::capability_submit,
            commands_m3::tools_list,
            commands_m3::tools_prepare,
            commands_m3::tools_prepare_status,
            commands_m3::tools_prepare_cancel,
            commands_m3::tools_install,
            commands_m3::tools_install_status,
            commands_m3::tools_install_cancel,
            commands_m3::tools_remove,
            commands_m3::provider_save,
            commands_m3::provider_list,
            commands_m3::provider_delete,
            commands_m3::run_plan_submit,
            commands_m3::run_plan_cancel,
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Regular);
            let state = AppState::assemble(app.handle())?;
            app.manage(Arc::clone(&state));
            #[cfg(all(not(feature = "desktop-test"), not(windows)))]
            {
                let update_app = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let _ = updates::check(&update_app).await;
                });
            }
            #[cfg(all(not(feature = "desktop-test"), not(windows)))]
            state.tools.prepare();
            let app_for_context_event = app.handle().clone();
            app.listen("context:changed", move |_| {
                if let Some(state) = app_for_context_event.try_state::<Arc<AppState>>()
                    && let Some(snapshot) = state.context.latest()
                    && windows::finder_interaction_active(&app_for_context_event)
                    && fleqi_platform::host::file_manager_frame().has_window
                {
                    state.surface.on_context_changed(&snapshot);
                }
            });
            commands::restore_hotkey(app.handle(), &state);
            build_tray(app)?;
            // M2：Finder 激活观察（followFinder 自动显示 / 上下文随 Finder 更新）。
            // 安装失败降级为手动刷新，不中断启动。
            if let Err(error) = state.finder_observer.install() {
                eprintln!("[fleqi] Finder 观察安装失败（降级为手动刷新）：{error}");
            }
            // M4：surface 状态 → 输入条窗口表现同步（隐藏/结束时收起窗口；
            // 暂隐恢复或显式显示后重新贴附；已在显示时不重复跑 AppleScript）。
            let app_handle_for_surface = app.handle().clone();
            app.listen("surface:changed", move |_event| {
                let app = app_handle_for_surface.clone();
                if let Some(state) = app.try_state::<Arc<AppState>>() {
                    let visible =
                        state.surface.visibility() == fleqi_domain::surface::Visibility::Visible;
                    if visible {
                        let _ = windows::show_composer_attached(
                            &app,
                            state.composer_focus_requested.load(Ordering::Acquire),
                        );
                    } else {
                        windows::hide_composer(&app);
                    }
                }
            });
            // M2 收口：Finder 前窗几何观察（拖动中暂隐、稳定后恢复），400ms 节奏。
            let app_for_geometry = app.handle().clone();
            std::thread::Builder::new()
                .name("fleqi-finder-geometry".into())
                .spawn(move || {
                    let mut watch = windows::FinderGeometryWatch::default();
                    loop {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                        let Some(state) = app_for_geometry.try_state::<Arc<AppState>>() else {
                            continue;
                        };
                        if state.quitting.load(Ordering::Acquire) {
                            break;
                        }
                        watch.tick(&app_for_geometry, &state);
                    }
                })
                .map_err(|e| format!("启动 Finder 几何观察线程失败：{e}"))?;
            let app_for_context = app.handle().clone();
            std::thread::Builder::new()
                .name("fleqi-finder-selection".into())
                .spawn(move || {
                    loop {
                        std::thread::sleep(std::time::Duration::from_millis(350));
                        let Some(state) = app_for_context.try_state::<Arc<AppState>>() else {
                            continue;
                        };
                        if state.quitting.load(Ordering::Acquire) {
                            break;
                        }
                        if fleqi_platform::host::file_manager_frame().foreground == 1 {
                            state.refresh_context_routed(&app_for_context);
                        }
                    }
                })
                .map_err(|e| format!("启动 Finder 选区观察失败：{e}"))?;
            // manual 模式不自动创建输入条窗口（FR-ENTRY-003）。控制台仍作为管理入口。
            open_window(app.handle(), WindowRole::Console, None)?;
            state.run_self_checks();
            Ok(())
        });

    #[cfg(feature = "desktop-test")]
    let builder = builder.plugin(tauri_plugin_wdio_webdriver::init());

    let app = builder
        .build(tauri::generate_context!())
        .expect("构建 Tauri 应用失败");

    app.run(|app_handle, event| {
        #[cfg(target_os = "macos")]
        if matches!(&event, RunEvent::Reopen { .. }) {
            let _ = open_window(app_handle, WindowRole::Console, None);
        }
        if let RunEvent::ExitRequested { code, api, .. } = &event {
            // 最后一个窗口关闭（code=None）不退出宿主；显式退出（app.exit）才结束。
            let quitting = app_handle
                .try_state::<Arc<AppState>>()
                .map(|s| s.quitting.load(Ordering::SeqCst))
                .unwrap_or(false);
            if code.is_none() && !quitting {
                api.prevent_exit();
            }
        }
    });
}
