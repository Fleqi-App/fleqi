//! M2 领域合同测试：显示状态机（§5.1）、目录同步协议（§5.2）、会话生命周期（§6.1）、`!` 解析（FR-TERM-001/002）。

use fleqi_domain::composer::{ComposerMode, parse_composer_input};
use fleqi_domain::directory_sync::{
    DirectorySync, QueuedLine, ShellReadiness, SyncDecision, SyncMachine, SyncOutcome,
};
use fleqi_domain::revision::Revision;
use fleqi_domain::session::{SessionState, next_state_after_end, validate_active_limit};
use fleqi_domain::settings::{Activation, HideBehavior, Settings, limits};
use fleqi_domain::surface::{SurfaceEvent, SurfaceMachine, SurfaceOutcome, Visibility};

fn settings(activation: Activation, hotkey: bool, bar_enabled: bool) -> Settings {
    Settings {
        activation,
        bar_enabled,
        hotkey: hotkey.then(|| fleqi_domain::settings::Hotkey {
            key: "F9".into(),
            modifiers: vec![],
        }),
        ..Settings::default()
    }
}

// ---------- 显示状态机 ----------

#[test]
fn changing_activation_while_disabling_clears_previous_hide_suppression() {
    let mut machine = SurfaceMachine::new(&settings(Activation::Manual, true, true), true);
    machine.apply(SurfaceEvent::UserShow);
    machine.apply(SurfaceEvent::UserHide);
    assert!(machine.auto_show_suppressed());
    machine.apply(SurfaceEvent::SettingsChanged {
        settings: settings(Activation::FollowFinder, true, false),
        has_valid_directory: true,
    });
    assert!(!machine.auto_show_suppressed());
    assert_eq!(machine.visibility(), Visibility::UserHidden);
    machine.apply(SurfaceEvent::SettingsChanged {
        settings: settings(Activation::FollowFinder, true, true),
        has_valid_directory: true,
    });
    assert!(matches!(
        machine.apply(SurfaceEvent::ContextDirectoryChanged {
            has_valid_directory: true
        }),
        SurfaceOutcome::Show {
            focus_input: false,
            ..
        }
    ));
}

#[test]
fn manual_without_hotkey_never_auto_shows_and_manual_show_needs_hotkey() {
    let mut m = SurfaceMachine::new(&settings(Activation::Manual, false, true), true);
    assert_eq!(m.visibility(), Visibility::UserHidden);
    let out = m.apply(SurfaceEvent::ContextDirectoryChanged {
        has_valid_directory: true,
    });
    assert_eq!(out, SurfaceOutcome::NoChange);
    let out = m.apply(SurfaceEvent::UserShow);
    assert_eq!(
        out,
        SurfaceOutcome::Refused {
            reason: "manual 模式需要先注册有效快捷键".into()
        }
    );
}

#[test]
fn manual_with_hotkey_show_creates_new_session_and_hide_sets_suppression() {
    let mut m = SurfaceMachine::new(&settings(Activation::Manual, true, true), true);
    let out = m.apply(SurfaceEvent::UserShow);
    assert_eq!(
        out,
        SurfaceOutcome::Show {
            create_session: true,
            focus_input: true
        }
    );
    assert_eq!(m.visibility(), Visibility::Visible);
    m.apply(SurfaceEvent::SessionBecameVisible {
        session_id: "s1".into(),
    });
    // 再次 UserShow 在已可见时不重复创建。
    assert_eq!(m.apply(SurfaceEvent::UserShow), SurfaceOutcome::Focus);
    let out = m.apply(SurfaceEvent::UserHide);
    assert_eq!(
        out,
        SurfaceOutcome::Hide {
            behavior: HideBehavior::KeepAll,
            end_all: false
        }
    );
    assert_eq!(m.visibility(), Visibility::UserHidden);
    assert!(m.auto_show_suppressed());
    assert!(m.visible_session_id().is_none());
}

