//! M2 编排测试：SurfaceService + SessionService + TerminalService（脚本化终端端口）。
//! 覆盖 FR-SESSION-002/004/005、FR-CTX-002/004/006、FR-TERM-004/005、FR-ENTRY-004。

use fleqi_application::context_service::ContextService;
use fleqi_application::dto::AppEvent;
use fleqi_application::paths::PathRegistry;
use fleqi_application::ports::{
    Clock, ContextPort, DirectoryPick, EventSink, IdGenerator, RawContext, RawPath, SessionStore,
    StorageError, TerminalEvent, TerminalHandle, TerminalPort,
};
use fleqi_application::session_service::{SessionGroup, SessionService};
use fleqi_application::surface_service::SurfaceService;
use fleqi_application::terminal_service::{SubmitOutcome, TerminalService};
use fleqi_domain::context::{ContextAvailability, PathKind};
use fleqi_domain::directory_sync::{ShellReadiness, SyncDecision};
use fleqi_domain::revision::Revision;
use fleqi_domain::session::SessionState;
use fleqi_domain::settings::{Activation, HideBehavior, Settings, Theme};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

// ---------- 测试基建 ----------

struct FakeClock;
impl Clock for FakeClock {
    fn now_rfc3339(&self) -> String {
        static TICK: AtomicU64 = AtomicU64::new(1);
        let n = TICK.fetch_add(1, Ordering::SeqCst);
        format!("2026-09-18T00:{:04}Z", n)
    }
}

#[derive(Default)]
struct Events(Mutex<Vec<String>>);
impl EventSink for Events {
    fn emit(&self, event: AppEvent) {
        self.0.lock().unwrap().push(event.name().to_owned());
    }
}
impl Events {
    fn count(&self, name: &str) -> usize {
        self.0.lock().unwrap().iter().filter(|n| *n == name).count()
    }
}

struct SeqIds(AtomicU64);
impl IdGenerator for SeqIds {
    fn next_id(&self, prefix: &str) -> String {
        format!("{prefix}-{}", self.0.fetch_add(1, Ordering::SeqCst))
    }
}

#[derive(Default)]
struct MemorySessions(Mutex<HashMap<String, fleqi_domain::session::Session>>);
impl SessionStore for MemorySessions {
    fn load_all(&self) -> Result<Vec<fleqi_domain::session::Session>, StorageError> {
        Ok(self.0.lock().unwrap().values().cloned().collect())
    }
    fn upsert(&self, session: &fleqi_domain::session::Session) -> Result<(), StorageError> {
        self.0
            .lock()
            .unwrap()
            .insert(session.id.clone(), session.clone());
        Ok(())
    }
    fn delete(&self, session_id: &str) -> Result<(), StorageError> {
        self.0.lock().unwrap().remove(session_id);
        Ok(())
    }
    fn append_entry(
        &self,
        _entry: &fleqi_domain::session::ConversationEntry,
    ) -> Result<(), StorageError> {
        Ok(())
    }
    fn entries(
        &self,
        _session_id: &str,
        _limit: usize,
        _before: Option<&str>,
    ) -> Result<Vec<fleqi_domain::session::ConversationEntry>, StorageError> {
        Ok(Vec::new())
    }
}

/// 脚本化终端：记录写入、可注入事件、可编程 readiness。
struct FakeHandle {
    id: String,
    written: Arc<Mutex<Vec<Vec<u8>>>>,
    readiness: Arc<Mutex<ShellReadiness>>,
    snapshot_cwd: Arc<Mutex<String>>,
    shutdown: Arc<Mutex<bool>>,
}

