//! Linux Bash 私有控制通道；连接必须来自本终端 shell，PTY 输出不参与状态判定。
use super::{Inner, TerminalEvent};
use fleqi_domain::revision::Revision;
use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::fd::AsRawFd;
use std::os::unix::{
    ffi::OsStrExt,
    ffi::OsStringExt,
    fs::PermissionsExt,
    net::{UnixListener, UnixStream},
};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

const LIMIT: usize = 65536;
const BRIDGE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/fleqi-bash.so"));

struct Link {
    stream: Option<UnixStream>,
    generation: u64,
    input_generation: u64,
    visible: bool,
    pending: Option<(u64, Revision)>,
}

impl Link {
    fn send(&mut self, kind: char, arguments: &str) -> std::io::Result<u64> {
        let stream = self
            .stream
            .as_mut()
            .ok_or_else(|| std::io::Error::other("Bash 自动目录同步尚未就绪"))?;
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| std::io::Error::other("目录请求版本已耗尽"))?;
        let packet = format!("{kind} {}{arguments}\n", self.generation);
        stream.write_all(packet.as_bytes())?;
        Ok(self.generation)
    }
}

pub(super) struct Control {
    directory: tempfile::TempDir,
    listener: UnixListener,
    link: Arc<Mutex<Link>>,
    pub connected: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

impl Control {
    pub fn new() -> std::io::Result<Self> {
        let directory = tempfile::Builder::new().prefix("fleqi-bash-").tempdir()?;
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
        let socket = directory.path().join("control");
        let listener = UnixListener::bind(&socket)?;
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        std::fs::write(directory.path().join("bridge.so"), BRIDGE)?;
        Ok(Self {
            directory,
            listener,
            link: Arc::new(Mutex::new(Link {
                stream: None,
                generation: 0,
                input_generation: 0,
                visible: true,
                pending: None,
            })),
            connected: Arc::new(AtomicBool::new(false)),
            stop: Arc::new(AtomicBool::new(false)),
        })
    }
    pub fn socket(&self) -> PathBuf {
        self.directory.path().join("control")
    }
    pub fn library(&self) -> PathBuf {
        self.directory.path().join("bridge.so")
    }