#[test]
fn follow_finder_auto_shows_without_hotkey_and_respects_suppression() {
    let mut m = SurfaceMachine::new(&settings(Activation::FollowFinder, false, true), true);
    assert_eq!(
        m.apply(SurfaceEvent::ContextDirectoryChanged {
            has_valid_directory: false
        }),
        SurfaceOutcome::NoChange
    );
    let out = m.apply(SurfaceEvent::ContextDirectoryChanged {
        has_valid_directory: true,
    });
    assert_eq!(
        out,
        SurfaceOutcome::Show {
            create_session: true,
            focus_input: false
        }
    );
    m.apply(SurfaceEvent::SessionBecameVisible {
        session_id: "s1".into(),
    });
    // 栏已显示时目录变化：复用当前会话，请求同步。
    assert_eq!(
        m.apply(SurfaceEvent::ContextDirectoryChanged {
            has_valid_directory: true
        }),
        SurfaceOutcome::SyncDirectory {
            session_id: "s1".into()
        }
    );
    // 主动隐藏后普通 Finder 变化不弹回。
    m.apply(SurfaceEvent::UserHide);
    assert!(m.auto_show_suppressed());
    assert_eq!(
        m.apply(SurfaceEvent::ContextDirectoryChanged {
            has_valid_directory: true
        }),
        SurfaceOutcome::NoChange
    );
    assert_eq!(
        m.apply(SurfaceEvent::FinderActivated),
        SurfaceOutcome::NoChange
    );
    // 显式显示解除抑制并新建。
    assert_eq!(
        m.apply(SurfaceEvent::UserShow),
        SurfaceOutcome::Show {
            create_session: true,
            focus_input: true
        }
    );
    assert!(!m.auto_show_suppressed());
}

#[test]
fn activation_change_clears_suppression_and_reevaluates() {
    let mut m = SurfaceMachine::new(&settings(Activation::FollowFinder, false, true), true);
    m.apply(SurfaceEvent::ContextDirectoryChanged {
        has_valid_directory: true,
    });
    m.apply(SurfaceEvent::SessionBecameVisible {
        session_id: "s1".into(),
    });
    m.apply(SurfaceEvent::UserHide);
    assert!(m.auto_show_suppressed());
    let out = m.apply(SurfaceEvent::SettingsChanged {
        settings: settings(Activation::FollowFinder, false, true),
        has_valid_directory: true,
    });
    // 值未变：不解除抑制。
    assert_eq!(out, SurfaceOutcome::NoChange);
    assert!(m.auto_show_suppressed());
    let out = m.apply(SurfaceEvent::SettingsChanged {
        settings: settings(Activation::Manual, false, true),
        has_valid_directory: true,
    });
    assert_eq!(out, SurfaceOutcome::NoChange);
    assert!(!m.auto_show_suppressed(), "实际更改唤起模式清除抑制");
    let out = m.apply(SurfaceEvent::SettingsChanged {
        settings: settings(Activation::FollowFinder, false, true),
        has_valid_directory: true,
    });
    assert_eq!(
        out,
        SurfaceOutcome::Show {
            create_session: true,
            focus_input: false
        }
    );
}