impl TerminalHandle for FakeHandle {
    fn terminal_id(&self) -> String {
        self.id.clone()
    }
    fn write_input(&self, bytes: &[u8]) -> Result<(), String> {
        self.written.lock().unwrap().push(bytes.to_vec());
        Ok(())
    }
    fn send_cd(&self, target: &Path, revision: Revision) -> Result<(), String> {
        self.written
            .lock()
            .unwrap()
            .push(format!("__cd__{}@{}\r", target.display(), revision).into_bytes());
        Ok(())
    }
    fn resize(&self, _c: u16, _r: u16) -> Result<(), String> {
        Ok(())
    }
    fn readiness(&self) -> ShellReadiness {
        *self.readiness.lock().unwrap()
    }
    fn foreground_process(&self) -> Option<String> {
        None
    }
    fn current_directory(&self) -> String {
        self.snapshot_cwd.lock().unwrap().clone()
    }
    fn snapshot(&self) -> fleqi_domain::terminal::TerminalSnapshot {
        fleqi_domain::terminal::TerminalSnapshot {
            terminal_id: self.id.clone(),
            session_id: "s".into(),
            state: fleqi_domain::terminal::TerminalState::Running,
            shell: "fake".into(),
            size: fleqi_domain::terminal::TerminalSize { cols: 80, rows: 24 },
            shell_readiness: fleqi_domain::terminal::ShellReadinessState::Ready,
            foreground_process: None,
            current_directory: self.current_directory(),
            pending_directory: None,
            directory_sync: fleqi_domain::directory_sync::DirectorySync::Synced,
            screen: String::new(),
            stream_cursor: "0".into(),
            exit_status: None,
            truncated: false,
        }
    }
    fn subscribe_from(&self, _cursor: u64, _sender: Sender<TerminalEvent>) -> Result<u64, String> {
        Ok(1)
    }
    fn unsubscribe(&self, _subscription: u64) -> Result<(), String> {
        Ok(())
    }
    fn has_exited(&self) -> bool {
        false
    }
    fn shutdown(self: Box<Self>) {
        *self.shutdown.lock().unwrap() = true;
    }
}

#[derive(Clone)]
struct FakeTerminalPort {
    written: Arc<Mutex<Vec<Vec<u8>>>>,
    readiness: Arc<Mutex<ShellReadiness>>,
    cwd: Arc<Mutex<String>>,
    shutdown: Arc<Mutex<bool>>,
    events_tx: Arc<Mutex<Vec<Sender<TerminalEvent>>>>,
    spawn_count: Arc<AtomicU64>,
}

impl TerminalPort for FakeTerminalPort {
    fn spawn(
        &self,
        session_id: &str,
        cwd: &Path,
        _cols: u16,
        _rows: u16,
        events: Sender<TerminalEvent>,
    ) -> Result<Box<dyn TerminalHandle>, String> {
        self.spawn_count.fetch_add(1, Ordering::SeqCst);
        *self.cwd.lock().unwrap() = cwd.to_string_lossy().into_owned();
        self.events_tx.lock().unwrap().push(events);
        Ok(Box::new(FakeHandle {
            id: format!("term-{session_id}"),
            written: Arc::clone(&self.written),
            readiness: Arc::clone(&self.readiness),
            snapshot_cwd: Arc::clone(&self.cwd),
            shutdown: Arc::clone(&self.shutdown),
        }))
    }
}

impl FakeTerminalPort {
    fn emit(&self, event: TerminalEvent) {
        for tx in self.events_tx.lock().unwrap().iter() {
            let _ = tx.send(event.clone());
        }
    }
}

struct FixedContext(RawContext);
impl ContextPort for FixedContext {
    fn capture(&self) -> RawContext {
        self.0.clone()
    }
    fn pick_directory(&self) -> DirectoryPick {
        DirectoryPick::Cancelled
    }
}

fn safe() -> ShellReadiness {
    ShellReadiness {
        prompt_ready: true,
        edit_line_empty: true,
        foreground_is_shell: true,
        delivering: false,
    }
}

struct Rig {
    sessions: Arc<SessionService>,
    terminal: Arc<TerminalService>,
    surface: Arc<SurfaceService>,
    context: Arc<ContextService>,
    port: FakeTerminalPort,
    paths: Arc<PathRegistry>,
    events: Arc<Events>,
}

impl Rig {
    /// 刷新上下文并按宿主方式把事件路由给 SurfaceService（含自动显示/目录同步）。
    fn refresh(&self) -> fleqi_domain::context::ContextSnapshot {
        let snapshot = self.context.refresh();
        self.surface.on_context_changed(&snapshot);
        snapshot
    }
}

