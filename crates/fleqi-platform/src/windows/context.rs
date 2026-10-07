//! Explorer 的当前窗口和活动标签页；COM 读取在单一 STA 上执行。
use crate::scheduling::MainThreadExecutor;
use fleqi_application::ports::{ContextPort, DirectoryPick, RawContext};
use fleqi_domain::context::{ContextAvailability, ContextSource};
use std::sync::Arc;

pub struct WindowsContextPort {
    main: Arc<dyn MainThreadExecutor>,
    #[cfg(windows)]
    requests: std::sync::mpsc::SyncSender<std::sync::mpsc::Sender<RawContext>>,
}

impl WindowsContextPort {
    pub fn new(main: Arc<dyn MainThreadExecutor>) -> Self {
        #[cfg(windows)]
        {
            let (requests, receive) =
                std::sync::mpsc::sync_channel::<std::sync::mpsc::Sender<RawContext>>(1);
            std::thread::spawn(move || {
                let apartment = super::native::Apartment::new();
                for reply in receive {
                    let context = match &apartment {
                        Ok(_) => capture_explorer().unwrap_or_else(failed),
                        Err(error) => failed(format!("Explorer COM 初始化失败：{error}")),
                    };
                    let _ = reply.send(context);
                }
            });
            Self { main, requests }
        }
        #[cfg(not(windows))]
        Self { main }
    }
}

fn failed(message: String) -> RawContext {
    RawContext {
        unavailable: Some(ContextAvailability::Failed { message }),
        ..RawContext::default()
    }
}

impl ContextPort for WindowsContextPort {
    fn source(&self) -> ContextSource {
        ContextSource::Explorer
    }
    fn capture(&self) -> RawContext {
        #[cfg(windows)]
        {
            let (reply, receive) = std::sync::mpsc::channel();
            if self.requests.try_send(reply).is_err() {
                return failed("Explorer 正忙，请稍后重试".into());
            }
            receive
                .recv_timeout(std::time::Duration::from_secs(3))
                .unwrap_or_else(|_| failed("读取 Explorer 超时，请稍后重试".into()))
        }
        #[cfg(not(windows))]
        failed("资源管理器仅在 Windows 上读取".into())
    }
    fn pick_directory(&self) -> DirectoryPick {
        super::picker::pick_directory_blocking(self.main.as_ref())
    }
}

#[cfg(windows)]
fn capture_explorer() -> Result<RawContext, String> {
    use fleqi_application::ports::RawPath;
    use fleqi_domain::context::PathKind;
    use windows::Win32::System::Com::{
        CLSCTX_ALL, CoCreateInstance, CoTaskMemFree, IServiceProvider,
    };
    use windows::Win32::System::Variant::VARIANT;
    use windows::Win32::UI::Shell::*;
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowExW, GA_ROOT, GetAncestor, IsWindowVisible,
    };
    use windows::core::{Interface, w};
    let target = super::surface::explorer_window()
        .ok_or("没有可绑定的资源管理器窗口，请打开文件夹或手动选择目录")?;
    let mut candidates = Vec::new();
    // SAFETY: 所有 COM 接口在本 STA 内使用；只导出路径值，不保留原生指针。
    unsafe {
        // Windows 11 会保留非活动 view 的 WS_VISIBLE；活动 ShellTab 位于子窗口 Z 序最前。
        // 必须匹配该标签的 IShellBrowser，不能按 ShellWindows 的枚举顺序选择目录。
        let active_tab = FindWindowExW(Some(target), None, w!("ShellTabWindowClass"), None)
            .map_err(|_| "无法识别资源管理器的活动标签页".to_string())?;
        let windows: IShellWindows =
            CoCreateInstance(&ShellWindows, None, CLSCTX_ALL).map_err(|e| e.to_string())?;
        for index in 0..windows.Count().map_err(|e| e.to_string())? {
            let candidate = (|| -> windows::core::Result<_> {
                let dispatch = windows.Item(&VARIANT::from(index))?;
                let browser: IWebBrowserApp = dispatch.cast()?;
                if browser.HWND()?.0 != target.0 as isize {
                    return Ok(None);
                }
                let provider: IServiceProvider = dispatch.cast()?;
                let shell: IShellBrowser = provider.QueryService(&SID_STopLevelBrowser)?;
                if shell.GetWindow()? != active_tab {
                    return Ok(None);
                }
                let view = shell.QueryActiveShellView()?;
                let view_window = view.GetWindow()?;
                if !IsWindowVisible(view_window).as_bool()
                    || GetAncestor(view_window, GA_ROOT) != target
                {
                    return Ok(None);
                }
                let folder: IFolderView2 = view.cast()?;
                let persisted: IPersistFolder2 = folder.GetFolder()?;
                let pidl = persisted.GetCurFolder()?;
                let item: windows::core::Result<IShellItem> = SHCreateItemFromIDList(pidl);
                CoTaskMemFree(Some(pidl.cast()));
                let directory = super::native::item_path(&item?)?;
                let selected = folder.GetSelection(false)?;
                let count = selected.GetCount()?;
                if count as usize > fleqi_domain::settings::limits::SELECTION_ITEMS {
                    return Err(windows::core::Error::new(
                        windows::core::HRESULT(0x80070057_u32 as i32),
                        "选区超过 1000 项，请缩小范围",
                    ));
                }
                let mut selection = Vec::new();
                for index in 0..count {
                    let native = super::native::item_path(&selected.GetItemAt(index)?)?;
                    let kind = if native.is_dir() {
                        PathKind::Directory
                    } else {
                        PathKind::File
                    };
                    selection.push(RawPath { native, kind });
                }
                Ok(Some((view_window, directory, selection)))
            })();
            match candidate {
                Ok(Some(value)) => candidates.push(value),
                Ok(None) => (),
                Err(error) => {
                    // 非文件系统视图也必须拒绝，不能回退成其它窗口的目录。
                    if let Ok(dispatch) = windows.Item(&VARIANT::from(index))
                        && let Ok(browser) = dispatch.cast::<IWebBrowserApp>()
                        && browser.HWND().is_ok_and(|hwnd| hwnd.0 == target.0 as isize)
                    {
                        return Err(format!("无法完整读取当前 Explorer 目录或选区：{error}"));
                    }
                }
            }
        }
        if FindWindowExW(Some(target), None, w!("ShellTabWindowClass"), None).ok()
            != Some(active_tab)
        {
            return Err("读取期间活动标签页发生变化，请重试".into());
        }
    }
    if candidates.len() != 1 {
        return Err("无法唯一确定资源管理器的活动标签页，请重新选择文件夹后重试".into());
    }
    let (_view, directory, selection) = candidates.pop().expect("one candidate");
    if !directory.is_dir() {
        return Err("当前资源管理器位置不是可访问的文件夹".into());
    }
    Ok(RawContext {
        source_window_id: Some(target.0 as u64),
        directory: Some(fleqi_application::ports::RawPath {
            native: directory,
            kind: fleqi_domain::context::PathKind::Directory,
        }),
        selection,
        view_kind: Some(fleqi_domain::context::ViewKind::Physical),
        unavailable: None,
    })
}
