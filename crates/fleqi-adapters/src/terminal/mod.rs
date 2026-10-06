//! TerminalManager（architecture.md §6、ADR-003/004）：portable-pty + 平台 shell + 自带 shell integration。
//! 所有权：真实 child handle、串行写入、输出消费与落盘、屏幕状态（vt100）、订阅游标、退出回收。
//! 手动输入与目录控制消息共用一条串行写入队列；目录控制消息只由本模块生成。

pub mod integration;
pub mod osc;
pub mod segments;
#[cfg(windows)]
mod windows_control;

use fleqi_domain::directory_sync::{DirectorySync, ShellReadiness};
use fleqi_domain::revision::Revision;
use fleqi_domain::terminal::{ShellReadinessState, TerminalSize, TerminalSnapshot, TerminalState};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use osc::{Integration, OscExtractor};
use segments::SegmentStore;

const RING_BYTES: usize = 512 * 1024;
const SCROLLBACK_LINES: usize = 10_000;

pub use fleqi_application::ports::TerminalEvent;
use fleqi_application::ports::{TerminalHandle, TerminalPort};

pub struct TerminalOptions {
    pub session_id: String,
    pub cwd: PathBuf,
    pub cols: u16,
    pub rows: u16,
    /// 本终端的输出分段与集成文件目录（应用数据目录下）。
    pub data_dir: PathBuf,
    pub max_persisted_bytes: u64,
    pub events: Sender<TerminalEvent>,
}

#[derive(Debug, thiserror::Error)]
pub enum TerminalError {
    #[error("PTY 错误：{0}")]
    Pty(String),
    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),
    #[error("终端已退出")]
    Exited,
    #[error("游标 {cursor} 已超出保留区（最早 {earliest}），需要重新获取快照")]
    CursorOutOfRange { cursor: u64, earliest: u64 },
}

struct State {
    prompt_ready: bool,
    edit_len: usize,
    cwd: String,
    size: TerminalSize,
    cursor: u64,
    ring: VecDeque<(u64, Vec<u8>)>,
    ring_bytes: usize,
    parser: vt100::Parser,
    segments: SegmentStore,
    pending_cd: Option<(PathBuf, Revision)>,
    subscribers: Vec<(u64, Sender<TerminalEvent>)>,
    next_subscription: u64,
    exited: Option<Option<i32>>,
    delivering: bool,
}

struct Inner {
    session_id: String,
    terminal_id: String,
    shell_pid: u32,
    master: Mutex<Option<Box<dyn MasterPty + Send>>>,
    state: Mutex<State>,
    events: Sender<TerminalEvent>,
}

pub struct TerminalManager {
    inner: Arc<Inner>,
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    reader: Option<JoinHandle<()>>,
    /// Unix PTY 设备名，用于结束时回收仍附着该 tty 的进程。Windows ConPTY 没有对应路径。
    #[cfg_attr(not(unix), allow(dead_code))]
    tty: Option<PathBuf>,
    shell: integration::ShellKind,
    #[cfg(windows)]
    control: windows_control::Control,
    #[cfg(windows)]
    job: crate::windows_job::Job,
}