fn rig(settings: &Settings, directory: &Path) -> Rig {
    let events: Arc<dyn EventSink> = Arc::new(Events::default());
    let clock: Arc<dyn Clock> = Arc::new(FakeClock);
    let ids: Arc<dyn IdGenerator> = Arc::new(SeqIds(AtomicU64::new(1)));
    let store: Arc<dyn SessionStore> = Arc::new(MemorySessions::default());
    let sessions =
        Arc::new(SessionService::load(store, clock.clone(), ids.clone(), events.clone()).unwrap());
    let port = FakeTerminalPort {
        written: Arc::new(Mutex::new(Vec::new())),
        readiness: Arc::new(Mutex::new(safe())),
        cwd: Arc::new(Mutex::new(directory.to_string_lossy().into_owned())),
        shutdown: Arc::new(Mutex::new(false)),
        events_tx: Arc::new(Mutex::new(Vec::new())),
        spawn_count: Arc::new(AtomicU64::new(0)),
    };
    let registry = Arc::new(PathRegistry::new());
    let terminal = TerminalService::new(
        Arc::new(port.clone()),
        sessions.clone(),
        Arc::clone(&registry),
        clock,
        ids,
        events.clone(),
        Arc::new(fleqi_application::ports::ThreadSpawner),
    );
    let raw = RawContext {
        directory: Some(RawPath {
            native: directory.to_path_buf(),
            kind: PathKind::Directory,
        }),
        view_kind: Some(fleqi_domain::context::ViewKind::Physical),
        ..RawContext::default()
    };
    let context = Arc::new(ContextService::new(
        Arc::new(FixedContext(raw)),
        Arc::new(FakeClock),
        events.clone(),
        Arc::clone(&registry),
    ));
    let surface = SurfaceService::new(
        settings,
        true,
        fleqi_application::surface_service::SurfaceDeps {
            sessions: sessions.clone(),
            terminal: terminal.clone(),
            context: Arc::clone(&context),
            paths: Arc::clone(&registry),
            events: events.clone(),
        },
    );
    Rig {
        sessions,
        terminal,
        surface,
        context,
        port,
        paths: registry,
        events: Arc::new(Events::default()),
    }
}

fn written_texts(port: &FakeTerminalPort) -> Vec<String> {
    port.written
        .lock()
        .unwrap()
        .iter()
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .collect()
}

// ---------- 用例 ----------

#[test]
fn show_creates_session_without_terminal_until_first_use() {
    let settings = Settings {
        activation: Activation::FollowFinder,
        ..Settings::default()
    };
    let dir = tempfile::tempdir().unwrap();
    let rig = rig(&settings, dir.path());
    assert_eq!(
        rig.port.spawn_count.load(Ordering::SeqCst),
        0,
        "纯 AI 会话不创建 PTY"
    );

    rig.refresh();
    assert_eq!(
        rig.surface.visibility(),
        fleqi_domain::surface::Visibility::Visible,
        "followFinder 首个有效目录自动显示"
    );
    assert_eq!(rig.sessions.active_count(), 1, "自动显示创建会话");
    assert_eq!(
        rig.port.spawn_count.load(Ordering::SeqCst),
        0,
        "显示不等于启动 shell（FR-CTX-010）"
    );

    // 首次 ! 提交按需创建 PTY 并发送。
    let session_id = rig.surface.visible_session().unwrap().id;
    let outcome = rig
        .terminal
        .submit_line(
            "r1",
            &session_id,
            "ls",
            Revision::new(1),
            dir.path().to_str().unwrap(),
            Some(dir.path().to_path_buf()),
        )
        .unwrap();
    assert!(matches!(outcome, SubmitOutcome::Sent));
    assert_eq!(rig.port.spawn_count.load(Ordering::SeqCst), 1);
    assert!(written_texts(&rig.port).iter().any(|t| t.contains("ls\r")));
}

