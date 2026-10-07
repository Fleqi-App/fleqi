//! TerminalService（architecture.md §5.2、§6；FR-CTX/FR-TERM）：按需创建 PTY、输入租约、
//! queuedLine 与目录同步编排。TerminalManager 的 Output 直接经订阅游标转发（Channel）；
//! 本服务只处理低频控制事件并驱动 SyncMachine。

use fleqi_domain::context::ContextSnapshot;
use fleqi_domain::directory_sync::{
    DirectorySync, QueuedLine, SyncDecision, SyncMachine, SyncOutcome,
};
use fleqi_domain::revision::Revision;
use fleqi_domain::session::EntryRole;
use fleqi_domain::terminal::TerminalSnapshot;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use crate::dto::{AppError, AppEvent, AppResult};
use crate::paths::PathRegistry;
use crate::ports::{Clock, EventSink, IdGenerator, TerminalEvent, TerminalHandle, TerminalPort};
use crate::session_service::SessionService;

struct Entry {
    handle: Box<dyn TerminalHandle>,
    sync: SyncMachine,
    /// 可见会话才投递目录控制；temporarilyHidden 只暂停投递。
    delivery_visible: bool,
    lease: Option<(String, String)>,
    revision: u64,
    /// 消费方已回执的流位置（terminal_ack；流控/诊断用）。
    acked_cursor: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmitOutcome {
    Sent,
    Queued { queued_behind_sync: bool },
}

pub struct TerminalService {
    port: Arc<dyn TerminalPort>,
    sessions: Arc<SessionService>,
    paths: Arc<PathRegistry>,
    ids: Arc<dyn IdGenerator>,
    events: Arc<dyn EventSink>,
    spawner: Arc<dyn crate::ports::Spawner>,
    inner: Mutex<HashMap<String, Entry>>,
    withdrawn: Mutex<HashMap<String, QueuedLine>>,
    /// 用户主动结束的会话：其 PTY 退出码（如 SIGHUP 的非零值）不构成 failed。
    expecting_exit: Mutex<HashMap<String, bool>>,
}

impl TerminalService {
    pub fn new(
        port: Arc<dyn TerminalPort>,
        sessions: Arc<SessionService>,
        paths: Arc<PathRegistry>,
        _clock: Arc<dyn Clock>,
        ids: Arc<dyn IdGenerator>,
        events: Arc<dyn EventSink>,
        spawner: Arc<dyn crate::ports::Spawner>,
    ) -> Arc<Self> {
        Arc::new(Self {
            port,
            sessions,
            paths,
            ids,
            events,
            spawner,
            inner: Mutex::new(HashMap::new()),
            withdrawn: Mutex::new(HashMap::new()),
            expecting_exit: Mutex::new(HashMap::new()),
        })
    }

    /// 按需创建 PTY：需要有效目录（FR-CTX-010，不以用户主目录替代）。
    pub fn open(
        self: &Arc<Self>,
        session_id: &str,
        context: Option<&ContextSnapshot>,
        cols: u16,
        rows: u16,
    ) -> AppResult<String> {
        if let Some(entry) = self.inner.lock().expect("terminals").get(session_id) {
            return Ok(entry.handle.terminal_id());
        }
        let session = self.sessions.get(session_id)?;
        if !session.state.is_active() {
            return Err(AppError::conflict("会话已结束，不能打开终端", None));
        }
        let directory = context
            .and_then(|c| c.directory_ref.as_ref())
            .and_then(|d| self.paths.resolve(&d.id))
            .or_else(|| session.initial_directory.as_ref().map(PathBuf::from))
            .or_else(|| session.current_directory.as_ref().map(PathBuf::from));
        let Some(cwd) = directory.filter(|d| d.is_dir()) else {
            return Err(AppError::validation(vec![
                fleqi_domain::settings::FieldError {
                    field: "directory".into(),
                    code: "invalid".into(),
                    message: "没有有效工作目录，请先选择文件夹".into(),
                },
            ]));
        };
        let (tx, rx) = channel::<TerminalEvent>();
        let handle = self
            .port
            .spawn(session_id, &cwd, cols, rows, tx)
            .map_err(AppError::internal)?;
        let terminal_id = handle.terminal_id();
        let cwd_display = cwd.to_string_lossy().into_owned();
        let sync = SyncMachine::new(cwd_display.clone());
        self.sessions
            .set_terminal(session_id, Some(terminal_id.clone()))?;
        self.sessions.set_directories(
            session_id,
            Some(cwd_display.clone()),
            Some(cwd_display),
            DirectorySync::Synced,
        )?;
        self.inner.lock().expect("terminals").insert(
            session_id.to_owned(),
            Entry {
                handle,
                sync,
                delivery_visible: true,
                lease: None,
                revision: 1,
                acked_cursor: 0,
            },
        );
        self.emit_changed(session_id);
        self.start_pump(session_id.to_owned(), rx);
        Ok(terminal_id)
    }