impl TerminalManager {
    pub fn spawn(options: TerminalOptions) -> Result<Self, TerminalError> {
        std::fs::create_dir_all(&options.data_dir)?;
        let shell = integration::detect_shell();
        #[cfg(windows)]
        let control = windows_control::Control::new()?;
        #[cfg(windows)]
        let job = crate::windows_job::Job::new()?;
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows: options.rows,
                cols: options.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| TerminalError::Pty(e.to_string()))?;
        let mut cmd = match shell {
            integration::ShellKind::Zsh => {
                let home = std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("/"));
                let user_zdotdir = std::env::var_os("ZDOTDIR")
                    .map(PathBuf::from)
                    .unwrap_or(home);
                let zdotdir = integration::install(&options.data_dir, &user_zdotdir)?;
                let mut cmd = CommandBuilder::new("/bin/zsh");
                cmd.arg("-i");
                cmd.env("ZDOTDIR", &zdotdir);
                cmd
            }
            integration::ShellKind::Bash => {
                let rc = integration::install_bash(&options.data_dir)?;
                let mut cmd = CommandBuilder::new("/bin/bash");
                cmd.arg("--noprofile");
                cmd.arg("--rcfile");
                cmd.arg(rc);
                cmd
            }
            integration::ShellKind::PowerShell => {
                let profile = integration::install_powershell(&options.data_dir)?;
                let mut cmd = CommandBuilder::new(crate::environment::powershell());
                cmd.args([
                    "-NoLogo",
                    "-NoExit",
                    "-NoProfile",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-File",
                ]);
                cmd.arg(profile);
                #[cfg(windows)]
                cmd.env("FLEQI_PIPE_NAME", &control.name);
                cmd
            }
        };
        cmd.cwd(&options.cwd);
        cmd.env(
            "PATH",
            crate::environment::executable_path(&std::env::var_os("PATH").unwrap_or_default()),
        );
        cmd.env("TERM", "xterm-256color");
        cmd.env(
            "LANG",
            std::env::var("LANG").unwrap_or_else(|_| "en_US.UTF-8".into()),
        );
        cmd.env("FLEQI_TERMINAL", "1");
        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| TerminalError::Pty(e.to_string()))?;
        drop(pair.slave);
        let shell_pid = child
            .process_id()
            .ok_or_else(|| TerminalError::Pty("无法取得 shell pid".into()))?;
        #[cfg(windows)]
        {
            let mut child = child;
            if let Err(error) = child
                .as_raw_handle()
                .ok_or_else(|| std::io::Error::other("无法取得终端进程句柄"))
                .and_then(|handle| job.assign(handle))
            {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.into());
            }
            Self::finish_spawn(options, shell, pair.master, child, shell_pid, control, job)
        }
        #[cfg(not(windows))]
        Self::finish_spawn(options, shell, pair.master, child, shell_pid)
    }

    fn finish_spawn(
        options: TerminalOptions,
        shell: integration::ShellKind,
        master: Box<dyn MasterPty + Send>,
        child: Box<dyn Child + Send + Sync>,
        shell_pid: u32,
        #[cfg(windows)] control: windows_control::Control,
        #[cfg(windows)] job: crate::windows_job::Job,
    ) -> Result<Self, TerminalError> {
        let reader = master
            .try_clone_reader()
            .map_err(|e| TerminalError::Pty(e.to_string()))?;
        let writer = master
            .take_writer()
            .map_err(|e| TerminalError::Pty(e.to_string()))?;
        #[cfg(unix)]
        let tty = master.tty_name();
        #[cfg(not(unix))]
        let tty = None;

        let segments = SegmentStore::open(
            &options.data_dir.join("segments"),
            options.max_persisted_bytes,
        )?;
        let inner = Arc::new(Inner {
            session_id: options.session_id.clone(),
            terminal_id: format!("term-{}-{shell_pid}", options.session_id),
            shell_pid,
            master: Mutex::new(Some(master)),
            state: Mutex::new(State {
                prompt_ready: false,
                edit_len: 0,
                cwd: options.cwd.to_string_lossy().into_owned(),
                size: TerminalSize {
                    cols: options.cols,
                    rows: options.rows,
                },
                cursor: 0,
                ring: VecDeque::new(),
                ring_bytes: 0,
                parser: vt100::Parser::new(options.rows, options.cols, SCROLLBACK_LINES),
                segments,
                pending_cd: None,
                subscribers: Vec::new(),
                next_subscription: 1,
                exited: None,
                delivering: false,
            }),
            events: options.events,
        });
        let reader_inner = Arc::clone(&inner);
        let child = Arc::new(Mutex::new(child));
        #[cfg(windows)]
        {
            let child = Arc::clone(&child);
            let inner = Arc::clone(&inner);
            std::thread::spawn(move || {
                let status = child
                    .lock()
                    .expect("child")
                    .wait()
                    .ok()
                    .map(|status| status.exit_code() as i32);
                inner.state.lock().expect("state").exited = Some(status);
                // Windows 的客户端退出不会自行关闭 ConPTY；先关闭控制台，reader 才能收到 EOF。
                inner.master.lock().expect("master").take();
            });
        }
        let writer = Arc::new(Mutex::new(writer));
        let reader_writer = Arc::clone(&writer);
        #[cfg(windows)]
        control.start(Arc::downgrade(&inner), shell_pid)?;
        let reader_thread = std::thread::Builder::new()
            .name(format!("fleqi-pty-reader-{shell_pid}"))
            .spawn(move || read_loop(reader_inner, reader, reader_writer))?;
        Ok(Self {
            inner,
            child,
            writer,
            reader: Some(reader_thread),
            tty,
            shell,
            #[cfg(windows)]
            control,
            #[cfg(windows)]
            job,
        })
    }

    pub fn terminal_id(&self) -> &str {
        &self.inner.terminal_id
    }

    pub fn shell_pid(&self) -> u32 {
        self.inner.shell_pid
    }

    /// 终端面板的原始用户输入：不经 AI、不解析。
    pub fn write_input(&self, bytes: &[u8]) -> Result<(), TerminalError> {
        self.ensure_running()?;
        {
            let mut state = self.inner.state.lock().expect("state");
            state.delivering = true;
            #[cfg(windows)]
            {
                state.prompt_ready = false;
                state.edit_len = 1;
            }
        }
        let result = {
            let mut writer = self.writer.lock().expect("writer");
            writer.write_all(bytes).and_then(|_| writer.flush())
        };
        self.inner.state.lock().expect("state").delivering = false;
        result.map_err(TerminalError::Io)
    }

    /// 目录控制消息：只有本模块生成；调用方（应用层）负责先核对安全提示符。
    pub fn send_cd(&self, target: &Path, revision: Revision) -> Result<(), TerminalError> {
        self.ensure_running()?;
        {
            let mut state = self.inner.state.lock().expect("state");
            state.pending_cd = Some((target.to_path_buf(), revision));
            state.prompt_ready = false;
        }
        #[cfg(windows)]
        {
            let result = self.control.cd(target, revision);
            if result.is_err() {
                self.inner.state.lock().expect("state").pending_cd = None;
            }
            result.map_err(TerminalError::Io)
        }
        #[cfg(not(windows))]
        let line = match self.shell {
            integration::ShellKind::PowerShell => integration::powershell_cd_line(target),
            integration::ShellKind::Zsh | integration::ShellKind::Bash => {
                integration::cd_control_line(target)
            }
        };
        #[cfg(not(windows))]
        {
            let mut writer = self.writer.lock().expect("writer");
            writer.write_all(&line)?;
            writer.flush()?;
            Ok(())
        }
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), TerminalError> {
        if cols == 0 || rows == 0 {
            return Ok(());
        }
        self.inner
            .master
            .lock()
            .expect("master")
            .as_ref()
            .ok_or(TerminalError::Exited)?
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| TerminalError::Pty(e.to_string()))?;
        let mut state = self.inner.state.lock().expect("state");
        state.size = TerminalSize { cols, rows };
        state.parser.screen_mut().set_size(rows, cols);
        Ok(())
    }

    /// 安全性证据：prompt/编辑行来自 shell integration，前台进程组来自 tcgetpgrp。
    pub fn readiness(&self) -> ShellReadiness {
        let state = self.inner.state.lock().expect("state");
        #[cfg(not(windows))]
        let foreground_is_shell = self
            .foreground_pgid()
            .map(|pg| pg == self.inner.shell_pid as i32)
            .unwrap_or(false);
        #[cfg(windows)]
        let foreground_is_shell = self
            .control
            .connected
            .load(std::sync::atomic::Ordering::Acquire)
            && state.prompt_ready;
        ShellReadiness {
            prompt_ready: state.prompt_ready && state.exited.is_none(),
            edit_line_empty: state.edit_len == 0,
            foreground_is_shell,
            delivering: state.delivering,
        }
    }

    #[cfg(unix)]
    fn foreground_pgid(&self) -> Option<i32> {
        self.inner
            .master
            .lock()
            .expect("master")
            .as_ref()
            .and_then(|master| master.process_group_leader())
    }

    #[cfg(not(unix))]
    fn foreground_pgid(&self) -> Option<i32> {
        None
    }

    /// 前台程序名（前台进程组 ≠ shell 时）。
    pub fn foreground_process(&self) -> Option<String> {
        let pgid = self.foreground_pgid()?;
        if pgid == self.inner.shell_pid as i32 {
            return None;
        }
        let output = std::process::Command::new("/bin/ps")
            .args(["-o", "comm=", "-p", &pgid.to_string()])
            .output()
            .ok()?;
        let name = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if name.is_empty() {
            Some(format!("pid {pgid}"))
        } else {
            Some(name)
        }
    }

    pub fn current_directory(&self) -> String {
        self.inner.state.lock().expect("state").cwd.clone()
    }

    pub fn snapshot(&self) -> TerminalSnapshot {
        // 前台程序名是实时查询（tcgetpgrp + ps），在取状态锁前完成。
        let foreground = self.foreground_process();
        let state = self.inner.state.lock().expect("state");
        #[cfg(windows)]
        let integration_unavailable = !self
            .control
            .connected
            .load(std::sync::atomic::Ordering::Acquire);
        #[cfg(not(windows))]
        let integration_unavailable = false;
        let readiness = if state.exited.is_some() || integration_unavailable {
            ShellReadinessState::Unknown
        } else if state.prompt_ready {
            ShellReadinessState::Ready
        } else {
            ShellReadinessState::Busy
        };
        TerminalSnapshot {
            terminal_id: self.inner.terminal_id.clone(),
            session_id: self.inner.session_id.clone(),
            state: match state.exited {
                Some(_) => TerminalState::Exited,
                None => TerminalState::Running,
            },
            shell: self.shell.label().into(),
            size: state.size,
            shell_readiness: readiness,
            foreground_process: foreground,
            current_directory: state.cwd.clone(),
            pending_directory: state
                .pending_cd
                .as_ref()
                .map(|(p, _)| p.to_string_lossy().into_owned()),
            directory_sync: if state.pending_cd.is_some() {
                DirectorySync::Syncing
            } else {
                DirectorySync::Synced
            },
            screen: String::from_utf8_lossy(&state.parser.screen().state_formatted()).into_owned(),
            stream_cursor: state.cursor.to_string(),
            exit_status: state.exited.flatten(),
            truncated: state.segments.truncated(),
        }
    }

    /// 从游标继续订阅：先补发保留区内游标之后的字节，再接收后续输出。
    pub fn subscribe_from(
        &self,
        cursor: u64,
        sender: Sender<TerminalEvent>,
    ) -> Result<u64, TerminalError> {
        let mut state = self.inner.state.lock().expect("state");
        let earliest = state
            .ring
            .front()
            .map(|(start, _)| *start)
            .unwrap_or(state.cursor);
        if cursor < earliest {
            return Err(TerminalError::CursorOutOfRange { cursor, earliest });
        }
        for (start, chunk) in &state.ring {
            let end = start + chunk.len() as u64;
            if end <= cursor {
                continue;
            }
            let skip = cursor.saturating_sub(*start) as usize;
            let _ = sender.send(TerminalEvent::Output {
                bytes: chunk[skip..].to_vec(),
                cursor: end,
            });
        }
        let id = state.next_subscription;
        state.next_subscription += 1;
        state.subscribers.push((id, sender));
        Ok(id)
    }

    fn ensure_running(&self) -> Result<(), TerminalError> {
        if self.inner.state.lock().expect("state").exited.is_some() {
            Err(TerminalError::Exited)
        } else {
            Ok(())
        }
    }

    pub fn has_exited(&self) -> bool {
        self.inner.state.lock().expect("state").exited.is_some()
    }

    /// 结束会话：SIGHUP shell（zsh 向作业转发 HUP），等待，再对仍附着本 tty 的进程 SIGKILL；回收 reader。
    pub fn shutdown(mut self) {
        self.terminate();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }

    fn terminate(&mut self) {
        #[cfg(windows)]
        self.job.terminate();
        let pid = self.inner.shell_pid as i32;
        #[cfg(unix)]
        {
            // SAFETY: 向本进程创建的 shell 发送信号。
            unsafe {
                libc::kill(pid, libc::SIGHUP);
            }
        }
        #[cfg(not(unix))]
        let _ = pid;
        let deadline = std::time::Instant::now() + Duration::from_millis(1500);
        loop {
            let done = self
                .child
                .lock()
                .expect("child")
                .try_wait()
                .ok()
                .flatten()
                .is_some();
            if done || std::time::Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        #[cfg(unix)]
        if let Some(tty) = &self.tty {
            let name = tty.to_string_lossy();
            let short = name.strip_prefix("/dev/").unwrap_or(&name).to_owned();
            if let Ok(output) = std::process::Command::new("/bin/ps")
                .args(["-o", "pid=", "-t", &short])
                .output()
            {
                for line in String::from_utf8_lossy(&output.stdout).lines() {
                    if let Ok(p) = line.trim().parse::<i32>() {
                        // SAFETY: 只对附着本终端 tty 的进程发送信号。
                        unsafe {
                            libc::kill(p, libc::SIGKILL);
                        }
                    }
                }
            }
        }
        let _ = self.child.lock().expect("child").kill();
        let _ = self.child.lock().expect("child").wait();
        // 关闭 master 使 reader 线程读到 EOF。
        let _ = std::mem::replace(
            &mut *self.writer.lock().expect("writer"),
            Box::new(std::io::sink()),
        );
        // ConPTY 的输出 EOF 依赖关闭 pseudo-console，必须先关闭再 join reader。
        self.inner.master.lock().expect("master").take();
        let mut state = self.inner.state.lock().expect("state");
        if state.exited.is_none() {
            state.exited = Some(None);
        }
    }
}

