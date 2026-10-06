//! 当前用户命名管道；握手核对实际 shell PID。终端输出不能伪造安全状态。
use super::{Inner, Integration, TerminalEvent, apply_integration};
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

pub(super) struct Control {
    pub name: String,
    pipe: File,
    writer: Arc<Mutex<Option<File>>>,
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
            writer: Arc::new(Mutex::new(None)),
            connected: Arc::new(AtomicBool::new(false)),
            stop: Arc::new(AtomicBool::new(false)),
        })
    }
    pub fn start(&self, inner: Weak<Inner>, pid: u32) -> std::io::Result<()> {
        let pipe = self.pipe.try_clone()?;
        let writer = self.writer.clone();
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
            *writer.lock().expect("pipe writer") = Some(output);
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
                let mut state = inner.state.lock().expect("state");
                match value["event"].as_str() {
                    Some("preexec") => apply_integration(&inner, &mut state, Integration::Preexec),
                    Some("state") => {
                        let Some(cwd) = value["cwd"].as_str() else {
                            continue;
                        };
                        let Some(edit) = value["edit"].as_u64() else {
                            continue;
                        };
                        if state.prompt_ready && state.cwd == cwd && state.edit_len == edit as usize
                        {
                            continue;
                        }
                        apply_integration(
                            &inner,
                            &mut state,
                            Integration::Prompt { cwd: cwd.into() },
                        );
                        apply_integration(&inner, &mut state, Integration::EditLen(edit as usize));
                    }
                    Some("cd") => {
                        let Some((target, revision)) = state.pending_cd.as_ref() else {
                            continue;
                        };
                        if value["revision"].as_str() != Some(revision.to_string().as_str()) {
                            continue;
                        }
                        let Some(cwd) = value["cwd"].as_str() else {
                            continue;
                        };
                        let ok = value["ok"] == true
                            && super::same_directory(target, std::path::Path::new(cwd));
                        let revision = *revision;
                        state.pending_cd = None;
                        state.cwd = cwd.into();
                        let _ = inner.events.send(TerminalEvent::CdResult {
                            revision,
                            ok,
                            cwd: cwd.into(),
                            message: (!ok).then(|| {
                                value["message"]
                                    .as_str()
                                    .unwrap_or("shell 未进入目标目录")
                                    .to_owned()
                            }),
                        });
                    }
                    _ => (),
                }
            }
            connected.store(false, Ordering::Release);
            *writer.lock().expect("pipe writer") = None;
            if let Some(inner) = inner.upgrade() {
                apply_integration(
                    &inner,
                    &mut inner.state.lock().expect("state"),
                    Integration::Preexec,
                );
            }
        });
        Ok(())
    }
    pub fn cd(
        &self,
        path: &std::path::Path,
        revision: fleqi_domain::revision::Revision,
    ) -> std::io::Result<()> {
        let path = path
            .to_str()
            .ok_or_else(|| std::io::Error::other("此目录包含 PowerShell 无法无损传递的字符"))?;
        let mut guard = self.writer.lock().expect("pipe writer");
        let writer = guard
            .as_mut()
            .ok_or_else(|| std::io::Error::other("PowerShell 集成尚未就绪"))?;
        serde_json::to_writer(
            &mut *writer,
            &serde_json::json!({ "path": path, "revision": revision.to_string() }),
        )?;
        writer.write_all(b"\n")?;
        writer.flush()
    }
}
impl Drop for Control {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
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