    fn start_pump(self: &Arc<Self>, session_id: String, receiver: Receiver<TerminalEvent>) {
        let service = Arc::clone(self);
        self.spawner.spawn(Box::new(move || {
            for event in receiver {
                if let TerminalEvent::Output { .. } = event {
                    continue;
                }
                service.handle_control(&session_id, event);
            }
        }));
    }

    fn handle_control(self: &Arc<Self>, session_id: &str, event: TerminalEvent) {
        match event {
            TerminalEvent::PromptReady { cwd } => {
                self.with_entry(session_id, |entry, _| {
                    entry.sync.manual_cwd_changed(&cwd);
                    if entry.sync.pending_target().is_some()
                        && entry.sync.state() == DirectorySync::Pending
                    {
                        let readiness = entry.handle.readiness();
                        if let Some(SyncDecision::SendCd { target, revision }) = entry
                            .sync
                            .shell_became_safe(&readiness, entry.delivery_visible)
                        {
                            let _ = Self::deliver_cd(entry, Path::new(&target), revision);
                        }
                    }
                });
                self.publish_directory_state(session_id);
                self.emit_changed(session_id);
            }
            TerminalEvent::CdResult {
                revision,
                ok,
                cwd,
                message,
            } => {
                let outcome = if ok {
                    SyncOutcome::Confirmed { cwd: cwd.clone() }
                } else {
                    SyncOutcome::Failed {
                        cwd: cwd.clone(),
                        message: message.clone().unwrap_or_else(|| "无法切换目录".into()),
                    }
                };
                let decision = self
                    .with_entry(session_id, |entry, _| {
                        let decision = entry.sync.on_cd_result(revision, outcome);
                        if let Some(SyncDecision::SyncedAndSend { line, .. }) = &decision {
                            let _ = entry
                                .handle
                                .write_input(format!("{}\r", line.text).as_bytes());
                        }
                        decision
                    })
                    .flatten();
                match decision {
                    Some(SyncDecision::SyncedAndSend { line, .. }) => {
                        let _ = self.sessions.append_entry(
                            session_id,
                            EntryRole::ManualCommand,
                            &line.text,
                            None,
                        );
                    }
                    Some(SyncDecision::Failed { message, .. }) => {
                        self.withdrawn(session_id, &line_withdraw_message(&message));
                    }
                    _ => {}
                }
                self.publish_directory_state(session_id);
                self.emit_changed(session_id);
            }
            TerminalEvent::CdCancelled { revision } => {
                self.with_entry(session_id, |entry, _| {
                    entry.sync.on_cd_cancelled(revision);
                });
                self.publish_directory_state(session_id);
                self.emit_changed(session_id);
            }
            TerminalEvent::Preexec | TerminalEvent::EditLine { .. } => {
                self.emit_changed(session_id);
            }
            TerminalEvent::Exited { status } => {
                // 用户主动结束时 PTY 往往以信号退出码终止（如 SIGHUP=129）；
                // 这不是会话失败，只有意外退出才标 failed（FR-SESSION-005）。
                let intentional = self
                    .expecting_exit
                    .lock()
                    .expect("expecting exit")
                    .remove(session_id)
                    .unwrap_or(false);
                self.shutdown_entry(session_id, true);
                let _ = self
                    .sessions
                    .mark_ended(session_id, !intentional && status != Some(0));
                self.events.emit(AppEvent::SessionChanged {
                    session_id: session_id.to_owned(),
                    revision: Revision::new(0),
                    deleted: true,
                });
            }
            TerminalEvent::Output { .. } => unreachable!(),
        }
    }

    fn publish_directory_state(&self, session_id: &str) {
        if let Some((cwd, target, sync)) = self.with_entry(session_id, |entry, _| {
            (
                entry.sync.current().to_owned(),
                entry.sync.pending_target().map(str::to_owned),
                entry.sync.state(),
            )
        }) {
            let _ = self
                .sessions
                .set_directories(session_id, Some(cwd), target, sync);
        }
    }

