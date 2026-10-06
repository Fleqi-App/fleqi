//! 每个任务/终端独占 Job；退出或取消只回收本任务拥有的进程树。
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use windows::Win32::{
    Foundation::HANDLE,
    System::{Diagnostics::ToolHelp::*, JobObjects::*, Threading::*},
};

pub(crate) struct Job(OwnedHandle);
impl Job {
    pub(crate) fn new() -> std::io::Result<Self> {
        // SAFETY: Job 由 OwnedHandle 唯一关闭，调用期间句柄有效。
        unsafe {
            let handle = CreateJobObjectW(None, None).map_err(io)?;
            let job = Self(OwnedHandle::from_raw_handle(handle.0));
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                std::mem::size_of_val(&limits) as u32,
            )
            .map_err(io)?;
            Ok(job)
        }
    }
    pub(crate) fn assign(&self, process: RawHandle) -> std::io::Result<()> {
        unsafe { AssignProcessToJobObject(HANDLE(self.0.as_raw_handle()), HANDLE(process)) }
            .map_err(io)
    }
    pub(crate) fn terminate(&self) {
        let _ = unsafe { TerminateJobObject(HANDLE(self.0.as_raw_handle()), 1) };
    }
}
fn io(error: windows::core::Error) -> std::io::Error {
    std::io::Error::other(error.to_string())
}

/// std::process 保留进程句柄；挂起启动时只有一个初始线程，在加入 Job 后恢复。
pub(crate) fn resume(pid: u32) -> std::io::Result<()> {
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0).map_err(io)?;
        let _snapshot = OwnedHandle::from_raw_handle(snapshot.0);
        let mut entry = THREADENTRY32 {
            dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        Thread32First(snapshot, &mut entry).map_err(io)?;
        loop {
            if entry.th32OwnerProcessID == pid {
                let handle =
                    OpenThread(THREAD_SUSPEND_RESUME, false, entry.th32ThreadID).map_err(io)?;
                let _thread = OwnedHandle::from_raw_handle(handle.0);
                if ResumeThread(handle) == u32::MAX {
                    return Err(std::io::Error::last_os_error());
                }
                return Ok(());
            }
            if Thread32Next(snapshot, &mut entry).is_err() {
                break;
            }
        }
    }
    Err(std::io::Error::other("无法找到本任务的挂起线程"))
}
