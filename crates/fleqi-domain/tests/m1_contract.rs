//! M1.1 领域合同测试：需求 §2.1 默认值、补丁校验、幂等判定、权限撤销推导、上下文规则。

use fleqi_domain::context::{
    ContextAvailability, ContextSnapshotBuilder, PathKind, PathRef, SELECTION_LIMIT, ViewKind,
};
use fleqi_domain::idempotency::{Receipt, ReceiptOutcome, evaluate_receipt};
use fleqi_domain::lifecycle::{Generation, HostState};
use fleqi_domain::permissions::{
    Permission, PermissionProcedure, PermissionRecord, PermissionStatus, RecoveryAction,
};
use fleqi_domain::revision::Revision;
use fleqi_domain::settings::{
    Activation, AiPolicy, FieldAllowlist, HideBehavior, MotionMode, NameConflict, OutputLocation,
    Settings, SettingsPatch, SummaryModel, Theme,
};

#[test]
fn defaults_follow_requirements_table() {
    let s = Settings::default();
    assert!(s.bar_enabled);
    assert_eq!(s.activation, Activation::Manual);
    assert!(s.hotkey.is_none());
    assert_eq!(s.hide_behavior, HideBehavior::KeepAll);
    assert_eq!(s.ai_policy, AiPolicy::ReadOnlyAutoConfirmChanges);
    assert!(!s.launch_at_login);
    assert_eq!(s.theme, Theme::Dark);
    assert_eq!(s.bubble_seconds, Some(4.8));
    assert!(s.inline_suggestions_enabled);
    assert_eq!(s.inline_suggestions_limit, 3);
    assert!(s.transparency);
    assert_eq!(s.motion_mode, MotionMode::System);
    assert!(s.providers.is_empty());
    assert!(s.default_model.is_none());
    assert_eq!(s.summary_model, SummaryModel::Default);
    assert_eq!(s.terminal_font_size, 13);
    assert_eq!(s.output_location, OutputLocation::BesideSource);
    assert_eq!(s.name_conflict, NameConflict::UniqueName);
    assert_eq!(fleqi_domain::settings::limits::AI_CONCURRENCY, 4);
    assert_eq!(fleqi_domain::settings::limits::ACTIVE_SESSIONS, 16);
    assert_eq!(fleqi_domain::settings::limits::SELECTION_ITEMS, 1000);
    assert_eq!(fleqi_domain::settings::limits::INPUT_HISTORY, 200);
    assert_eq!(fleqi_domain::settings::limits::HISTORY_RETENTION_DAYS, 30);
    assert_eq!(
        fleqi_domain::settings::limits::RUN_OUTPUT_BYTES,
        50 * 1024 * 1024
    );
    assert_eq!(
        fleqi_domain::settings::limits::SESSION_OUTPUT_BYTES,
        100 * 1024 * 1024
    );
    assert_eq!(fleqi_domain::settings::limits::SCROLLBACK_LINES, 10_000);
}