    /// 状态决策与实际投递持有同一会话锁，隐藏或新目标不能插入两者之间。
    fn deliver_cd(entry: &mut Entry, target: &Path, revision: Revision) -> AppResult<()> {
        let result = entry.handle.send_cd(target, revision);
        if let Err(message) = &result {
            entry.sync.on_cd_result(
                revision,
                SyncOutcome::Failed {
                    cwd: entry.handle.current_directory(),
                    message: message.clone(),
                },
            );
        }
        result.map_err(AppError::internal)
    }

    /// 每个会话至多一个输入租约；控制台与气泡同时查看时未持有租约的面板只读。
    pub fn acquire_lease(&self, session_id: &str, owner: &str) -> AppResult<String> {
        let mut inner = self.inner.lock().expect("terminals");
        let entry = inner
            .get_mut(session_id)
            .ok_or_else(|| AppError::not_found("终端未启动"))?;
        let lease = self.ids.next_id("lease");
        entry.lease = Some((lease.clone(), owner.to_owned()));
        Ok(lease)
    }

    pub fn release_lease(&self, session_id: &str, lease: &str) {
        if let Some(entry) = self.inner.lock().expect("terminals").get_mut(session_id)
            && entry.lease.as_ref().is_some_and(|(held, _)| held == lease)
        {
            entry.lease = None;
        }
    }

    pub fn input(&self, session_id: &str, lease: Option<&str>, bytes: &[u8]) -> AppResult<()> {
        let mut inner = self.inner.lock().expect("terminals");
        let entry = inner
            .get_mut(session_id)
            .ok_or_else(|| AppError::not_found("终端未启动"))?;
        match (&entry.lease, lease) {
            (Some((held, _)), Some(given)) if held == given => {}
            _ => return Err(AppError::forbidden("终端输入需要当前输入租约")),
        }
        entry.handle.write_input(bytes).map_err(AppError::internal)
    }

    pub fn resize(&self, session_id: &str, cols: u16, rows: u16) -> AppResult<()> {
        let inner = self.inner.lock().expect("terminals");
        let entry = inner
            .get(session_id)
            .ok_or_else(|| AppError::not_found("终端未启动"))?;
        entry.handle.resize(cols, rows).map_err(AppError::internal)
    }

    /// 消费位点回执（architecture.md §12.5 terminal_ack）：记录消费方已确认的
    /// 流位置。投递侧内存由有界环 + 分段持久化硬上限约束，回执用于流控与诊断，
    /// 不改变重连/保留合同。
    pub fn ack(&self, session_id: &str, cursor: u64) -> AppResult<u64> {
        let mut inner = self.inner.lock().expect("terminals");
        let entry = inner
            .get_mut(session_id)
            .ok_or_else(|| AppError::not_found("终端未启动"))?;
        if cursor > entry.acked_cursor {
            entry.acked_cursor = cursor;
        }
        Ok(entry.acked_cursor)
    }

    /// 手动 `!` 行：目标目录已同步且一致 → 立即发送；否则排队等待（可撤销）。
    pub fn submit_line(
        self: &Arc<Self>,
        request_id: &str,
        session_id: &str,
        text: &str,
        context_revision: Revision,
        target_display: &str,
        target_native: Option<PathBuf>,
    ) -> AppResult<SubmitOutcome> {
        let session = self.sessions.get(session_id)?;
        if !session.state.is_active() {
            return Err(AppError::conflict("会话已结束", None));
        }
        // 锁内只做判定与状态变更；副作用（发送/持久化/广播）出锁后执行，避免重入死锁。
        enum Step {
            OpenTerminal,
            Send,
            Queue { state: DirectorySync },
            Conflict(String),
        }
        let step = {
            let mut inner = self.inner.lock().expect("terminals");
            if !inner.contains_key(session_id) {
                Step::OpenTerminal
            } else {
                let entry = inner.get_mut(session_id).expect("已核对存在");
                let snapshot = entry.handle.snapshot();
                if snapshot.shell == "/bin/bash"
                    && snapshot.shell_readiness
                        == fleqi_domain::terminal::ShellReadinessState::Unknown
                {
                    return Err(AppError::unavailable(
                        "Bash 集成尚未就绪或已失效，请在终端面板执行命令",
                    ));
                }
                let target = target_native
                    .clone()
                    .unwrap_or_else(|| PathBuf::from(target_display));
                let already_there = match (
                    std::path::Path::new(entry.sync.current()).canonicalize(),
                    target.canonicalize(),
                ) {
                    (Ok(current), Ok(target)) => current == target,
                    _ => false,
                };
                if entry.sync.state() == DirectorySync::Synced
                    && already_there
                    && entry.sync.queued_line().is_none()
                {
                    match entry.handle.write_input(format!("{text}\r").as_bytes()) {
                        Ok(()) => Step::Send,
                        Err(error) => return Err(AppError::internal(error)),
                    }
                } else {
                    let line = QueuedLine {
                        request_id: request_id.to_owned(),
                        session_id: session_id.to_owned(),
                        context_revision,
                        target: target_display.to_owned(),
                        text: text.to_owned(),
                    };
                    match entry.sync.queue_line(line) {
                        Ok(()) => Step::Queue {
                            state: entry.sync.state(),
                        },
                        Err(existing) => Step::Conflict(existing.text),
                    }
                }
            }
        };
        match step {
            Step::OpenTerminal => {
                // 终端未启动：先创建（首次提交 ! 时按需创建）。
                self.open(session_id, None, 100, 30)?;
                self.submit_line(
                    request_id,
                    session_id,
                    text,
                    context_revision,
                    target_display,
                    target_native,
                )
            }
            Step::Send => {
                self.sessions
                    .append_entry(session_id, EntryRole::ManualCommand, text, None)?;
                Ok(SubmitOutcome::Sent)
            }
            Step::Queue { state } => {
                let _ = self.sessions.set_directories(
                    session_id,
                    None,
                    Some(target_display.to_owned()),
                    state,
                );
                self.emit_changed(session_id);
                Ok(SubmitOutcome::Queued {
                    queued_behind_sync: true,
                })
            }
            Step::Conflict(existing) => Err(AppError::conflict(
                format!("已有等待发送的命令（{existing}），请先取消"),
                None,
            )),
        }
    }

