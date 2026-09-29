//! Windows 适配在非 Windows 主机上的可执行检查：纯函数、不可用路径，以及不启动系统进程。

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use fleqi_application::ports::{
    ContextPort, CredentialError, CredentialPort, DirectoryPick, PermissionPort,
};
use fleqi_domain::context::{ContextAvailability, PathKind};
use fleqi_domain::permissions::{Permission, PermissionProcedure, PermissionStatus};
use fleqi_platform::credentials::self_test;
use fleqi_platform::scheduling::MainThreadExecutor;
use fleqi_platform::windows::context::{
    WindowsContextPort, context_from_explorer_output, location_to_path,
};
use fleqi_platform::windows::credentials::{
    CREDENTIAL_SCRIPT, WIN32_ERROR_ALREADY_EXISTS, WIN32_ERROR_NOT_FOUND, WindowsCredentials,
    credential_target, map_credential_code,
};
use fleqi_platform::windows::observer::FileManagerActivationObserver;
use fleqi_platform::windows::permissions::WindowsPermissions;
use fleqi_platform::windows::picker::{FOLDER_DIALOG_SCRIPT, classify_picker_output};
use fleqi_platform::windows::surface::{file_manager_frame, material_kind};
use fleqi_platform::windows::update_install::install_verified;

fn default_value<T: Default>() -> T {
    T::default()
}

struct RejectMain;

impl MainThreadExecutor for RejectMain {
    fn run(&self, _job: Box<dyn FnOnce() + Send>) {
        panic!("非 Windows 不应把对话框投递到主线程");
    }
}

#[test]
fn credential_target_accepts_pairs_and_rejects_controls() {
    assert_eq!(
        credential_target("app.fleqi.desktop", "provider").unwrap(),
        "fleqi:app.fleqi.desktop:provider"
    );
    assert_eq!(credential_target("应用", "键").unwrap(), "fleqi:应用:键");
    assert!(credential_target("", "key").is_err());
    assert!(credential_target("app", "").is_err());
    assert!(credential_target("app\0", "key").is_err());
    assert!(credential_target("app\r", "key").is_err());
    assert!(credential_target("app\n", "key").is_err());
    assert!(credential_target("app", "ke\u{0001}y").is_err());
    assert!(credential_target("app\t", "key").is_err());
}

