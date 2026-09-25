//! macOS 平台适配测试：权限映射与真实无提示探测、Finder 读取的结构化转换、脚本固定性。
#![cfg(target_os = "macos")]

use fleqi_application::ports::{PermissionPort, RawContext};
use fleqi_domain::context::{ContextAvailability, PathKind, SELECTION_LIMIT, ViewKind};
use fleqi_domain::permissions::{Permission, PermissionProcedure, PermissionStatus};
use fleqi_platform::macos::finder::{FinderReading, reading_to_raw, snapshot_script_is_constant};
use fleqi_platform::macos::permissions::{
    MacPermissions, map_automation_status, system_settings_url,
};

#[test]
fn automation_status_mapping_covers_documented_codes() {
    assert_eq!(map_automation_status(0).0, PermissionStatus::Allowed);
    assert_eq!(
        map_automation_status(-1744).0,
        PermissionStatus::NeedsConsent
    );
    assert_eq!(map_automation_status(-1743).0, PermissionStatus::Denied);
    assert_eq!(
        map_automation_status(-600).0,
        PermissionStatus::TargetNotRunning
    );
    let (status, error) = map_automation_status(-1);
    assert_eq!(status, PermissionStatus::Failed);
    assert!(error.unwrap().contains("-1"));
}

#[test]
fn passive_probes_never_prompt_and_return_a_definite_status() {
    let port = MacPermissions::default();
    for permission in Permission::ALL {
        let probe = port.check(permission, PermissionProcedure::Passive);
        assert_ne!(
            probe.status,
            PermissionStatus::Unknown,
            "{permission:?}: {probe:?}"
        );
        if probe.status == PermissionStatus::Failed {
            assert!(probe.error.is_some());
        }
    }
    assert!(system_settings_url(Permission::FinderAutomation).contains("Privacy_Automation"));
    assert!(system_settings_url(Permission::Accessibility).contains("Privacy_Accessibility"));
}

fn reading(kind: &str, dir: &str, count: usize, selection: &[&str]) -> FinderReading {
    FinderReading {
        window_id: if kind == "desktop" { 0 } else { 42 },
        kind: kind.into(),
        directory: dir.into(),
        selection_count: count,
        selection: selection.iter().map(|s| s.to_string()).collect(),
    }
}

#[test]
fn physical_window_maps_to_directory_and_selection() {
    let raw = reading_to_raw(reading(
        "physical",
        "/Users/me/含 空格/目录/",
        2,
        &[
            "/Users/me/含 空格/目录/a b.txt",
            "/Users/me/含 空格/目录/sub/",
        ],
    ));
    assert_eq!(raw.source_window_id, Some(42));
    assert_eq!(raw.view_kind, Some(ViewKind::Physical));
    assert_eq!(raw.directory.as_ref().unwrap().kind, PathKind::Directory);
    assert_eq!(raw.selection.len(), 2);
    assert_eq!(raw.selection[0].kind, PathKind::File);
    assert_eq!(raw.selection[1].kind, PathKind::Directory);
    assert!(raw.unavailable.is_none());
}

#[test]
fn virtual_window_has_no_directory_but_keeps_real_selection() {
    let raw = reading_to_raw(reading("virtual", "", 1, &["/Users/me/x.pdf"]));
    assert_eq!(raw.view_kind, Some(ViewKind::Virtual));
    assert!(raw.directory.is_none(), "虚拟视图不猜 cwd");
    assert_eq!(raw.selection.len(), 1);
}

#[test]
fn desktop_without_windows_uses_desktop_folder() {
    let raw = reading_to_raw(reading("desktop", "/Users/me/Desktop/", 0, &[]));
    assert_eq!(raw.view_kind, Some(ViewKind::Desktop));
    assert_eq!(raw.source_window_id, None);
    assert!(raw.directory.is_some());
}

#[test]
fn selection_over_limit_is_reported_not_truncated() {
    let raw = reading_to_raw(reading("physical", "/tmp/", SELECTION_LIMIT + 5, &[]));
    assert!(raw.selection.is_empty());
    assert_eq!(
        raw.unavailable,
        Some(ContextAvailability::SelectionOverLimit {
            count: SELECTION_LIMIT + 5,
            limit: SELECTION_LIMIT
        })
    );
    let default = RawContext::default();
    assert!(default.unavailable.is_none());
}

#[test]
fn snapshot_script_is_fixed_and_parameter_free() {
    assert!(snapshot_script_is_constant());
}

#[test]
fn incomplete_finder_selection_is_rejected_instead_of_processing_a_different_subset() {
    let raw = reading_to_raw(FinderReading {
        window_id: 11,
        kind: "physical".into(),
        directory: "/tmp".into(),
        selection_count: 2,
        selection: vec!["/tmp/another-photo.png".into()],
    });
    assert!(raw.selection.is_empty());
    assert!(matches!(
        raw.unavailable,
        Some(ContextAvailability::Failed { .. })
    ));
}