    pub fn cancel_queued(&self, session_id: &str) -> Option<QueuedLine> {
        let line = self
            .with_entry(session_id, |entry, _| entry.sync.cancel_queued())
            .flatten();
        if line.is_some() {
            self.emit_changed(session_id);
        }
        line
    }

    /// Finder 目标变化（仅当前可见会话由调用方传入）。
    pub fn target_changed(
        &self,
        session_id: &str,
        target_native: &std::path::Path,
        _target_display: &str,
        revision: Revision,
    ) -> AppResult<()> {
        self.change_target(session_id, target_native, revision, false)
    }

    /// 先校验恢复时的最新目录，再允许执行器继续投递，避免恢复窗口中的旧请求抢跑。
    pub fn resume_directory(
        &self,
        session_id: &str,
        target: &Path,
        revision: Revision,
    ) -> AppResult<()> {
        self.change_target(session_id, target, revision, true)
    }

    fn change_target(
        &self,
        session_id: &str,
        target_native: &Path,
        revision: Revision,
        resume: bool,
    ) -> AppResult<()> {
        let result = self.with_entry(session_id, |entry, service| {
            if resume {
                entry.delivery_visible = true;
            }
            // 只归一化比较；已经处于同一目录时，不因符号链接或路径拼写重复切换。
            let target = match (
                Path::new(entry.sync.current()).canonicalize(),
                target_native.canonicalize(),
            ) {
                (Ok(current), Ok(target)) if current == target => entry.sync.current().to_owned(),
                _ => target_native.to_string_lossy().into_owned(),
            };
            if entry
                .sync
                .pending_target()
                .is_some_and(|previous| previous != target)
            {
                entry.handle.cancel_cd();
            }
            let readiness = entry.handle.readiness();
            let decision =
                entry
                    .sync
                    .target_changed(&target, revision, &readiness, entry.delivery_visible);
            // 目标变化撤销的等待命令转移到服务级草稿（UI 可取回提示重新提交）。
            if let Some(line) = entry.sync.take_withdrawn_line() {
                service
                    .withdrawn
                    .lock()
                    .expect("withdrawn")
                    .insert(session_id.to_owned(), line);
            }
            if resume {
                entry.handle.set_directory_visibility(true, false);
            }
            if let SyncDecision::SendCd { target, revision } = decision {
                Self::deliver_cd(entry, Path::new(&target), revision)
            } else {
                Ok(())
            }
        });
        self.publish_directory_state(session_id);
        result.unwrap_or(Ok(()))
    }

    /// 可见性：`cancel=true`（退到后台/keepAll 隐藏）撤销未投递 cd 与 queuedLine；
    /// `cancel=false`（系统暂隐）只暂停投递。
    pub fn set_visibility(&self, session_id: &str, visible: bool, cancel: bool) {
        let withdrawn_line = self
            .with_entry(session_id, |entry, _| {
                entry.handle.set_directory_visibility(visible, cancel);
                if !visible && cancel {
                    let withdrawn = entry.sync.went_background();
                    entry.delivery_visible = false;
                    withdrawn
                } else {
                    entry.delivery_visible = visible;
                    None
                }
            })
            .flatten();
        if let Some(line) = withdrawn_line {
            self.withdrawn
                .lock()
                .expect("withdrawn")
                .insert(session_id.to_owned(), line);
        }
        self.emit_changed(session_id);
    }

