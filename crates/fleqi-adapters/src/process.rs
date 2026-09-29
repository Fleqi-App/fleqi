//! ProcessRunner（ADR-004、architecture.md §7.3）：AI 一次性任务进程适配。
//! 与 TerminalManager 共用 portable-pty 的显式 CommandBuilder：路径与参数作为
//! 独立值传递，不经过任何 shell 解析（NFR-SEC-001）；取消终止整个进程组。

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize};
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

#[derive(Debug, Clone)]
pub struct SpawnRequest {
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessEvent {
    Output {
        stream: StreamKind,
        bytes: Vec<u8>,
        seq: u64,
    },
    Exited {
        status: Option<i32>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamKind {
    Stdout,
    Stderr,
}

/// 应用层 ProcessPort 的生产实现。
pub struct ProcessRunnerPort;

impl fleqi_application::ports::ProcessPort for ProcessRunnerPort {
    fn spawn(
        &self,
        executable: &str,
        args: &[String],
        cwd: &std::path::Path,
        env: &[(String, String)],
        events: Sender<fleqi_application::ports::ProcessEvent>,
    ) -> Result<Box<dyn fleqi_application::ports::ProcessHandle>, String> {
        // 事件桥：应用层事件 → 本模块事件（Exited 广播语义一致）。
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for event in rx {
                let forwarded = match event {
                    ProcessEvent::Output { stream, bytes, seq } => {
                        fleqi_application::ports::ProcessEvent::Output {
                            stream: match stream {
                                StreamKind::Stdout => {
                                    fleqi_application::ports::ProcessStreamKind::Stdout
                                }
                                StreamKind::Stderr => {
                                    fleqi_application::ports::ProcessStreamKind::Stderr
                                }
                            },
                            bytes,
                            seq,
                        }
                    }
                    ProcessEvent::Exited { status } => {
                        fleqi_application::ports::ProcessEvent::Exited { status }
                    }
                };
                if events.send(forwarded).is_err() {
                    break;
                }
            }
        });
        let runner = ProcessRunner::spawn(
            SpawnRequest {
                executable: std::path::PathBuf::from(executable),
                args: args.to_vec(),
                cwd: cwd.to_path_buf(),
                env: env.to_vec(),
            },
            tx,
        )
        .map_err(|e| e.to_string())?;
        Ok(Box::new(RunnerHandle {
            runner: std::sync::Mutex::new(Some(runner)),
        }))
    }
}

struct RunnerHandle {
    runner: std::sync::Mutex<Option<ProcessRunner>>,
}

impl fleqi_application::ports::ProcessHandle for RunnerHandle {
    fn wait(&self) -> Option<i32> {
        if let Ok(mut guard) = self.runner.lock() {
            guard.as_mut().and_then(|runner| runner.wait())
        } else {
            None
        }
    }
    fn cancel(&self) {
        if let Ok(mut guard) = self.runner.lock()
            && let Some(runner) = guard.as_mut()
        {
            runner.cancel();
        }
    }
}

pub struct ProcessRunner {
    child: Box<dyn Child + Send + Sync>,
    /// 保留 master 句柄：进程退出前 PTY 读取端保持打开；Drop 时随 runner 关闭。
    _master: Box<dyn MasterPty + Send>,
    cancelled: Arc<AtomicBool>,
    pid: i32,
}

impl ProcessRunner {
    /// 以显式参数向量启动（无 shell）：路径含空格/引号/命令符号也只是单个参数。
    /// 输出经 PTY 合并（stdout/stderr 标记为合并流；区分标记随 M4 视图需要加入）。
    pub fn spawn(request: SpawnRequest, events: Sender<ProcessEvent>) -> std::io::Result<Self> {
        let pty = portable_pty::native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows: 24,
                cols: 500,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let mut command = CommandBuilder::new(&request.executable);
        command.args(&request.args);
        command.cwd(&request.cwd);
        let inherited_path = request
            .env
            .iter()
            .rev()
            .find(|(key, _)| key == "PATH")
            .map(|(_, value)| std::ffi::OsString::from(value))
            .unwrap_or_else(|| std::env::var_os("PATH").unwrap_or_default());
        for (key, value) in &request.env {
            command.env(key, value);
        }
        command.env("PATH", crate::environment::executable_path(&inherited_path));
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        drop(pair.slave);
        let pid = child
            .process_id()
            .ok_or_else(|| std::io::Error::other("无法取得进程 ID"))? as i32;
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let cancelled = Arc::new(AtomicBool::new(false));
        let pump_cancelled = Arc::clone(&cancelled);
        let watcher_child = Arc::new(std::sync::Mutex::new(child));
        let watch_child = Arc::clone(&watcher_child);
        std::thread::spawn(move || {
            let mut reader = reader;
            let mut buffer = [0u8; 8 * 1024];
            let mut seq = 0u64;
            loop {
                if pump_cancelled.load(Ordering::SeqCst) {
                    break;
                }
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        seq += 1;
                        if events
                            .send(ProcessEvent::Output {
                                stream: StreamKind::Stdout,
                                bytes: buffer[..n].to_vec(),
                                seq,
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
            // 输出结束后等待退出并广播状态（取消路径不广播：状态由 wait() 返回）。
            if !pump_cancelled.load(Ordering::SeqCst)
                && let Ok(mut child) = watch_child.lock()
            {
                let status = child.wait().ok().map(|s| s.exit_code() as i32);
                let _ = events.send(ProcessEvent::Exited { status });
            }
        });
        Ok(Self {
            child: Box::new(WatchableChild {
                child: watcher_child,
            }),
            _master: pair.master,
            cancelled,
            pid,
        })
    }

    pub fn pid(&self) -> i32 {
        self.pid
    }

    /// 取消：先向子进程 SIGTERM（进程组随 PTY 会话），再 kill() 兜底并关闭 PTY。
    pub fn cancel(&mut self) {
        self.cancelled.store(true, Ordering::SeqCst);
        // Unix 先 SIGTERM 进程组；Windows 由 PTY/Job 的 kill 结束所属进程。
        #[cfg(unix)]
        unsafe {
            libc::kill(self.pid, libc::SIGTERM);
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        let _ = self.child.kill();
    }

    /// 等待退出并返回状态码。
    pub fn wait(&mut self) -> Option<i32> {
        self.child
            .wait()
            .ok()
            .map(|status| status.exit_code() as i32)
    }
}

impl Drop for ProcessRunner {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::SeqCst);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// 将共享的 child 适配回 Child/ChildKiller trait，供 ProcessRunner::wait/kill 使用。
struct WatchableChild {
    child: Arc<std::sync::Mutex<Box<dyn Child + Send + Sync>>>,
}

impl std::fmt::Debug for WatchableChild {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WatchableChild").finish_non_exhaustive()
    }
}

impl portable_pty::ChildKiller for WatchableChild {
    fn kill(&mut self) -> std::io::Result<()> {
        self.child.lock().expect("child 锁").kill()
    }
    fn clone_killer(&self) -> Box<dyn portable_pty::ChildKiller + Send + Sync> {
        self.child.lock().expect("child 锁").clone_killer()
    }
}

impl Child for WatchableChild {
    fn try_wait(&mut self) -> std::io::Result<Option<portable_pty::ExitStatus>> {
        self.child.lock().expect("child 锁").try_wait()
    }
    fn wait(&mut self) -> std::io::Result<portable_pty::ExitStatus> {
        self.child.lock().expect("child 锁").wait()
    }
    fn process_id(&self) -> Option<u32> {
        self.child.lock().expect("child 锁").process_id()
    }
}
