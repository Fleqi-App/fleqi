//! 当前用户命名管道；握手核对实际 shell PID。终端输出不能伪造安全状态。
use super::{Inner, Integration, TerminalEvent, apply_integration};
use fleqi_domain::revision::Revision;
use std::fs::File;
use std::io::{Read, Write};
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, Ordering},
};
use windows::Win32::{
    Foundation::{ERROR_PIPE_CONNECTED, HANDLE},
    Storage::FileSystem::PIPE_ACCESS_DUPLEX,
    System::Pipes::*,
};

struct Link {
    writer: Option<File>,
    serial: u64,
    visible: bool,
    pending: Option<(u64, Revision)>,
}
impl Link {
    fn send(&mut self, mut value: serde_json::Value) -> std::io::Result<u64> {
        let writer = self
            .writer
            .as_mut()
            .ok_or_else(|| std::io::Error::other("PowerShell 集成尚未就绪"))?;
        self.serial = self
            .serial
            .checked_add(1)
            .ok_or_else(|| std::io::Error::other("目录请求版本已耗尽"))?;
        value["serial"] = self.serial.to_string().into();
        serde_json::to_writer(&mut *writer, &value)?;
        writer.write_all(b"\n")?;
        writer.flush()?;
        Ok(self.serial)
    }
}

pub(super) struct Control {
    pub name: String,
    pipe: File,
    link: Arc<Mutex<Link>>,
    pub connected: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}
