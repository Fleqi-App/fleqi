//! SurfaceService（architecture.md §5.1；FR-ENTRY/FR-SESSION-002..006）：驱动 SurfaceMachine，
//! 执行显示决定（新建会话、隐藏策略、目录同步、后台切换），并维护可见会话。

use fleqi_domain::context::ContextSnapshot;
use fleqi_domain::revision::Revision;
use fleqi_domain::session::Session;
use fleqi_domain::settings::Settings;
use fleqi_domain::surface::{SurfaceEvent, SurfaceMachine, SurfaceOutcome, Visibility};
use std::sync::Arc;
use std::sync::Mutex;

use crate::context_service::ContextService;
use crate::dto::{AppError, AppResult};
use crate::paths::PathRegistry;
use crate::ports::EventSink;
use crate::session_service::SessionService;
use crate::terminal_service::TerminalService;

pub struct SurfaceDeps {
    pub sessions: Arc<SessionService>,
    pub terminal: Arc<TerminalService>,
    pub context: Arc<ContextService>,
    pub paths: Arc<PathRegistry>,
    pub events: Arc<dyn EventSink>,
}

pub struct SurfaceService {
    machine: Mutex<SurfaceMachine>,
    deps: SurfaceDeps,
    hotkey_registered: Mutex<bool>,
    revision: Mutex<u64>,
}

impl SurfaceService {
    pub fn new(settings: &Settings, hotkey_registered: bool, deps: SurfaceDeps) -> Arc<Self> {
        Arc::new(Self {
            machine: Mutex::new(SurfaceMachine::new(settings, hotkey_registered)),
            deps,
            hotkey_registered: Mutex::new(hotkey_registered),
            revision: Mutex::new(0),
        })
    }

    pub fn auto_show_suppressed(&self) -> bool {
        self.machine.lock().expect("surface").auto_show_suppressed()
    }

    pub fn visibility(&self) -> Visibility {
        self.machine.lock().expect("surface").visibility()
    }

    pub fn visible_session(&self) -> Option<Session> {
        let id = self
            .machine
            .lock()
            .expect("surface")
            .visible_session_id()?
            .to_owned();
        self.deps.sessions.get(&id).ok()
    }

    pub fn set_hotkey_registered(&self, registered: bool) {
        *self.hotkey_registered.lock().expect("hotkey") = registered;
        self.machine
            .lock()
            .expect("surface")
            .set_hotkey_registered(registered);
        self.emit();
    }

    pub fn set_settings(&self, settings: &Settings) {
        self.set_settings_for_interaction(settings, true);
    }

    pub fn set_settings_for_interaction(&self, settings: &Settings, interaction_active: bool) {
        let has_directory = self
            .deps
            .context
            .latest()
            .map(|c| {
                matches!(
                    c.availability,
                    fleqi_domain::context::ContextAvailability::Available
                )
            })
            .unwrap_or(false);
        let event = SurfaceEvent::SettingsChanged {
            settings: settings.clone(),
            has_valid_directory: has_directory && interaction_active,
        };
        self.apply(event);
    }

    /// 宿主收到上下文变化时调用（含首次有效目录）。
    pub fn on_context_changed(&self, snapshot: &ContextSnapshot) {
        let has_directory = snapshot.directory_ref.is_some();
        self.apply(SurfaceEvent::ContextDirectoryChanged {
            has_valid_directory: has_directory,
        });
    }

    pub fn user_show(&self) -> AppResult<()> {
        let outcome = self.apply(SurfaceEvent::UserShow);
        if let SurfaceOutcome::Refused { reason } = &outcome {
            return Err(AppError::unavailable(reason.clone()));
        }
        Ok(())
    }

    pub fn user_hide(&self) -> AppResult<()> {
        self.apply(SurfaceEvent::UserHide);
        Ok(())
    }

    pub fn system_hide(&self) {
        self.apply(SurfaceEvent::SystemHide);
    }