impl Drop for TerminalManager {
    fn drop(&mut self) {
        if self.reader.is_some() {
            self.terminate();
        }
    }
}

fn read_loop(
    inner: Arc<Inner>,
    mut reader: Box<dyn Read + Send>,
    _writer: Arc<Mutex<Box<dyn Write + Send>>>,
) {
    let mut extractor = OscExtractor::default();
    let mut buf = vec![0u8; 16 * 1024];
    #[cfg(windows)]
    let (mut inherited_cursor, mut startup_output) = (false, Vec::new());
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        let (forward, messages) = extractor.feed(&buf[..n]);
        #[cfg(windows)]
        let forward = if !inherited_cursor {
            // ConPTY 的 INHERIT_CURSOR 握手发生在 shell 启动前；即使终端面板未打开也必须应答。
            startup_output.extend_from_slice(&forward);
            if let Some(index) = startup_output
                .windows(4)
                .position(|bytes| bytes == b"\x1b[6n")
            {
                startup_output.drain(index..index + 4);
                let mut writer = _writer.lock().expect("writer");
                let _ = writer.write_all(b"\x1b[1;1R").and_then(|_| writer.flush());
                inherited_cursor = true;
                std::mem::take(&mut startup_output)
            } else if startup_output.len() > 65536 {
                inherited_cursor = true;
                std::mem::take(&mut startup_output)
            } else {
                Vec::new()
            }
        } else {
            forward
        };
        let mut state = inner.state.lock().expect("state");
        if !forward.is_empty() {
            state.parser.process(&forward);
            let _ = state.segments.append(&forward);
            let start = state.cursor;
            state.cursor += forward.len() as u64;
            let cursor = state.cursor;
            state.ring_bytes += forward.len();
            state.ring.push_back((start, forward.clone()));
            while state.ring_bytes > RING_BYTES {
                if let Some((_, old)) = state.ring.pop_front() {
                    state.ring_bytes -= old.len();
                } else {
                    break;
                }
            }
            let event = TerminalEvent::Output {
                bytes: forward,
                cursor,
            };
            state
                .subscribers
                .retain(|(_, s)| s.send(event.clone()).is_ok());
            let _ = inner.events.send(event);
        }
        for message in messages {
            #[cfg(not(windows))]
            apply_integration(&inner, &mut state, message);
            #[cfg(windows)]
            let _ = message; // PowerShell 状态只接受经 PID 验证的控制管道。
        }
    }
    let mut state = inner.state.lock().expect("state");
    state.prompt_ready = false;
    if state.exited.is_none() {
        state.exited = Some(None);
    }
    let event = TerminalEvent::Exited {
        status: state.exited.flatten(),
    };
    for (_, sender) in state.subscribers.drain(..) {
        let _ = sender.send(event.clone());
    }
    let _ = inner.events.send(event);
}