#[test]
fn queued_line_waits_for_sync_and_is_withdrawn_on_new_target() {
    let settings = Settings {
        activation: Activation::FollowFinder,
        ..Settings::default()
    };
    let base = tempfile::tempdir().unwrap();
    let rig = rig(&settings, base.path());
    rig.refresh();
    let session_id = rig.surface.visible_session().unwrap().id;
    rig.terminal.open(&session_id, None, 80, 24).unwrap();

    // 目标改为尚不安全的新目录 B。
    *rig.port.readiness.lock().unwrap() = ShellReadiness {
        prompt_ready: false,
        ..safe()
    };
    let target_b = base.path().join("B");
    std::fs::create_dir(&target_b).unwrap();
    rig.terminal
        .target_changed(&session_id, &target_b, "B", Revision::new(2))
        .unwrap();
    assert!(
        !written_texts(&rig.port)
            .iter()
            .any(|t| t.starts_with("__cd__")),
        "目标变化但 shell 忙：不应发 cd（FR-CTX-003）"
    );
    let outcome = rig
        .terminal
        .submit_line(
            "r2",
            &session_id,
            "make me",
            Revision::new(2),
            "B",
            Some(target_b.clone()),
        )
        .unwrap();
    assert!(matches!(outcome, SubmitOutcome::Queued { .. }));

    // Finder 再换 C：撤销自动投递，草稿可取回。
    let target_c = base.path().join("C");
    std::fs::create_dir(&target_c).unwrap();
    rig.terminal
        .target_changed(&session_id, &target_c, "C", Revision::new(3))
        .unwrap();
    let withdrawn = rig.terminal.take_withdrawn(&session_id);
    assert_eq!(withdrawn.map(|l| l.text), Some("make me".into()));
    assert!(
        !written_texts(&rig.port)
            .iter()
            .any(|t| t.contains("make me"))
    );
}

#[test]
fn user_hide_withdraws_queued_line_for_visible_session() {
    // AC-FLOW-015：主动隐藏时撤销未投递的排队命令并保留草稿（FR-SESSION-004/FR-CTX-002）。
    let settings = Settings {
        activation: Activation::FollowFinder,
        ..Settings::default()
    };
    let base = tempfile::tempdir().unwrap();
    let rig = rig(&settings, base.path());
    rig.refresh();
    let session_id = rig.surface.visible_session().unwrap().id;
    rig.terminal.open(&session_id, None, 80, 24).unwrap();

    *rig.port.readiness.lock().unwrap() = ShellReadiness {
        prompt_ready: false,
        ..safe()
    };
    let target_b = base.path().join("B");
    std::fs::create_dir(&target_b).unwrap();
    rig.terminal
        .target_changed(&session_id, &target_b, "B", Revision::new(2))
        .unwrap();
    let outcome = rig
        .terminal
        .submit_line(
            "r-hide",
            &session_id,
            "make me",
            Revision::new(2),
            "B",
            Some(target_b.clone()),
        )
        .unwrap();
    assert!(matches!(outcome, SubmitOutcome::Queued { .. }));

    rig.surface.user_hide().unwrap();
    let withdrawn = rig.terminal.take_withdrawn(&session_id);
    assert_eq!(
        withdrawn.map(|l| l.text),
        Some("make me".into()),
        "主动隐藏必须把可见会话的在途排队命令转为草稿"
    );
    assert!(
        !written_texts(&rig.port)
            .iter()
            .any(|t| t.contains("make me")),
        "被撤销的命令不得投递"
    );
}

