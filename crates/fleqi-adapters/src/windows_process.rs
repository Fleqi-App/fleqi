//! Windows 一次性任务使用管道和 Job；持续交互终端仍使用 ConPTY。
use crate::process::{ProcessEvent, SpawnRequest, StreamKind};
use crate::windows_job::Job;
use std::io::Read;
use std::os::windows::{io::AsRawHandle, process::CommandExt};
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicU64, Ordering},
    mpsc::Sender,
};

pub struct ProcessRunner {
    pid: i32,
    job: Arc<Job>,
    status: Arc<(Mutex<Option<Option<i32>>>, Condvar)>,
}
impl ProcessRunner {
    pub fn spawn(request: SpawnRequest, events: Sender<ProcessEvent>) -> std::io::Result<Self> {
        use windows::Win32::System::Threading::{CREATE_NO_WINDOW, CREATE_SUSPENDED};
        let job = Arc::new(Job::new()?);
        let mut command = std::process::Command::new(&request.executable);
        command
            .args(&request.args)
            .current_dir(&request.cwd)
            .envs(request.env.clone())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW.0 | CREATE_SUSPENDED.0);
        let inherited = request
            .env
            .iter()
            .rev()
            .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))
            .map(|(_, value)| value.into())
            .unwrap_or_else(|| std::env::var_os("PATH").unwrap_or_default());
        command.env("PATH", crate::environment::executable_path(&inherited));
        let mut child = command.spawn()?;
        if let Err(error) = job
            .assign(child.as_raw_handle())
            .and_then(|_| crate::windows_job::resume(child.id()))
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        let pid = child.id() as i32;
        let seq = Arc::new(AtomicU64::new(0));
        let stdout = pump(
            child.stdout.take().expect("stdout pipe"),
            StreamKind::Stdout,
            events.clone(),
            seq.clone(),
        );
        let stderr = pump(
            child.stderr.take().expect("stderr pipe"),
            StreamKind::Stderr,
            events.clone(),
            seq,
        );
        let status = Arc::new((Mutex::new(None), Condvar::new()));
        let completed = status.clone();
        let owned_job = job.clone();
        std::thread::spawn(move || {
            let exit = child.wait().ok().and_then(|exit| exit.code());
            // 主进程结束后不让后台子进程持有输出管道或逃离本 Run。
            owned_job.terminate();
            let _ = stdout.join();
            let _ = stderr.join();
            *completed.0.lock().expect("status") = Some(exit);
            completed.1.notify_all();
            let _ = events.send(ProcessEvent::Exited { status: exit });
        });
        Ok(Self { pid, job, status })
    }
    pub fn pid(&self) -> i32 {
        self.pid
    }
    pub fn cancel(&mut self) {
        self.job.terminate();
    }
    pub fn cancellation(&self) -> Box<dyn Fn() + Send + Sync> {
        let job = self.job.clone();
        Box::new(move || job.terminate())
    }
    pub fn wait(&mut self) -> Option<i32> {
        let mut status = self.status.0.lock().expect("status");
        while status.is_none() {
            status = self.status.1.wait(status).expect("status");
        }
        status.expect("finished")
    }
}
impl Drop for ProcessRunner {
    fn drop(&mut self) {
        self.job.terminate();
        let _ = self.wait();
    }
}

fn pump(
    mut reader: impl Read + Send + 'static,
    stream: StreamKind,
    events: Sender<ProcessEvent>,
    seq: Arc<AtomicU64>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut bytes = [0; 8192];
        loop {
            match reader.read(&mut bytes) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if events
                        .send(ProcessEvent::Output {
                            stream,
                            bytes: bytes[..n].to_vec(),
                            seq: seq.fetch_add(1, Ordering::Relaxed) + 1,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    })
}