    /// 取回被撤销的命令草稿（UI 提示重新提交）。
    pub fn take_withdrawn(&self, session_id: &str) -> Option<QueuedLine> {
        self.withdrawn.lock().expect("withdrawn").remove(session_id)
    }

    pub fn snapshot(&self, session_id: &str) -> AppResult<TerminalSnapshot> {
        let inner = self.inner.lock().expect("terminals");
        let entry = inner
            .get(session_id)
            .ok_or_else(|| AppError::not_found("终端未启动"))?;
        let mut snapshot = entry.handle.snapshot();
        snapshot.directory_sync = entry.sync.state();
        snapshot.pending_directory = if matches!(
            entry.sync.state(),
            DirectorySync::Pending | DirectorySync::Syncing | DirectorySync::Failed
        ) {
            entry.sync.pending_target().map(str::to_owned)
        } else {
            None
        };
        Ok(snapshot)
    }

    /// 显示端订阅：从游标继续（旧游标越界时要求先取快照）。
    pub fn subscribe(
        &self,
        session_id: &str,
        cursor: u64,
        sender: Sender<TerminalEvent>,
    ) -> AppResult<u64> {
        let inner = self.inner.lock().expect("terminals");
        let entry = inner
            .get(session_id)
            .ok_or_else(|| AppError::not_found("终端未启动"))?;
        entry
            .handle
            .subscribe_from(cursor, sender)
            .map_err(|message| {
                if message.contains("游标") {
                    AppError::conflict(message, None)
                } else {
                    AppError::internal(message)
                }
            })
    }

    pub fn unsubscribe(&self, session_id: &str, subscription: u64) -> AppResult<()> {
        let inner = self.inner.lock().expect("terminals");
        if let Some(entry) = inner.get(session_id) {
            entry
                .handle
                .unsubscribe(subscription)
                .map_err(AppError::internal)?;
        }
        Ok(())
    }

    pub fn has_terminal(&self, session_id: &str) -> bool {
        self.inner
            .lock()
            .expect("terminals")
            .contains_key(session_id)
    }

    /// 结束会话的终端：停止接收新输入、终止进程树、回收。
    /// 用户主动结束：后续 PTY 退出码不参与 failed 判定（FR-SESSION-005 结束状态）。
    pub fn end_session(&self, session_id: &str) {
        self.expecting_exit
            .lock()
            .expect("expecting exit")
            .insert(session_id.to_owned(), true);
        self.shutdown_entry(session_id, true);
    }

    pub fn end_all(&self) {
        let ids: Vec<String> = self
            .inner
            .lock()
            .expect("terminals")
            .keys()
            .cloned()
            .collect();
        for id in ids {
            self.end_session(&id);
        }
    }

    fn shutdown_entry(&self, session_id: &str, remove: bool) {
        let entry = if remove {
            self.inner.lock().expect("terminals").remove(session_id)
        } else {
            None
        };
        if let Some(entry) = entry {
            entry.handle.shutdown();
        }
        self.withdrawn.lock().expect("withdrawn").remove(session_id);
    }

    fn with_entry<T>(
        &self,
        session_id: &str,
        work: impl FnOnce(&mut Entry, &Self) -> T,
    ) -> Option<T> {
        let mut inner = self.inner.lock().expect("terminals");
        inner.get_mut(session_id).map(|entry| work(entry, self))
    }

    fn withdrawn(&self, session_id: &str, message: &str) {
        let _ = message;
        if let Some(line) = self
            .inner
            .lock()
            .expect("terminals")
            .get_mut(session_id)
            .and_then(|entry| entry.sync.take_withdrawn_line())
        {
            self.withdrawn
                .lock()
                .expect("withdrawn")
                .insert(session_id.to_owned(), line);
        }
    }

    fn emit_changed(&self, session_id: &str) {
        let revision = self
            .inner
            .lock()
            .expect("terminals")
            .get_mut(session_id)
            .map(|entry| {
                entry.revision += 1;
                entry.revision
            })
            .unwrap_or(0);
        self.events.emit(AppEvent::TerminalChanged {
            session_id: session_id.to_owned(),
            revision: Revision::new(revision),
        });
    }
}

fn line_withdraw_message(_message: &str) -> String {
    "目录同步失败，命令已退回草稿".to_owned()
}