#[test]
fn temporary_hide_keeps_session_and_restore_reuses_it() {
    let mut m = SurfaceMachine::new(&settings(Activation::FollowFinder, false, true), true);
    m.apply(SurfaceEvent::ContextDirectoryChanged {
        has_valid_directory: true,
    });
    m.apply(SurfaceEvent::SessionBecameVisible {
        session_id: "s1".into(),
    });
    let out = m.apply(SurfaceEvent::SystemHide);
    assert_eq!(
        out,
        SurfaceOutcome::TemporarilyHide {
            pause_delivery: true
        }
    );
    assert_eq!(m.visibility(), Visibility::TemporarilyHidden);
    assert_eq!(m.visible_session_id(), Some("s1"));
    let cycle = m.display_cycle();
    let out = m.apply(SurfaceEvent::SystemRestore {
        display_cycle: cycle,
        has_valid_directory: true,
    });
    assert_eq!(
        out,
        SurfaceOutcome::Restore {
            session_id: "s1".into(),
            resync: true
        }
    );
    assert_eq!(m.visibility(), Visibility::Visible);
    // 暂隐期间用户主动隐藏，旧恢复事件失效。
    m.apply(SurfaceEvent::SystemHide);
    let stale_cycle = m.display_cycle();
    m.apply(SurfaceEvent::UserHide);
    assert_eq!(
        m.apply(SurfaceEvent::SystemRestore {
            display_cycle: stale_cycle,
            has_valid_directory: true
        }),
        SurfaceOutcome::NoChange
    );
    assert_eq!(m.visibility(), Visibility::UserHidden);
}

#[test]
fn bar_disable_applies_hide_behavior_once_and_end_all_when_configured() {
    let mut s = settings(Activation::FollowFinder, false, true);
    s.hide_behavior = HideBehavior::EndAll;
    let mut m = SurfaceMachine::new(&s, true);
    m.apply(SurfaceEvent::ContextDirectoryChanged {
        has_valid_directory: true,
    });
    m.apply(SurfaceEvent::SessionBecameVisible {
        session_id: "s1".into(),
    });
    let mut disabled = s.clone();
    disabled.bar_enabled = false;
    let out = m.apply(SurfaceEvent::SettingsChanged {
        settings: disabled.clone(),
        has_valid_directory: true,
    });
    assert_eq!(
        out,
        SurfaceOutcome::Hide {
            behavior: HideBehavior::EndAll,
            end_all: true
        }
    );
    // 重复的 false 不再处理。
    assert_eq!(
        m.apply(SurfaceEvent::SettingsChanged {
            settings: disabled,
            has_valid_directory: true
        }),
        SurfaceOutcome::NoChange
    );
    // 禁用状态下任何唤起都不显示。
    assert!(matches!(
        m.apply(SurfaceEvent::UserShow),
        SurfaceOutcome::Refused { .. }
    ));
    assert_eq!(
        m.apply(SurfaceEvent::ContextDirectoryChanged {
            has_valid_directory: true
        }),
        SurfaceOutcome::NoChange
    );
}

#[test]
fn select_background_session_makes_it_visible_and_requests_sync() {
    let mut m = SurfaceMachine::new(&settings(Activation::Manual, true, true), true);
    m.apply(SurfaceEvent::UserShow);
    m.apply(SurfaceEvent::SessionBecameVisible {
        session_id: "s1".into(),
    });
    let out = m.apply(SurfaceEvent::SelectSession {
        session_id: "s2".into(),
    });
    assert_eq!(
        out,
        SurfaceOutcome::SyncDirectory {
            session_id: "s2".into()
        }
    );
    assert_eq!(m.visible_session_id(), Some("s2"));
}

// ---------- 目录同步协议 ----------

fn ready() -> ShellReadiness {
    ShellReadiness {
        prompt_ready: true,
        edit_line_empty: true,
        foreground_is_shell: true,
        delivering: false,
    }
}

