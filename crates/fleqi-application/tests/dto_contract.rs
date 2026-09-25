//! P0-CONTRACT-001：跨 IPC DTO 契约测试。
//! 验证字段完整性、serde camelCase、稳定常量与版本元数据一致性。

use fleqi_application::dto::{
    AppError, BUNDLE_IDENTIFIER, BuildInfo, ErrorCode, MINIMUM_MACOS_VERSION, PRODUCT_NAME, STAGE,
};

#[test]
fn build_info_carries_full_contract_fields() {
    let info = BuildInfo::current();
    assert_eq!(info.product_name, PRODUCT_NAME);
    assert_eq!(info.product_name, "Fleqi");
    assert_eq!(info.bundle_identifier, BUNDLE_IDENTIFIER);
    assert_eq!(info.bundle_identifier, "app.fleqi.desktop");
    assert_eq!(info.stage, STAGE);
    assert_eq!(info.minimum_macos_version, MINIMUM_MACOS_VERSION);
    assert_eq!(info.minimum_macos_version, "14.0");
    assert_eq!(info.target_os, std::env::consts::OS);
    assert_eq!(info.target_arch, std::env::consts::ARCH);
    assert!(matches!(info.build_profile.as_str(), "debug" | "release"));
}

#[test]
fn version_is_semver_and_matches_workspace() {
    let info = BuildInfo::current();
    // 与 workspace.package.version 一致（Cargo 元数据注入）。
    assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
    // Numeric app version also works in macOS bundle metadata; channel is explicit.
    let numbers = info.version.split('.').collect::<Vec<_>>();
    assert_eq!(numbers.len(), 3);
    assert!(numbers.iter().all(|part| part.parse::<u32>().is_ok()));
    assert_eq!(info.stage, "BETA");
}

#[test]
fn build_info_serializes_camel_case() {
    let info = BuildInfo::current();
    let json = serde_json::to_value(&info).expect("BuildInfo 可序列化");
    for key in [
        "productName",
        "version",
        "bundleIdentifier",
        "stage",
        "targetOs",
        "targetArch",
        "buildProfile",
        "minimumMacosVersion",
    ] {
        assert!(json.get(key).is_some(), "缺少 camelCase 字段 {key}：{json}");
    }
    assert!(
        json.get("product_name").is_none(),
        "不得出现 snake_case 字段"
    );
}

#[test]
fn app_error_has_stable_code_message_retryable() {
    let error = AppError::forbidden("宿主桥不可用");
    assert_eq!(error.code, ErrorCode::Forbidden);
    assert_eq!(error.code.as_str(), "forbidden");
    assert!(!error.retryable);
    assert_eq!(error.message, "宿主桥不可用");
    let json = serde_json::to_value(&error).expect("AppError 可序列化");
    assert_eq!(json["code"], "forbidden");
    assert_eq!(json["message"], "宿主桥不可用");
    assert_eq!(json["retryable"], false);
}

#[test]
fn error_code_round_trips() {
    let code = ErrorCode::Forbidden;
    let text = serde_json::to_string(&code).expect("ErrorCode 可序列化");
    assert_eq!(text, "\"forbidden\"");
    let back: ErrorCode = serde_json::from_str(&text).expect("ErrorCode 可反序列化");
    assert_eq!(back, code);
}

#[test]
fn event_names_satisfy_tauri_charset() {
    use fleqi_application::dto::AppEvent;
    use fleqi_domain::lifecycle::HostState;
    use fleqi_domain::revision::Revision;
    let events = [
        AppEvent::SettingsChanged {
            revision: Revision::new(1),
        },
        AppEvent::PermissionsChanged {
            revision: Revision::new(1),
        },
        AppEvent::PlatformChanged {
            revision: Revision::new(1),
        },
        AppEvent::ContextChanged {
            context_id: "ctx-1".into(),
            revision: Revision::new(1),
        },
        AppEvent::HostStateChanged {
            state: HostState::Ready,
        },
    ];
    for event in events {
        let name = event.name();
        // Tauri 事件名只允许字母数字与 - / : _；含 `.` 会被 emit/listen 拒绝。
        assert!(
            name.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '/' | ':' | '_')),
            "非法事件名：{name}"
        );
        assert!(name.ends_with(":changed"));
    }
}