    pub fn start(&self, inner: Weak<Inner>, pid: u32) -> std::io::Result<()> {
        let listener = self.listener.try_clone()?;
        let link = self.link.clone();
        let connected = self.connected.clone();
        let stop = self.stop.clone();
        std::thread::Builder::new()
            .name(format!("fleqi-bash-control-{pid}"))
            .spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(10);
                let peer = loop {
                    if stop.load(Ordering::Acquire) || Instant::now() >= deadline {
                        return;
                    }
                    match listener.accept() {
                        Ok((stream, _)) if peer_pid(&stream) == Some(pid) => break stream,
                        Ok(_) => (),
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(20))
                        }
                        Err(_) => return,
                    }
                };
                let mut peer = peer;
                let _ = peer.set_read_timeout(Some(Duration::from_millis(200)));
                let _ = peer.set_write_timeout(Some(Duration::from_secs(1)));
                let mut buffer = Vec::new();
                let mut initialized = false;
                while !stop.load(Ordering::Acquire) {
                    let packet = match read_packet(&mut peer, &mut buffer) {
                        Ok(Some(packet)) => packet,
                        Ok(None) if initialized || Instant::now() < deadline => continue,
                        _ => break,
                    };
                    if !initialized {
                        if packet != b"H 1" {
                            break;
                        }
                        let Ok(writer) = peer.try_clone() else {
                            break;
                        };
                        let mut writer_link = link.lock().expect("Bash link");
                        writer_link.stream = Some(writer);
                        let visibility = if writer_link.visible { " 1" } else { " 0" };
                        if writer_link.send('V', visibility).is_err() {
                            break;
                        }
                        initialized = true;
                        connected.store(true, Ordering::Release);
                        continue;
                    }
                    let Some(inner) = inner.upgrade() else {
                        break;
                    };
                    if !dispatch(&inner, &link, &packet) {
                        break;
                    }
                }
                connected.store(false, Ordering::Release);
                let pending = {
                    let mut link = link.lock().expect("Bash link");
                    if let Some(stream) = link.stream.take() {
                        let _ = stream.shutdown(Shutdown::Both);
                    }
                    link.pending.take()
                };
                if let Some(inner) = inner.upgrade() {
                    let mut state = inner.state.lock().expect("state");
                    state.prompt_ready = false;
                    state.pending_cd = None;
                    if let Some((_, revision)) = pending {
                        let _ = inner.events.send(TerminalEvent::CdResult {
                            revision,
                            ok: false,
                            cwd: state.cwd.clone(),
                            message: Some("Bash 控制通道中断，目录切换未确认".into()),
                        });
                    }
                    let _ = inner.events.send(TerminalEvent::Preexec);
                }
            })?;
        Ok(())
    }

    pub fn cd(&self, path: &Path, revision: Revision) -> std::io::Result<()> {
        let bytes = path.as_os_str().as_bytes();
        if !path.is_absolute() || bytes.contains(&0) || bytes.len() >= 16384 {
            return Err(std::io::Error::other("Bash 目录路径无效或过长"));
        }
        let mut link = self.link.lock().expect("Bash link");
        let serial = link.send(
            'C',
            &format!(" {} {}", revision.value(), encode_path(bytes)),
        )?;
        link.pending = Some((serial, revision));
        Ok(())
    }

    pub fn cancel(&self) {
        let mut link = self.link.lock().expect("Bash link");
        if link.pending.is_some()
            && link.send('X', "").is_err()
            && let Some(stream) = link.stream.as_ref()
        {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }

    pub fn input(&self, write: impl FnOnce() -> std::io::Result<()>) -> std::io::Result<()> {
        let mut link = self.link.lock().expect("Bash link");
        let result = write();
        if link.stream.is_none() {
            return result;
        }
        match link.send('I', "") {
            Ok(serial) => link.input_generation = serial,
            Err(_) => {
                if let Some(stream) = link.stream.as_ref() {
                    let _ = stream.shutdown(Shutdown::Both);
                }
            }
        }
        result
    }

    pub fn visibility(&self, visible: bool, cancel: bool) {
        let mut link = self.link.lock().expect("Bash link");
        if link.visible == visible && !cancel {
            return;
        }
        link.visible = visible;
        if link.stream.is_some()
            && link.send('V', if visible { " 1" } else { " 0" }).is_err()
            && let Some(stream) = link.stream.as_ref()
        {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

impl Drop for Control {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(stream) = self.link.lock().expect("Bash link").stream.as_ref() {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

fn peer_pid(stream: &UnixStream) -> Option<u32> {
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = std::mem::size_of_val(&credentials) as libc::socklen_t;
    // SAFETY: SO_PEERCRED 写入本调用持有且大小匹配的 ucred。
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut length,
        )
    };
    (result == 0 && credentials.pid > 0 && credentials.uid == unsafe { libc::getuid() })
        .then_some(credentials.pid as u32)
}

fn read_packet(stream: &mut UnixStream, buffer: &mut Vec<u8>) -> std::io::Result<Option<Vec<u8>>> {
    if let Some(end) = buffer.iter().position(|byte| *byte == b'\n') {
        let mut packet: Vec<_> = buffer.drain(..=end).collect();
        packet.pop();
        return Ok(Some(packet));
    }
    let mut bytes = [0; 4096];
    match stream.read(&mut bytes) {
        Ok(0) => Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof)),
        Ok(count) if buffer.len() + count <= LIMIT => {
            buffer.extend_from_slice(&bytes[..count]);
            Ok(None)
        }
        Ok(_) => Err(std::io::Error::other("Bash 控制消息超限")),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock
                    | std::io::ErrorKind::TimedOut
                    | std::io::ErrorKind::Interrupted
            ) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn encode_path(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

fn decode_path(text: &str) -> Option<PathBuf> {
    if text.is_empty()
        || !text.len().is_multiple_of(2)
        || !text.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    let bytes: Vec<u8> = text
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok())
        .collect::<Option<_>>()?;
    if bytes.contains(&0) {
        return None;
    }
    let path = PathBuf::from(std::ffi::OsString::from_vec(bytes));
    path.is_absolute().then_some(path)
}

fn dispatch(inner: &Inner, link: &Mutex<Link>, packet: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(packet) else {
        return false;
    };
    let parts: Vec<_> = text.split(' ').collect();
    match parts.as_slice() {
        ["S", input_generation, ready, edit, path] => {
            let Ok(input_generation) = input_generation.parse::<u64>() else {
                return false;
            };
            let link = link.lock().expect("Bash link");
            if input_generation != link.input_generation {
                return true;
            }
            let ready = match *ready {
                "0" => false,
                "1" => true,
                _ => return false,
            };
            let (Ok(edit), Some(path)) = (edit.parse::<usize>(), decode_path(path)) else {
                return false;
            };
            if ready && edit != 0 {
                return false;
            }
            let mut state = inner.state.lock().expect("state");
            let cwd = path.to_string_lossy().into_owned();
            let changed = state.prompt_ready != ready || state.edit_len != edit || state.cwd != cwd;
            state.cwd = cwd.clone();
            let ready = ready && link.visible && !state.delivering;
            state.prompt_ready = ready;
            state.edit_len = edit;
            if ready {
                // 取消回执后的重复空闲帧仍须唤醒最新待处理请求。
                let _ = inner.events.send(TerminalEvent::PromptReady { cwd });
            } else if changed {
                let _ = inner.events.send(TerminalEvent::Preexec);
                let _ = inner.events.send(TerminalEvent::EditLine { len: edit });
            }
            true
        }
        ["D", serial, revision, outcome, path] => {
            let (Ok(serial), Ok(revision), Some(path)) = (
                serial.parse::<u64>(),
                revision.parse::<u64>(),
                decode_path(path),
            ) else {
                return false;
            };
            if !matches!(*outcome, "0" | "1" | "2") {
                return false;
            }
            let revision = Revision::new(revision);
            let mut link = link.lock().expect("Bash link");
            if link.pending != Some((serial, revision)) {
                return true;
            }
            link.pending = None;
            let mut state = inner.state.lock().expect("state");
            let pending = state.pending_cd.take();
            let cwd = path.to_string_lossy().into_owned();
            state.cwd = cwd.clone();
            state.prompt_ready = false;
            let event = if *outcome == "2" {
                TerminalEvent::CdCancelled { revision }
            } else {
                let ok = *outcome == "0"
                    && pending.is_some_and(|(target, expected)| {
                        expected == revision && super::same_directory(&target, &path)
                    });
                TerminalEvent::CdResult {
                    revision,
                    ok,
                    cwd,
                    message: (!ok).then(|| "Bash 未进入请求的目录，详情见终端输出".into()),
                }
            };
            let _ = inner.events.send(event);
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn another_process_cannot_claim_the_shell_control_channel() {
        let control = Control::new().unwrap();
        control.start(Weak::new(), u32::MAX).unwrap();
        let mut peer = UnixStream::connect(control.socket()).unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let _ = peer.write_all(b"H 1\n");
        let outcome = peer.read(&mut [0; 16]);
        assert!(
            matches!(outcome, Ok(0))
                || matches!(outcome, Err(ref error) if error.kind() == std::io::ErrorKind::ConnectionReset)
        );
        assert!(!control.connected.load(Ordering::Acquire));
    }
}