#[test]
fn finder_drag_temporarily_hides_pauses_delivery_and_restores() {
    // M2 收口（FR-SESSION-006 语义端）：Finder 拖动 → system_hide 暂隐并暂停
    // 目录投递；拖动结束 → system_restore 恢复可见与投递。
    let settings = Settings {
        activation: Activation::FollowFinder,
        ..Settings::default()
    };
    let base = tempfile::tempdir().unwrap();
    let rig = rig(&settings, base.path());
    rig.refresh();
    let session_id = rig.surface.visible_session().unwrap().id;
    rig.terminal.open(&session_id, None, 80, 24).unwrap();
    *rig.port.readiness.lock().unwrap() = safe();

    // 暂隐：可见性 → temporarilyHidden；此时对另一目标的提交只能排队（不能立即投递）。
    rig.surface.system_hide();
    assert_eq!(
        rig.surface.visibility(),
        fleqi_domain::surface::Visibility::TemporarilyHidden
    );
    let outcome = rig
        .terminal
        .submit_line(
            "r-drag",
            &session_id,
            "echo during-drag",
            Revision::new(1),
            "/somewhere-else",
            None,
        )
        .unwrap();
    assert!(
        matches!(outcome, SubmitOutcome::Queued { .. }),
        "暂隐期间不得直接投递：{outcome:?}"
    );

    // 恢复：回到 visible；恢复时的目录重同步把异目标排队命令撤销为草稿
    //（FR-CTX-002 目标变化撤销自动投递保留草稿）。
    rig.surface.system_restore();
    assert_eq!(
        rig.surface.visibility(),
        fleqi_domain::surface::Visibility::Visible
    );
    let withdrawn = rig.terminal.take_withdrawn(&session_id);
    assert_eq!(
        withdrawn.map(|l| l.text),
        Some("echo during-drag".into()),
        "恢复重同步应把暂隐期排队命令转为草稿"
    );
}

#[test]
fn user_end_marks_session_ended_even_when_shell_exits_nonzero() {
    // FR-SESSION-005：主动结束（session_end/endAll）时 PTY 以信号码退出
    // （如 SIGHUP=129）是预期行为，会话必须落 ended 而非 failed。
    let settings = Settings {
        activation: Activation::FollowFinder,
        ..Settings::default()
    };
    let base = tempfile::tempdir().unwrap();
    let rig = rig(&settings, base.path());
    rig.refresh();
    let session_id = rig.surface.visible_session().unwrap().id;
    rig.terminal.open(&session_id, None, 80, 24).unwrap();

    rig.terminal.end_session(&session_id);
    rig.port.emit(TerminalEvent::Exited { status: Some(129) });
    std::thread::sleep(std::time::Duration::from_millis(80));
    let session = rig.sessions.get(&session_id).unwrap();
    assert_eq!(
        session.state,
        fleqi_domain::session::SessionState::Ended,
        "主动结束后的非零退出码不构成 failed"
    );
}

#[test]
fn unexpected_shell_exit_marks_session_failed() {
    // 对照：未经用户结束的意外退出（status != 0）仍标 failed。
    let settings = Settings {
        activation: Activation::FollowFinder,
        ..Settings::default()
    };
    let base = tempfile::tempdir().unwrap();
    let rig = rig(&settings, base.path());
    rig.refresh();
    let session_id = rig.surface.visible_session().unwrap().id;
    rig.terminal.open(&session_id, None, 80, 24).unwrap();

    rig.port.emit(TerminalEvent::Exited { status: Some(1) });
    std::thread::sleep(std::time::Duration::from_millis(80));
    let session = rig.sessions.get(&session_id).unwrap();
    assert_eq!(
        session.state,
        fleqi_domain::session::SessionState::Failed,
        "意外退出仍应标记 failed"
    );
}

#[test]
fn cd_confirm_delivers_queued_line_once() {
    let settings = Settings {
        activation: Activation::FollowFinder,
        ..Settings::default()
    };
    let base = tempfile::tempdir().unwrap();
    let rig = rig(&settings, base.path());
    rig.refresh();
    let session_id = rig.surface.visible_session().unwrap().id;
    rig.terminal.open(&session_id, None, 80, 24).unwrap();

    *rig.port.readiness.lock().unwrap() = ShellReadiness {
        prompt_ready: false,
        ..safe()
    };
    let target = base.path().join("T");
    std::fs::create_dir(&target).unwrap();
    rig.terminal
        .target_changed(&session_id, &target, "T", Revision::new(1))
        .unwrap();
    rig.terminal
        .submit_line(
            "r3",
            &session_id,
            "echo after-sync",
            Revision::new(1),
            "T",
            Some(target.clone()),
        )
        .unwrap();

    // shell 到达安全提示符 → 发 cd；回执确认 → 自动发送一次排队命令。
    *rig.port.readiness.lock().unwrap() = safe();
    *rig.port.cwd.lock().unwrap() = target
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    rig.port.emit(TerminalEvent::PromptReady {
        cwd: rig.port.cwd.lock().unwrap().clone(),
    });
    std::thread::sleep(std::time::Duration::from_millis(50));
    rig.port.emit(TerminalEvent::CdResult {
        revision: Revision::new(1),
        ok: true,
        cwd: rig.port.cwd.lock().unwrap().clone(),
        message: None,
    });
    std::thread::sleep(std::time::Duration::from_millis(50));
    let texts = written_texts(&rig.port);
    assert!(texts.iter().any(|t| t.starts_with("__cd__")));
    assert_eq!(
        texts
            .iter()
            .filter(|t| t.contains("echo after-sync"))
            .count(),
        1,
        "排队命令只发送一次"
    );
    let session = rig.sessions.get(&session_id).unwrap();
    assert_eq!(
        session.directory_sync,
        fleqi_domain::directory_sync::DirectorySync::Synced
    );
}