#[test]
fn credential_port_is_unavailable_off_windows_and_self_test_reports_it() {
    let port = WindowsCredentials::new("app.fleqi.desktop");
    assert_eq!(port.namespace(), "app.fleqi.desktop");
    for result in [
        port.store("provider", b"secret"),
        port.replace("provider", b"secret"),
        port.delete("provider"),
    ] {
        match result {
            Err(CredentialError::Unavailable(message)) => {
                assert!(message.contains("Windows"));
                assert!(message.contains("凭据"));
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }
    assert!(matches!(
        port.exists("provider"),
        Err(CredentialError::Unavailable(_))
    ));
    assert!(matches!(
        port.load("provider"),
        Err(CredentialError::Unavailable(_))
    ));
    let status = self_test(&port, "2026-09-29T00:00:00Z");
    assert!(!status.available);
    assert_eq!(status.namespace, "app.fleqi.desktop");
    assert!(status.message.unwrap().contains("Windows"));
}

#[test]
fn credential_script_calls_win32_and_does_not_interpolate_secrets() {
    assert!(CREDENTIAL_SCRIPT.contains("CredWrite"));
    assert!(CREDENTIAL_SCRIPT.contains("CredRead"));
    assert!(CREDENTIAL_SCRIPT.contains("CredDelete"));
    assert!(CREDENTIAL_SCRIPT.contains("$env:FLEQI_CRED_SECRET_B64"));
    assert!(CREDENTIAL_SCRIPT.contains("$env:FLEQI_CRED_KEY"));
    assert!(CREDENTIAL_SCRIPT.contains("$env:FLEQI_CRED_NAMESPACE"));
    assert!(!CREDENTIAL_SCRIPT.contains("{secret}"));
    assert!(!CREDENTIAL_SCRIPT.contains("{key}"));
    assert!(!CREDENTIAL_SCRIPT.contains("{namespace}"));
    assert!(!CREDENTIAL_SCRIPT.to_ascii_lowercase().contains("cmdkey"));
    assert!(map_credential_code(0).is_ok());
    assert_eq!(
        map_credential_code(WIN32_ERROR_NOT_FOUND),
        Err(CredentialError::NotFound)
    );
    match map_credential_code(WIN32_ERROR_ALREADY_EXISTS) {
        Err(CredentialError::Failed(message)) => assert!(message.contains("替换")),
        other => panic!("expected already-exists failure, got {other:?}"),
    }
    assert!(matches!(
        map_credential_code(5),
        Err(CredentialError::Failed(_))
    ));
}

#[test]
fn location_to_path_decodes_file_urls_and_rejects_unsafe_inputs() {
    let path = location_to_path("file:///C:/dir/My%20Folder").unwrap();
    assert_eq!(path.to_string_lossy(), "C:\\dir\\My Folder");
    let localhost = location_to_path("file://localhost/C:/dir/My%20Folder").unwrap();
    assert_eq!(localhost.to_string_lossy(), "C:\\dir\\My Folder");
    assert!(location_to_path("https://example.com/C:/dir").is_none());
    assert!(location_to_path("http://example.com").is_none());
    assert!(location_to_path("HTTP://example.com").is_none());
    assert!(location_to_path("").is_none());
    assert!(location_to_path("   ").is_none());
    assert!(location_to_path("file:///C:/dir/../secret").is_none());
    assert!(location_to_path("file:///C:/dir/%2e%2e/secret").is_none());
    assert!(location_to_path("file:///C:/dir/%2E%2E/secret").is_none());

    let found = context_from_explorer_output(
        "https://example.com\nfile:///C:/dir/My%20Folder\nfile:///D:/other\n",
    );
    assert_eq!(found.source_window_id, None);
    assert!(found.selection.is_empty());
    assert_eq!(
        found.view_kind,
        Some(fleqi_domain::context::ViewKind::Physical)
    );
    assert_eq!(
        found.directory.unwrap().native.to_string_lossy(),
        "C:\\dir\\My Folder"
    );
    match context_from_explorer_output("https://example.com\n").unavailable {
        Some(ContextAvailability::NoDirectory { reason }) => {
            assert_eq!(reason, "资源管理器没有打开文件夹");
        }
        other => panic!("expected NoDirectory, got {other:?}"),
    }
}

#[test]
fn permissions_fail_and_settings_are_not_spawned_off_windows() {
    let installed = WindowsPermissions::for_installed_build(std::path::Path::new("."));
    assert!(installed.reset_error().is_none());
    let permissions: WindowsPermissions = default_value();
    for permission in [Permission::FinderAutomation, Permission::Accessibility] {
        for procedure in [PermissionProcedure::Passive, PermissionProcedure::Explicit] {
            let probe = permissions.check(permission, procedure);
            assert_eq!(probe.status, PermissionStatus::Failed);
            assert_ne!(probe.status, PermissionStatus::Allowed);
            let message = probe.error.expect("failure reason");
            assert!(!message.is_empty());
        }
    }
    let finder = permissions
        .check(Permission::FinderAutomation, PermissionProcedure::Passive)
        .error
        .unwrap();
    assert!(finder.contains("Finder"));
    assert!(finder.contains("资源管理器"));
    let accessibility = permissions
        .check(Permission::Accessibility, PermissionProcedure::Explicit)
        .error
        .unwrap();
    assert!(accessibility.contains("UI Automation"));
    assert!(accessibility.contains("辅助功能"));
    let message = permissions
        .open_system_settings(Permission::Accessibility)
        .expect_err("settings stay manual off Windows");
    assert!(message.contains("设置 → 隐私和安全性"));
    assert!(!message.contains("explorer.exe"));
    assert!(!message.contains("os error"));
}

#[test]
fn surface_stays_solid_without_a_file_manager_window() {
    assert_eq!(material_kind(true), "solid");
    assert_eq!(material_kind(false), "solid");
    let frame = file_manager_frame();
    assert!(!frame.has_window);
    assert_eq!(frame.width, 0.0);
    assert_eq!(frame.height, 0.0);
}

#[test]
fn picker_classifies_cancel_separately_from_a_non_directory() {
    assert!(matches!(
        classify_picker_output(true, "C:\\Windows", true),
        DirectoryPick::Cancelled
    ));
    assert!(matches!(
        classify_picker_output(false, " \n", true),
        DirectoryPick::Cancelled
    ));
    assert!(matches!(
        classify_picker_output(false, "C:\\missing", false),
        DirectoryPick::Failed(_)
    ));
    match classify_picker_output(false, " D:\\work \n", true) {
        DirectoryPick::Selected(path) => {
            assert_eq!(path.kind, PathKind::Directory);
            assert_eq!(path.native.to_string_lossy(), "D:\\work");
        }
        other => panic!("expected Selected, got {other:?}"),
    }
    assert!(FOLDER_DIALOG_SCRIPT.contains("FolderBrowserDialog"));
    assert!(!FOLDER_DIALOG_SCRIPT.contains("{path}"));

    let port = WindowsContextPort::new(Arc::new(RejectMain));
    match port.capture().unavailable {
        Some(ContextAvailability::NoDirectory { reason }) => {
            assert!(reason.contains("Windows"));
            assert!(reason.contains("资源管理器"));
        }
        other => panic!("expected NoDirectory, got {other:?}"),
    }
    match port.pick_directory() {
        DirectoryPick::Failed(message) => assert!(message.contains("Windows")),
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[test]
fn observer_debounces_activation_and_install_does_not_refresh() {
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&hits);
    let observer = FileManagerActivationObserver::new(Box::new(move |active| {
        assert!(active);
        counter.fetch_add(1, Ordering::SeqCst);
    }));
    assert!(observer.install().is_ok());
    assert!(observer.install().is_ok());
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    observer.note_activation(true);
    observer.note_activation(true);
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    std::thread::sleep(Duration::from_millis(350));
    observer.note_activation(true);
    assert_eq!(hits.load(Ordering::SeqCst), 2);
}

#[test]
fn install_verified_rejects_an_empty_payload() {
    let error = install_verified(b"", "1.2.3").unwrap_err();
    assert!(error.contains("1.2.3"));
    assert!(error.contains("空"));
    assert!(error.contains("保持不变"));
    let other = install_verified(b"not-a-package", "2.0.0").unwrap_err();
    assert!(other.contains("2.0.0"));
    assert!(other.contains("MSIX"));
    assert!(other.contains("保持不变"));
    assert!(install_verified(&[0x1f, 0x8b], "3.0.0").is_err());
}