    pub fn system_restore(&self) {
        let cycle = self.machine.lock().expect("surface").display_cycle();
        let has_directory = self
            .deps
            .context
            .latest()
            .map(|c| c.directory_ref.is_some())
            .unwrap_or(false);
        self.apply(SurfaceEvent::SystemRestore {
            display_cycle: cycle,
            has_valid_directory: has_directory,
        });
    }

    pub fn select_session(&self, session_id: &str) -> AppResult<Session> {
        let session = self.deps.sessions.get(session_id)?;
        if session.state.is_active() {
            // 活跃会话：恢复其终端与目录同步（历史则由 UI 只读展示）。
            self.apply(SurfaceEvent::SelectSession {
                session_id: session_id.to_owned(),
            });
            self.sync_visible_to_context();
            return self.deps.sessions.get(session_id);
        }
        Ok(session)
    }

    fn sync_visible_to_context(&self) {
        let Some(session_id) = self
            .machine
            .lock()
            .expect("surface")
            .visible_session_id()
            .map(|s| s.to_owned())
        else {
            return;
        };
        let Some(snapshot) = self.deps.context.latest() else {
            return;
        };
        let Some(directory) = snapshot.directory_ref.as_ref() else {
            return;
        };
        if let Some(target) = self.deps.paths.resolve(&directory.id) {
            let _ = self.deps.terminal.target_changed(
                &session_id,
                &target,
                &directory.display_path,
                snapshot.revision,
            );
        }
    }

    fn apply(&self, event: SurfaceEvent) -> SurfaceOutcome {
        // UserHide 与 barEnabled 关闭会在状态机内先清除可见会话；
        // 隐藏副作用（撤销在途投递/排队命令）需要事件前的可见会话。
        let session_before = self
            .machine
            .lock()
            .expect("surface")
            .visible_session_id()
            .map(|s| s.to_owned());
        let outcome = self.machine.lock().expect("surface").apply(event);
        self.execute(&outcome, session_before.as_deref());
        if !matches!(outcome, SurfaceOutcome::NoChange) {
            self.emit();
        }
        outcome
    }

    fn execute(&self, outcome: &SurfaceOutcome, session_before: Option<&str>) {
        match outcome {
            SurfaceOutcome::Show { create_session, .. } => {
                if *create_session {
                    let context = self.deps.context.latest();
                    if let Ok(session) = self.deps.sessions.create(context.as_ref(), None) {
                        let _ = self.deps.sessions.touch(&session.id);
                        self.machine.lock().expect("surface").apply(
                            SurfaceEvent::SessionBecameVisible {
                                session_id: session.id.clone(),
                            },
                        );
                    }
                }
            }
            SurfaceOutcome::Hide { end_all, .. } => {
                if let Some(session_id) = session_before {
                    self.deps.terminal.set_visibility(session_id, false, true);
                }
                if *end_all {
                    for id in self.deps.sessions.active_ids() {
                        let _ = self.deps.sessions.begin_end(&id);
                        self.deps.terminal.end_session(&id);
                        let _ = self.deps.sessions.mark_ended(&id, false);
                    }
                }
            }
            SurfaceOutcome::TemporarilyHide { .. } => {
                if let Some(session_id) = session_before {
                    self.deps.terminal.set_visibility(session_id, false, false);
                }
            }
            SurfaceOutcome::Restore { session_id, resync } => {
                self.deps.terminal.set_visibility(session_id, true, false);
                if *resync {
                    self.sync_visible_to_context();
                }
            }
            SurfaceOutcome::SyncDirectory { session_id } => {
                self.deps.terminal.set_visibility(session_id, true, false);
                self.sync_visible_to_context();
            }
            _ => {}
        }
    }

    fn emit(&self) {
        let mut revision = self.revision.lock().expect("surface revision");
        *revision += 1;
        let value = *revision;
        drop(revision);
        self.deps.events.emit(crate::dto::AppEvent::SurfaceChanged {
            revision: Revision::new(value),
        });
    }
}