fn apply_integration(inner: &Inner, state: &mut State, message: Integration) {
    match message {
        Integration::Prompt { cwd } => {
            state.prompt_ready = true;
            state.edit_len = 0;
            state.cwd = cwd.clone();
            if !cfg!(windows)
                && let Some((target, revision)) = state.pending_cd.take()
            {
                let ok = same_directory(&target, Path::new(&cwd));
                let message = (!ok).then(|| format!("shell 未进入 {}", target.to_string_lossy()));
                let _ = inner.events.send(TerminalEvent::CdResult {
                    revision,
                    ok,
                    cwd: cwd.clone(),
                    message,
                });
            }
            let event = TerminalEvent::PromptReady { cwd };
            state
                .subscribers
                .retain(|(_, sender)| sender.send(event.clone()).is_ok());
            let _ = inner.events.send(event);
        }
        Integration::Preexec => {
            state.prompt_ready = false;
            state.edit_len = 0;
            let _ = inner.events.send(TerminalEvent::Preexec);
        }
        Integration::EditLen(len) => {
            state.edit_len = len;
            let _ = inner.events.send(TerminalEvent::EditLine { len });
        }
        Integration::Unknown(_) => {}
    }
}

fn same_directory(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

impl TerminalHandle for TerminalManager {
    fn terminal_id(&self) -> String {
        self.inner.terminal_id.clone()
    }

    fn write_input(&self, bytes: &[u8]) -> Result<(), String> {
        TerminalManager::write_input(self, bytes).map_err(|e| e.to_string())
    }

    fn send_cd(&self, target: &Path, revision: Revision) -> Result<(), String> {
        TerminalManager::send_cd(self, target, revision).map_err(|e| e.to_string())
    }

    fn resize(&self, cols: u16, rows: u16) -> Result<(), String> {
        TerminalManager::resize(self, cols, rows).map_err(|e| e.to_string())
    }

    fn readiness(&self) -> ShellReadiness {
        TerminalManager::readiness(self)
    }

    fn foreground_process(&self) -> Option<String> {
        TerminalManager::foreground_process(self)
    }

    fn current_directory(&self) -> String {
        TerminalManager::current_directory(self)
    }

    fn snapshot(&self) -> TerminalSnapshot {
        TerminalManager::snapshot(self)
    }

    fn subscribe_from(&self, cursor: u64, sender: Sender<TerminalEvent>) -> Result<u64, String> {
        TerminalManager::subscribe_from(self, cursor, sender).map_err(|e| e.to_string())
    }

    fn unsubscribe(&self, subscription: u64) -> Result<(), String> {
        self.inner
            .state
            .lock()
            .expect("state")
            .subscribers
            .retain(|(id, _)| *id != subscription);
        Ok(())
    }
    fn has_exited(&self) -> bool {
        TerminalManager::has_exited(self)
    }

    fn shutdown(self: Box<Self>) {
        TerminalManager::shutdown(*self)
    }
}

/// 生产端口：每个会话在应用数据目录下拥有独立的终端数据目录。
pub struct PtyTerminalPort {
    pub data_root: PathBuf,
    pub max_persisted_bytes: u64,
}

impl TerminalPort for PtyTerminalPort {
    fn spawn(
        &self,
        session_id: &str,
        cwd: &Path,
        cols: u16,
        rows: u16,
        events: Sender<TerminalEvent>,
    ) -> Result<Box<dyn TerminalHandle>, String> {
        let manager = TerminalManager::spawn(TerminalOptions {
            session_id: session_id.to_owned(),
            cwd: cwd.to_path_buf(),
            cols,
            rows,
            data_dir: self.data_root.join("terminals").join(session_id),
            max_persisted_bytes: self.max_persisted_bytes,
            events,
        })
        .map_err(|e| e.to_string())?;
        Ok(Box::new(manager))
    }
}
