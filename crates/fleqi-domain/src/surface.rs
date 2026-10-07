//! 输入条显示状态机（architecture.md §5.1、requirements FR-ENTRY-001..009、ui-design §3.2）。
//! 可见性独立于会话存活；`autoShowSuppressed` 是运行期状态，只由显式显示或实际更改唤起模式清除。

use crate::settings::{Activation, HideBehavior, Settings};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum Visibility {
    Visible,
    TemporarilyHidden,
    UserHidden,
}

/// 显示周期：每次开始显示递增；暂隐恢复必须携带同一周期，否则失效。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DisplayCycle(u64);

#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceEvent {
    /// 用户显式显示（热键/菜单/托盘）。
    UserShow,
    /// 用户主动隐藏（隐藏按钮/热键/菜单）。
    UserHide,
    /// 系统原因临时隐藏（Finder 移动、失焦、空间切换）。
    SystemHide,
    SystemRestore {
        display_cycle: DisplayCycle,
        has_valid_directory: bool,
    },
    /// 活动 Finder 文件夹改变（或首次得到有效上下文）。
    ContextDirectoryChanged { has_valid_directory: bool },
    /// Finder 被激活/切换窗口但目录未变。
    FinderActivated,
    /// 宿主已把某会话连接为可见会话。
    SessionBecameVisible { session_id: String },
    /// 用户从选择器选择会话。
    SelectSession { session_id: String },
    /// 当前可见会话已结束或删除。
    VisibleSessionGone,
    SettingsChanged {
        settings: Settings,
        has_valid_directory: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SurfaceOutcome {
    NoChange,
    /// 显示输入条；`create_session` 为 true 时宿主新建会话后回报 SessionBecameVisible。
    Show {
        create_session: bool,
        focus_input: bool,
    },
    /// 已可见时的显式显示：只聚焦输入。
    Focus,
    Hide {
        behavior: HideBehavior,
        end_all: bool,
    },
    TemporarilyHide {
        pause_delivery: bool,
    },
    Restore {
        session_id: String,
        resync: bool,
    },
    /// 复用当前可见会话并按最新目录同步。
    SyncDirectory {
        session_id: String,
    },
    Refused {
        reason: String,
    },
}

#[derive(Debug, Clone)]
pub struct SurfaceMachine {
    visibility: Visibility,
    visible_session_id: Option<String>,
    auto_show_suppressed: bool,
    display_cycle: DisplayCycle,
    settings: Settings,
    hotkey_registered: bool,
}

impl SurfaceMachine {
    /// `hotkey_registered`：宿主是否已成功注册设置中的快捷键（未注册的候选不算有效绑定）。
    pub fn new(settings: &Settings, hotkey_registered: bool) -> Self {
        Self {
            visibility: Visibility::UserHidden,
            visible_session_id: None,
            auto_show_suppressed: false,
            display_cycle: DisplayCycle(0),
            settings: settings.clone(),
            hotkey_registered: hotkey_registered && settings.hotkey.is_some(),
        }
    }

    pub fn visibility(&self) -> Visibility {
        self.visibility
    }

    pub fn visible_session_id(&self) -> Option<&str> {
        self.visible_session_id.as_deref()
    }

    pub fn auto_show_suppressed(&self) -> bool {
        self.auto_show_suppressed
    }

    pub fn display_cycle(&self) -> DisplayCycle {
        self.display_cycle
    }

    pub fn set_hotkey_registered(&mut self, registered: bool) {
        self.hotkey_registered = registered && self.settings.hotkey.is_some();
    }

    fn start_cycle(&mut self) {
        self.display_cycle = DisplayCycle(self.display_cycle.0 + 1);
        self.visibility = Visibility::Visible;
    }

    fn can_auto_show(&self, has_valid_directory: bool) -> bool {
        self.settings.bar_enabled
            && self.settings.activation == Activation::FollowFinder
            && has_valid_directory
            && !self.auto_show_suppressed
            && self.visibility == Visibility::UserHidden
    }

    pub fn apply(&mut self, event: SurfaceEvent) -> SurfaceOutcome {
        match event {
            SurfaceEvent::UserShow => {
                if !self.settings.bar_enabled {
                    return SurfaceOutcome::Refused {
                        reason: "操作栏已禁用，请先在设置中启用".into(),
                    };
                }
                if self.settings.activation == Activation::Manual && !self.hotkey_registered {
                    return SurfaceOutcome::Refused {
                        reason: "manual 模式需要先注册有效快捷键".into(),
                    };
                }
                self.auto_show_suppressed = false;
                if self.visibility == Visibility::Visible {
                    return SurfaceOutcome::Focus;
                }
                self.start_cycle();
                self.visible_session_id = None;
                SurfaceOutcome::Show {
                    create_session: true,
                    focus_input: true,
                }
            }
            SurfaceEvent::UserHide => {
                if self.visibility == Visibility::UserHidden {
                    return SurfaceOutcome::NoChange;
                }
                self.visibility = Visibility::UserHidden;
                self.visible_session_id = None;
                self.auto_show_suppressed = true;
                self.display_cycle = DisplayCycle(self.display_cycle.0 + 1);
                let behavior = self.settings.hide_behavior;
                SurfaceOutcome::Hide {
                    behavior,
                    end_all: behavior == HideBehavior::EndAll,
                }
            }
            SurfaceEvent::SystemHide => {
                if self.visibility != Visibility::Visible {
                    return SurfaceOutcome::NoChange;
                }
                self.visibility = Visibility::TemporarilyHidden;
                SurfaceOutcome::TemporarilyHide {
                    pause_delivery: true,
                }
            }
            SurfaceEvent::SystemRestore {
                display_cycle,
                has_valid_directory,
            } => {
                if self.visibility != Visibility::TemporarilyHidden
                    || display_cycle != self.display_cycle
                    || !self.settings.bar_enabled
                {
                    return SurfaceOutcome::NoChange;
                }
                let Some(session_id) = self.visible_session_id.clone() else {
                    self.visibility = Visibility::UserHidden;
                    return SurfaceOutcome::NoChange;
                };
                self.visibility = Visibility::Visible;
                SurfaceOutcome::Restore {
                    session_id,
                    resync: has_valid_directory,
                }
            }
            SurfaceEvent::ContextDirectoryChanged {
                has_valid_directory,
            } => {
                if self.visibility == Visibility::Visible {
                    return match &self.visible_session_id {
                        Some(id) if has_valid_directory => SurfaceOutcome::SyncDirectory {
                            session_id: id.clone(),
                        },
                        _ => SurfaceOutcome::NoChange,
                    };
                }
                if self.can_auto_show(has_valid_directory) {
                    self.start_cycle();
                    self.visible_session_id = None;
                    return SurfaceOutcome::Show {
                        create_session: true,
                        focus_input: false,
                    };
                }
                SurfaceOutcome::NoChange
            }
            SurfaceEvent::FinderActivated => SurfaceOutcome::NoChange,
            SurfaceEvent::SessionBecameVisible { session_id } => {
                self.visible_session_id = Some(session_id);
                SurfaceOutcome::NoChange
            }
            SurfaceEvent::SelectSession { session_id } => {
                self.visible_session_id = Some(session_id.clone());
                if self.visibility != Visibility::Visible {
                    self.start_cycle();
                }
                SurfaceOutcome::SyncDirectory { session_id }
            }
            SurfaceEvent::VisibleSessionGone => {
                self.visible_session_id = None;
                SurfaceOutcome::NoChange
            }
            SurfaceEvent::SettingsChanged {
                settings,
                has_valid_directory,
            } => {
                let previous = std::mem::replace(&mut self.settings, settings);
                self.hotkey_registered = self.hotkey_registered && self.settings.hotkey.is_some();
                let activation_changed = previous.activation != self.settings.activation;
                if activation_changed {
                    self.auto_show_suppressed = false;
                }
                if previous.bar_enabled && !self.settings.bar_enabled {
                    // 用户显式关闭操作栏：按当前隐藏策略处理一次。
                    let behavior = self.settings.hide_behavior;
                    let was_visible = self.visibility != Visibility::UserHidden;
                    self.visibility = Visibility::UserHidden;
                    self.visible_session_id = None;
                    self.display_cycle = DisplayCycle(self.display_cycle.0 + 1);
                    return if was_visible || behavior == HideBehavior::EndAll {
                        SurfaceOutcome::Hide {
                            behavior,
                            end_all: behavior == HideBehavior::EndAll,
                        }
                    } else {
                        SurfaceOutcome::NoChange
                    };
                }
                if activation_changed {
                    if self.visibility == Visibility::Visible {
                        return SurfaceOutcome::NoChange;
                    }
                    if self.can_auto_show(has_valid_directory) {
                        self.start_cycle();
                        self.visible_session_id = None;
                        return SurfaceOutcome::Show {
                            create_session: true,
                            focus_input: false,
                        };
                    }
                }
                SurfaceOutcome::NoChange
            }
        }
    }
}