#[test]
fn directory_target_change_applies_when_shell_safe_and_merges_latest_when_busy() {
    let mut m = SyncMachine::new("/a");
    assert_eq!(m.state(), DirectorySync::Synced);
    let d = m.target_changed("/b", Revision::new(1), &ready(), true);
    assert_eq!(
        d,
        SyncDecision::SendCd {
            target: "/b".into(),
            revision: Revision::new(1)
        }
    );
    assert_eq!(m.state(), DirectorySync::Syncing);
    assert_eq!(
        m.on_cd_result(
            Revision::new(1),
            SyncOutcome::Confirmed { cwd: "/b".into() }
        ),
        Some(SyncDecision::Synced { cwd: "/b".into() })
    );
    assert_eq!(m.current(), "/b");

    let busy = ShellReadiness {
        prompt_ready: false,
        ..ready()
    };
    assert_eq!(
        m.target_changed("/c", Revision::new(2), &busy, true),
        SyncDecision::Pending {
            target: "/c".into()
        }
    );
    assert_eq!(
        m.target_changed("/d", Revision::new(3), &busy, true),
        SyncDecision::Pending {
            target: "/d".into()
        }
    );
    assert_eq!(m.pending_target(), Some("/d"));
    // 安全提示符恢复：只应用最新 D。
    assert_eq!(
        m.shell_became_safe(&ready(), true),
        Some(SyncDecision::SendCd {
            target: "/d".into(),
            revision: Revision::new(3)
        })
    );
    // 过期回执（版本 2）不能覆盖。
    assert_eq!(
        m.on_cd_result(
            Revision::new(2),
            SyncOutcome::Confirmed { cwd: "/c".into() }
        ),
        None
    );
    assert_eq!(m.current(), "/b");
    assert_eq!(
        m.on_cd_result(
            Revision::new(3),
            SyncOutcome::Confirmed { cwd: "/d".into() }
        ),
        Some(SyncDecision::Synced { cwd: "/d".into() })
    );
}

#[test]
fn edit_line_not_empty_or_foreground_program_keeps_pending() {
    let mut m = SyncMachine::new("/a");
    let editing = ShellReadiness {
        edit_line_empty: false,
        ..ready()
    };
    assert_eq!(
        m.target_changed("/b", Revision::new(1), &editing, true),
        SyncDecision::Pending {
            target: "/b".into()
        }
    );
    let program = ShellReadiness {
        foreground_is_shell: false,
        ..ready()
    };
    assert_eq!(m.shell_became_safe(&program, true), None);
    assert_eq!(m.state(), DirectorySync::Pending);
    assert_eq!(
        m.shell_became_safe(&ready(), true),
        Some(SyncDecision::SendCd {
            target: "/b".into(),
            revision: Revision::new(1)
        })
    );
}

#[test]
fn cd_failure_keeps_real_cwd_and_allows_retry() {
    let mut m = SyncMachine::new("/a");
    m.target_changed("/gone", Revision::new(1), &ready(), true);
    let d = m.on_cd_result(
        Revision::new(1),
        SyncOutcome::Failed {
            cwd: "/a".into(),
            message: "No such file".into(),
        },
    );
    assert_eq!(
        d,
        Some(SyncDecision::Failed {
            cwd: "/a".into(),
            message: "No such file".into()
        })
    );
    assert_eq!(m.state(), DirectorySync::Failed);
    assert_eq!(m.current(), "/a");
    assert_eq!(
        m.retry(&ready(), true),
        Some(SyncDecision::SendCd {
            target: "/gone".into(),
            revision: Revision::new(1)
        })
    );
}

#[test]
fn queued_line_sends_once_after_sync_and_is_withdrawn_when_target_changes() {
    let mut m = SyncMachine::new("/a");
    let busy = ShellReadiness {
        prompt_ready: false,
        ..ready()
    };
    m.target_changed("/b", Revision::new(1), &busy, true);
    let q = QueuedLine {
        request_id: "r1".into(),
        session_id: "s1".into(),
        context_revision: Revision::new(1),
        target: "/b".into(),
        text: "ls".into(),
    };
    assert_eq!(m.queue_line(q.clone()), Ok(()));
    assert!(
        m.queue_line(q.clone()).is_err(),
        "每会话仅保留一个等待发送的命令"
    );
    // 目标再变：撤销自动投递，草稿保留。
    let d = m.target_changed("/c", Revision::new(2), &busy, true);
    assert_eq!(
        d,
        SyncDecision::Pending {
            target: "/c".into()
        }
    );
    assert_eq!(m.take_withdrawn_line(), Some(q.clone()));
    assert!(m.queued_line().is_none());
    // 重新提交到 C 并同步成功后自动发送一次。
    let q2 = QueuedLine {
        target: "/c".into(),
        context_revision: Revision::new(2),
        ..q
    };
    m.queue_line(q2.clone()).unwrap();
    m.shell_became_safe(&ready(), true);
    let d = m.on_cd_result(
        Revision::new(2),
        SyncOutcome::Confirmed { cwd: "/c".into() },
    );
    assert_eq!(
        d,
        Some(SyncDecision::SyncedAndSend {
            cwd: "/c".into(),
            line: q2
        })
    );
    assert!(m.queued_line().is_none());
}