#[test]
fn input_requires_lease_and_background_cancels_pending() {
    let settings = Settings::default();
    let dir = tempfile::tempdir().unwrap();
    let rig = rig(&settings, dir.path());
    let snapshot = rig.context.refresh();
    rig.sessions.create(Some(&snapshot), None).unwrap();
    let session_id = rig.sessions.list(SessionGroup::Active, 0, 10)[0].id.clone();
    rig.terminal.open(&session_id, None, 80, 24).unwrap();

    assert!(
        rig.terminal.input(&session_id, None, b"ls\r").is_err(),
        "无租约输入被拒绝"
    );
    let lease = rig.terminal.acquire_lease(&session_id, "composer").unwrap();
    rig.terminal
        .input(&session_id, Some(&lease), b"ls\r")
        .unwrap();
    assert!(
        rig.terminal
            .input(&session_id, Some("lease-other"), b"x")
            .is_err(),
        "他人租约被拒绝"
    );
    let replacement = rig
        .terminal
        .acquire_lease(&session_id, "second-panel")
        .unwrap();
    assert_ne!(lease, replacement, "固定时钟下租约也必须唯一");
    rig.terminal.release_lease(&session_id, &lease);
    assert!(
        rig.terminal
            .input(&session_id, Some(&replacement), b"x")
            .is_ok()
    );
    assert!(rig.terminal.input(&session_id, Some(&lease), b"x").is_err());
    rig.terminal.release_lease(&session_id, &replacement);
    assert!(
        rig.terminal
            .input(&session_id, Some("fabricated"), b"x")
            .is_err()
    );

    // 排队后切后台：撤销并保留草稿（FR-CTX-002）。
    *rig.port.readiness.lock().unwrap() = ShellReadiness {
        prompt_ready: false,
        ..safe()
    };
    let target = dir.path().join("Z");
    std::fs::create_dir(&target).unwrap();
    rig.terminal
        .target_changed(&session_id, &target, "Z", Revision::new(1))
        .unwrap();
    rig.terminal
        .submit_line(
            "r4",
            &session_id,
            "queued-then-background",
            Revision::new(1),
            "Z",
            Some(target),
        )
        .unwrap();
    rig.terminal.set_visibility(&session_id, false, true);
    let withdrawn = rig.terminal.take_withdrawn(&session_id);
    assert_eq!(
        withdrawn.map(|l| l.text),
        Some("queued-then-background".into())
    );
    assert!(rig.terminal.take_withdrawn(&session_id).is_none());
}

#[test]
fn keep_all_hide_preserves_and_end_all_ends_sessions() {
    let keep = Settings {
        activation: Activation::FollowFinder,
        ..Settings::default()
    };
    let dir = tempfile::tempdir().unwrap();
    let rig = rig(&keep, dir.path());
    rig.refresh();
    let a = rig.surface.visible_session().unwrap().id;
    rig.terminal.open(&a, None, 80, 24).unwrap();
    rig.surface.user_hide().unwrap();
    assert_eq!(
        rig.sessions.get(&a).unwrap().state,
        SessionState::Active,
        "keepAll 隐藏保留会话"
    );
    // 再显式显示：新建会话（FR-SESSION-002）。
    rig.surface.user_show().unwrap();
    let b = rig.surface.visible_session().unwrap().id;
    assert_ne!(a, b);

    let mut end_all = keep.clone();
    end_all.hide_behavior = HideBehavior::EndAll;
    rig.surface.set_settings(&end_all);
    rig.surface.user_hide().unwrap();
    assert_eq!(rig.sessions.active_count(), 0, "endAll 结束全部会话");
    assert!(*rig.port.shutdown.lock().unwrap(), "PTY 已回收");
    assert!(
        rig.sessions.list(SessionGroup::History, 0, 10).len() >= 2,
        "历史保留"
    );
}