#[test]
fn conversion_preference_defaults_for_old_settings_and_validates_patch() {
    use fleqi_domain::settings::ConversionSourceHandling;
    let mut old = serde_json::to_value(Settings::default()).unwrap();
    old.as_object_mut()
        .unwrap()
        .remove("conversionSourceHandling");
    let loaded: Settings = serde_json::from_value(old).unwrap();
    assert_eq!(
        loaded.conversion_source_handling,
        ConversionSourceHandling::Keep
    );
    let patch: SettingsPatch =
        serde_json::from_str(r#"{"conversionSourceHandling":"trashAfterSuccess"}"#).unwrap();
    let changed = loaded.apply_patch(&patch, &FieldAllowlist::All).unwrap();
    let roundtrip: Settings =
        serde_json::from_str(&serde_json::to_string(&changed).unwrap()).unwrap();
    assert_eq!(
        roundtrip.conversion_source_handling,
        ConversionSourceHandling::TrashAfterSuccess
    );
    assert!(
        serde_json::from_str::<SettingsPatch>(
            r#"{"conversionSourceHandling":"deleteImmediately"}"#
        )
        .is_err()
    );
}

#[test]
fn both_name_conflict_modes_survive_settings_roundtrip() {
    let mut settings = Settings::default();
    for value in ["overwrite", "uniqueName"] {
        let patch: SettingsPatch =
            serde_json::from_value(serde_json::json!({"nameConflict": value})).unwrap();
        settings = settings.apply_patch(&patch, &FieldAllowlist::All).unwrap();
        let roundtrip: Settings =
            serde_json::from_value(serde_json::to_value(&settings).unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(roundtrip).unwrap()["nameConflict"],
            value
        );
    }
}

#[test]
fn enums_serialize_with_contract_names() {
    assert_eq!(
        serde_json::to_string(&Activation::FollowFinder).unwrap(),
        "\"followFinder\""
    );
    assert_eq!(
        serde_json::to_string(&HideBehavior::EndAll).unwrap(),
        "\"endAll\""
    );
    assert_eq!(
        serde_json::to_string(&AiPolicy::ReadOnlyAutoConfirmChanges).unwrap(),
        "\"readOnlyAutoConfirmChanges\""
    );
    assert_eq!(serde_json::to_string(&AiPolicy::Yolo).unwrap(), "\"yolo\"");
    assert_eq!(serde_json::to_string(&Theme::System).unwrap(), "\"system\"");
    assert_eq!(
        serde_json::to_string(&MotionMode::Reduce).unwrap(),
        "\"reduce\""
    );
    let json = serde_json::to_value(Settings::default()).unwrap();
    assert_eq!(json["hideBehavior"], "keepAll");
    assert_eq!(json["aiPolicy"], "readOnlyAutoConfirmChanges");
    assert_eq!(json["bubbleSeconds"], 4.8);
    assert!(json["hotkey"].is_null());
}

#[test]
fn m1_allowlist_accepts_only_theme_transparency_motion() {
    let base = Settings::default();
    let patch: SettingsPatch =
        serde_json::from_str(r#"{"theme":"light","transparency":false,"motionMode":"reduce"}"#)
            .unwrap();
    let next = base
        .apply_patch(&patch, &FieldAllowlist::M1)
        .expect("M1 允许字段");
    assert_eq!(next.theme, Theme::Light);
    assert!(!next.transparency);
    assert_eq!(next.motion_mode, MotionMode::Reduce);

    let blocked: SettingsPatch =
        serde_json::from_str(r#"{"theme":"light","activation":"followFinder"}"#).unwrap();
    let errors = base.apply_patch(&blocked, &FieldAllowlist::M1).unwrap_err();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].field, "activation");
    assert_eq!(errors[0].code, "notAvailable");
}

#[test]
fn patch_validation_reports_each_invalid_field() {
    let base = Settings::default();
    let patch: SettingsPatch = serde_json::from_str(
        r#"{"bubbleSeconds":45,"inlineSuggestionsLimit":9,"terminalFontSize":8}"#,
    )
    .unwrap();
    let errors = base.apply_patch(&patch, &FieldAllowlist::All).unwrap_err();
    let fields: Vec<_> = errors.iter().map(|e| e.field.as_str()).collect();
    assert_eq!(
        fields,
        vec![
            "bubbleSeconds",
            "inlineSuggestionsLimit",
            "terminalFontSize"
        ]
    );
    assert!(errors.iter().all(|e| e.code == "invalid"));
}

#[test]
fn nullable_patch_fields_distinguish_missing_from_null() {
    let base = Settings {
        bubble_seconds: Some(10.0),
        ..Settings::default()
    };
    let untouched: SettingsPatch = serde_json::from_str(r#"{"theme":"system"}"#).unwrap();
    assert_eq!(
        base.apply_patch(&untouched, &FieldAllowlist::All)
            .unwrap()
            .bubble_seconds,
        Some(10.0)
    );
    let cleared: SettingsPatch = serde_json::from_str(r#"{"bubbleSeconds":null}"#).unwrap();
    assert_eq!(
        base.apply_patch(&cleared, &FieldAllowlist::All)
            .unwrap()
            .bubble_seconds,
        None
    );
    let empty: SettingsPatch = serde_json::from_str("{}").unwrap();
    assert!(empty.is_empty());
}

#[test]
fn revision_is_decimal_string_across_ipc() {
    let rev = Revision::new(42);
    assert_eq!(serde_json::to_string(&rev).unwrap(), "\"42\"");
    let back: Revision = serde_json::from_str("\"43\"").unwrap();
    assert_eq!(back, Revision::new(43));
    assert_eq!(rev.next(), Revision::new(43));
    assert!(serde_json::from_str::<Revision>("\"abc\"").is_err());
}

#[test]
fn receipt_replays_same_payload_and_conflicts_on_different_payload() {
    let stored = Receipt {
        request_id: "r1".into(),
        fingerprint: "fp-a".into(),
        result_json: "{\"ok\":true}".into(),
    };
    assert_eq!(evaluate_receipt(None, "r1", "fp-a"), ReceiptOutcome::Fresh);
    assert_eq!(
        evaluate_receipt(Some(&stored), "r1", "fp-a"),
        ReceiptOutcome::Replay("{\"ok\":true}".into())
    );
    assert_eq!(
        evaluate_receipt(Some(&stored), "r1", "fp-b"),
        ReceiptOutcome::Conflict
    );
}

#[test]
fn permission_revocation_only_from_previous_allowed() {
    let allowed = PermissionRecord::new(
        Permission::FinderAutomation,
        PermissionStatus::Allowed,
        PermissionProcedure::Passive,
        "t1",
    );
    let denied = allowed.transition(PermissionStatus::Denied, PermissionProcedure::Passive, "t2");
    assert!(denied.revoked);
    assert_eq!(denied.recovery, RecoveryAction::OpenSystemSettings);
    let still_denied =
        denied.transition(PermissionStatus::Denied, PermissionProcedure::Passive, "t3");
    assert!(!still_denied.revoked);
    let not_running = allowed.transition(
        PermissionStatus::TargetNotRunning,
        PermissionProcedure::Passive,
        "t4",
    );
    assert!(!not_running.revoked, "Finder 未运行不是撤销");
    assert_eq!(not_running.recovery, RecoveryAction::LaunchTarget);
    assert_eq!(
        PermissionRecord::new(
            Permission::Accessibility,
            PermissionStatus::NeedsConsent,
            PermissionProcedure::Passive,
            "t"
        )
        .recovery,
        RecoveryAction::RequestExplicitly
    );
    assert_eq!(
        serde_json::to_string(&Permission::FinderAutomation).unwrap(),
        "\"finderAutomation\""
    );
}

#[test]
fn context_snapshot_rules_for_virtual_views_and_selection_limit() {
    let dir = PathRef::new("p1", "/Users/me/Docs", PathKind::Directory);
    let physical = ContextSnapshotBuilder::new("c1", Revision::new(1), "2026-09-17T00:00:00Z")
        .finder_window(7)
        .view_kind(ViewKind::Physical)
        .directory(Some(dir.clone()))
        .selection(vec![PathRef::new(
            "f1",
            "/Users/me/Docs/a.txt",
            PathKind::File,
        )])
        .build();
    assert_eq!(physical.availability, ContextAvailability::Available);
    assert!(physical.selection_complete);

    let virtual_no_dir = ContextSnapshotBuilder::new("c2", Revision::new(2), "t")
        .view_kind(ViewKind::Virtual)
        .directory(None)
        .selection(vec![PathRef::new("f2", "/Users/me/x.pdf", PathKind::File)])
        .build();
    assert!(matches!(
        virtual_no_dir.availability,
        ContextAvailability::NoDirectory { .. }
    ));
    assert_eq!(
        virtual_no_dir.selected_items.len(),
        1,
        "虚拟视图仍可呈现真实选中文件"
    );

    let too_many: Vec<PathRef> = (0..=SELECTION_LIMIT)
        .map(|i| PathRef::new(format!("s{i}"), format!("/tmp/{i}"), PathKind::File))
        .collect();
    let over = ContextSnapshotBuilder::new("c3", Revision::new(3), "t")
        .directory(Some(dir))
        .selection(too_many)
        .build();
    assert_eq!(
        over.availability,
        ContextAvailability::SelectionOverLimit {
            count: SELECTION_LIMIT + 1,
            limit: SELECTION_LIMIT
        }
    );
    assert!(over.selected_items.is_empty(), "超限不截取前 1000 项");
    assert!(!over.selection_complete);
}

#[test]
fn lifecycle_transitions_and_generation_invalidate_late_results() {
    assert!(HostState::Starting.can_transition_to(HostState::Ready));
    assert!(HostState::Starting.can_transition_to(HostState::Degraded));
    assert!(HostState::Ready.can_transition_to(HostState::Stopping));
    assert!(!HostState::Stopping.can_transition_to(HostState::Ready));
    assert!(!HostState::Ready.accepts_changes() || HostState::Ready.accepts_changes());
    assert!(HostState::Ready.accepts_changes());
    assert!(!HostState::Stopping.accepts_changes());
    let g1 = Generation::initial();
    let g2 = g1.next();
    assert!(g2.is_current(g2));
    assert!(!g2.is_current(g1), "旧代际回执失效");
}
