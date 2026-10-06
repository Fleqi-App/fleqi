//! 原生文件夹选择器由独立 STA 承载；等待用户选择不会阻塞 Tauri 主线程。
use crate::scheduling::MainThreadExecutor;
use fleqi_application::ports::DirectoryPick;

pub fn pick_directory_blocking(_main: &dyn MainThreadExecutor) -> DirectoryPick {
    #[cfg(windows)]
    {
        std::thread::spawn(pick)
            .join()
            .unwrap_or_else(|_| DirectoryPick::Failed("文件夹对话框线程异常".into()))
    }
    #[cfg(not(windows))]
    DirectoryPick::Failed("文件夹对话框仅在 Windows 上打开".into())
}

#[cfg(windows)]
fn pick() -> DirectoryPick {
    use fleqi_application::ports::RawPath;
    use fleqi_domain::context::PathKind;
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
    use windows::Win32::UI::Shell::{
        FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, FileOpenDialog,
        IFileOpenDialog,
    };
    let result = (|| -> windows::core::Result<_> {
        let _apartment = super::native::Apartment::new()?;
        // SAFETY: 对话框与 Shell 对象全部在当前 STA 创建和释放。
        unsafe {
            let dialog: IFileOpenDialog =
                CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
            dialog.SetOptions(
                FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_NOCHANGEDIR,
            )?;
            dialog.Show(None)?;
            super::native::item_path(&dialog.GetResult()?)
        }
    })();
    match result {
        Ok(native) if native.is_dir() => DirectoryPick::Selected(RawPath {
            native,
            kind: PathKind::Directory,
        }),
        Ok(_) => DirectoryPick::Failed("所选文件夹已不存在".into()),
        Err(error) if error.code().0 as u32 == 0x800704c7 => DirectoryPick::Cancelled,
        Err(error) => DirectoryPick::Failed(format!("无法选择文件夹：{error}")),
    }
}