#[test]
fn session_limit_and_continue_from_history() {
    let settings = Settings::default();
    let dir = tempfile::tempdir().unwrap();
    let rig = rig(&settings, dir.path());
    for _ in 0..16 {
        rig.sessions.create(None, None).unwrap();
    }
    let error = rig.sessions.create(None, None).unwrap_err();
    assert!(error.message.contains("上限"), "{}", error.message);
    let first = rig.sessions.list(SessionGroup::Active, 0, 1)[0].id.clone();
    rig.sessions.begin_end(&first).unwrap();
    rig.sessions.mark_ended(&first, false).unwrap();
    assert_eq!(rig.sessions.get(&first).unwrap().state, SessionState::Ended);
    let resumed = rig.sessions.continue_from(&first, None).unwrap();
    assert_ne!(resumed.id, first);
    assert_eq!(resumed.parent_session_id.as_deref(), Some(first.as_str()));
    // 历史只读：删除已结束会话不删用户文件；删除活动会话先要求结束。
    let error = rig.sessions.delete(&resumed.id, None).unwrap_err();
    assert_eq!(error.code, fleqi_application::ErrorCode::Conflict);
    rig.sessions.begin_end(&resumed.id).unwrap();
    rig.sessions.mark_ended(&resumed.id, false).unwrap();
    rig.sessions.delete(&resumed.id, None).unwrap();
    assert!(rig.sessions.get(&resumed.id).is_err());
}

#[test]
fn restart_marks_active_sessions_interrupted() {
    let store: Arc<MemorySessions> = Arc::new(MemorySessions::default());
    let events: Arc<dyn EventSink> = Arc::new(Events::default());
    let mut session = rig(&Settings::default(), Path::new("/tmp"))
        .sessions
        .create(None, None)
        .unwrap();
    session.terminal_id = Some("term-x".into());
    store.upsert(&session).unwrap();
    session.id = "second".into();
    store.upsert(&session).unwrap();
    let reloaded = SessionService::load(
        store,
        Arc::new(FakeClock),
        Arc::new(SeqIds(AtomicU64::new(1))),
        events,
    )
    .unwrap();
    for s in reloaded.list(SessionGroup::All, 0, 10) {
        assert_eq!(s.state, SessionState::Interrupted, "{}", s.id);
        assert!(s.terminal_id.is_none(), "不复活进程句柄");
    }
}

#[test]
fn theme_setting_flow_does_not_affect_surface() {
    let settings = Settings {
        activation: Activation::FollowFinder,
        theme: Theme::Light,
        ..Settings::default()
    };
    let dir = tempfile::tempdir().unwrap();
    let rig = rig(&settings, dir.path());
    rig.refresh();
    rig.surface.user_hide().unwrap();
    let same_theme = { settings.clone() };
    rig.surface.set_settings(&same_theme);
    assert_eq!(
        rig.surface.visibility(),
        fleqi_domain::surface::Visibility::UserHidden,
        "改主题不解除抑制"
    );
    let mut manual = settings.clone();
    manual.activation = Activation::Manual;
    rig.surface.set_settings(&manual);
    assert_eq!(
        rig.surface.visibility(),
        fleqi_domain::surface::Visibility::UserHidden
    );
    assert!(!matches!(
        rig.surface.visibility(),
        fleqi_domain::surface::Visibility::Visible
    ));
    let _ = SyncDecision::Pending {
        target: "unused".into(),
    };
    let _ = ContextAvailability::Available;
    void(rig.events.count("session:changed"));
    void(&rig.paths);
}

fn void<T>(_value: T) {}