impl Control {
    pub fn new() -> std::io::Result<Self> {
        let nonce = tempfile::Builder::new().prefix("fleqi-").tempfile()?;
        let name = format!(
            "fleqi-{}-{}",
            std::process::id(),
            nonce
                .path()
                .file_name()
                .expect("temp name")
                .to_string_lossy()
        );
        let path = windows::core::HSTRING::from(format!("\\\\.\\pipe\\{name}"));
        // SAFETY: 使用当前进程默认 DACL；远程客户端拒绝，连接后另核对 shell PID。
        let raw = unsafe {
            CreateNamedPipeW(
                &path,
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                256 * 1024,
                256 * 1024,
                0,
                None,
            )
        };
        if raw.is_invalid() {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self {
            name,
            pipe: unsafe { File::from_raw_handle(raw.0) },
            link: Arc::new(Mutex::new(Link {
                writer: None,
                serial: 0,
                visible: true,
                pending: None,
            })),
            connected: Arc::new(AtomicBool::new(false)),
            stop: Arc::new(AtomicBool::new(false)),
        })
    }
    pub fn start(&self, inner: Weak<Inner>, pid: u32) -> std::io::Result<()> {
        let pipe = self.pipe.try_clone()?;
        let link = self.link.clone();
        let connected = self.connected.clone();
        let stop = self.stop.clone();
        std::thread::spawn(move || {
            let handle = HANDLE(pipe.as_raw_handle());
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
            loop {
                let result = unsafe { ConnectNamedPipe(handle, None) };
                if result.is_ok()
                    || result.as_ref().is_err_and(|error| {
                        error.code() == windows::core::HRESULT::from_win32(ERROR_PIPE_CONNECTED.0)
                    })
                {
                    break;
                }
                if stop.load(Ordering::Acquire) || std::time::Instant::now() > deadline {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            let mut client_pid = 0;
            if unsafe { GetNamedPipeClientProcessId(handle, &mut client_pid) }.is_err()
                || client_pid != pid
            {
                return;
            }
            let Ok(output) = pipe.try_clone() else {
                return;
            };
            {
                let mut link = link.lock().expect("pipe link");
                link.writer = Some(output);
                let visible = link.visible;
                if link
                    .send(serde_json::json!({ "kind": "visibility", "visible": visible }))
                    .is_err()
                {
                    link.writer = None;
                    let _ = unsafe { DisconnectNamedPipe(handle) };
                    return;
                }
            }
            connected.store(true, Ordering::Release);
            let mut reader = pipe;
            let mut buffered = Vec::new();
            loop {
                if stop.load(Ordering::Acquire) {
                    break;
                }
                let line = match read_message(&mut reader, &mut buffered) {
                    Ok(Some(line)) => line,
                    Ok(None) => {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                        continue;
                    }
                    Err(_) => break,
                };
                let Some(inner) = inner.upgrade() else {
                    break;
                };
                let Ok(value) = serde_json::from_slice::<serde_json::Value>(&line) else {
                    break;
                };
                let mut link = link.lock().expect("pipe link");
                let mut state = inner.state.lock().expect("state");
                match value["event"].as_str() {
                    Some("preexec") => apply_integration(&inner, &mut state, Integration::Preexec),
                    Some("state") => {
                        let (Some(cwd), Some(edit)) =
                            (value["cwd"].as_str(), value["edit"].as_u64())
                        else {
                            continue;
                        };
                        let ready = link.visible
                            && value["ready"] == true
                            && edit == 0
                            && !state.delivering;
                        if state.prompt_ready == ready
                            && state.cwd == cwd
                            && state.edit_len == edit as usize
                        {
                            continue;
                        }
                        state.cwd = cwd.into();
                        if ready {
                            apply_integration(
                                &inner,
                                &mut state,
                                Integration::Prompt { cwd: cwd.into() },
                            );
                        } else {
                            apply_integration(&inner, &mut state, Integration::Preexec);
                        }
                        apply_integration(&inner, &mut state, Integration::EditLen(edit as usize));
                    }
                    Some("cd" | "cancelled") => {
                        let serial = value["serial"]
                            .as_str()
                            .and_then(|serial| serial.parse::<u64>().ok());
                        let revision = value["revision"]
                            .as_str()
                            .and_then(|revision| revision.parse::<u64>().ok())
                            .map(Revision::new);
                        let Some((serial, revision)) = serial.zip(revision) else {
                            continue;
                        };
                        if link.pending != Some((serial, revision)) {
                            continue;
                        }
                        let Some(cwd) = value["cwd"].as_str() else {
                            continue;
                        };
                        link.pending = None;
                        let pending = state.pending_cd.take();
                        state.cwd = cwd.into();
                        state.prompt_ready = false;
                        let event = if value["event"] == "cancelled" {
                            TerminalEvent::CdCancelled { revision }
                        } else {
                            let ok = value["ok"] == true
                                && pending.is_some_and(|(target, expected)| {
                                    expected == revision
                                        && super::same_directory(&target, std::path::Path::new(cwd))
                                });
                            TerminalEvent::CdResult {
                                revision,
                                ok,
                                cwd: cwd.into(),
                                message: (!ok).then(|| {
                                    value["message"]
                                        .as_str()
                                        .unwrap_or("shell 未进入目标目录")
                                        .to_owned()
                                }),
                            }
                        };
                        let _ = inner.events.send(event);
                    }
                    _ => (),
                }
            }
            connected.store(false, Ordering::Release);
            let pending = {
                let mut link = link.lock().expect("pipe link");
                link.writer = None;
                link.pending.take()
            };
            if let Some(inner) = inner.upgrade() {
                let mut state = inner.state.lock().expect("state");
                state.pending_cd = None;
                if let Some((_, revision)) = pending {
                    let _ = inner.events.send(TerminalEvent::CdResult {
                        revision,
                        ok: false,
                        cwd: state.cwd.clone(),
                        message: Some("PowerShell 控制通道中断，目录切换未确认".into()),
                    });
                }
                apply_integration(&inner, &mut state, Integration::Preexec);
            }
        });
        Ok(())
    }
    pub fn cd(&self, path: &std::path::Path, revision: Revision) -> std::io::Result<()> {
        let path = path
            .to_str()
            .ok_or_else(|| std::io::Error::other("此目录包含 PowerShell 无法无损传递的字符"))?;
        let mut link = self.link.lock().expect("pipe link");
        let serial = link.send(
            serde_json::json!({ "kind": "cd", "path": path, "revision": revision.to_string() }),
        )?;
        link.pending = Some((serial, revision));
        Ok(())
    }
    pub fn cancel(&self) {
        let mut link = self.link.lock().expect("pipe link");
        if link.pending.is_some() && link.send(serde_json::json!({ "kind": "cancel" })).is_err() {
            self.disconnect();
        }
    }
    pub fn visibility(&self, visible: bool, cancel: bool) {
        let mut link = self.link.lock().expect("pipe link");
        if link.visible == visible && !cancel {
            return;
        }
        link.visible = visible;
        if link.writer.is_some()
            && link
                .send(serde_json::json!({ "kind": "visibility", "visible": visible }))
                .is_err()
        {
            self.disconnect();
        }
    }
    fn disconnect(&self) {
        self.connected.store(false, Ordering::Release);
        let _ = unsafe { DisconnectNamedPipe(HANDLE(self.pipe.as_raw_handle())) };
    }
}
impl Drop for Control {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.disconnect();
    }
}

fn read_message(pipe: &mut File, buffered: &mut Vec<u8>) -> std::io::Result<Option<Vec<u8>>> {
    if let Some(end) = buffered.iter().position(|byte| *byte == b'\n') {
        return Ok(Some(buffered.drain(..=end).collect()));
    }
    let mut bytes = [0; 8192];
    match pipe.read(&mut bytes) {
        Ok(0) => Ok(None), // PIPE_NOWAIT：已连接但暂时没有字节，不能当成 EOF。
        Ok(n) if buffered.len() + n <= 256 * 1024 => {
            buffered.extend_from_slice(&bytes[..n]);
            Ok(None)
        }
        Ok(_) => Err(std::io::Error::other("控制消息超限")),
        Err(error) if error.raw_os_error() == Some(232) => Ok(None),
        Err(error) => Err(error),
    }
}