#[test]
fn background_or_hidden_session_cancels_undelivered_cd_and_queued_line_but_keeps_late_receipt() {
    let mut m = SyncMachine::new("/a");
    let busy = ShellReadiness {
        prompt_ready: false,
        ..ready()
    };
    m.target_changed("/b", Revision::new(1), &busy, true);
    m.queue_line(QueuedLine {
        request_id: "r1".into(),
        session_id: "s1".into(),
        context_revision: Revision::new(1),
        target: "/b".into(),
        text: "ls".into(),
    })
    .unwrap();
    let withdrawn = m.went_background();
    assert_eq!(withdrawn.map(|l| l.request_id), Some("r1".into()));
    assert!(m.pending_target().is_none(), "后台不再自动 cd");
    // 后台到达提示符不投递。
    assert_eq!(m.shell_became_safe(&ready(), false), None);
    // 在途 cd 的晚回执只更新真实 cwd。
    let mut n = SyncMachine::new("/a");
    n.target_changed("/b", Revision::new(1), &ready(), true);
    n.went_background();
    assert_eq!(
        n.on_cd_result(
            Revision::new(1),
            SyncOutcome::Confirmed { cwd: "/b".into() }
        ),
        Some(SyncDecision::Synced { cwd: "/b".into() })
    );
    assert_eq!(n.current(), "/b");
}

#[test]
fn manual_cd_updates_real_cwd_without_pulling_back() {
    let mut m = SyncMachine::new("/a");
    m.manual_cwd_changed("/x");
    assert_eq!(m.current(), "/x");
    assert_eq!(m.state(), DirectorySync::Synced);
    assert!(m.pending_target().is_none());
}

// ---------- 会话 ----------

#[test]
fn active_session_limit_is_16_and_end_transitions() {
    assert!(validate_active_limit(15).is_ok());
    assert_eq!(
        validate_active_limit(limits::ACTIVE_SESSIONS as usize).unwrap_err(),
        limits::ACTIVE_SESSIONS
    );
    assert_eq!(
        next_state_after_end(SessionState::Active),
        SessionState::Ending
    );
    assert_eq!(
        next_state_after_end(SessionState::Ending),
        SessionState::Ending
    );
    assert_eq!(
        next_state_after_end(SessionState::Ended),
        SessionState::Ended
    );
}

// ---------- `!` 解析 ----------

#[test]
fn manual_marker_only_when_first_non_blank_char_is_ascii_bang() {
    assert_eq!(
        parse_composer_input("!ls -la"),
        (ComposerMode::Terminal, "ls -la".to_string())
    );
    assert_eq!(
        parse_composer_input("  \t!echo \"a | b\""),
        (ComposerMode::Terminal, "echo \"a | b\"".to_string())
    );
    assert_eq!(
        parse_composer_input("！ls"),
        (ComposerMode::Ai, "！ls".to_string())
    );
    assert_eq!(
        parse_composer_input("hello !world"),
        (ComposerMode::Ai, "hello !world".to_string())
    );
    assert_eq!(
        parse_composer_input("!!double"),
        (ComposerMode::Terminal, "!double".to_string())
    );
    assert_eq!(
        parse_composer_input("!   "),
        (ComposerMode::Terminal, "   ".to_string())
    );
    assert_eq!(parse_composer_input(""), (ComposerMode::Ai, String::new()));
}
