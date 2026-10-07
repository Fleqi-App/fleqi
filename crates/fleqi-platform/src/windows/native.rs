//! COM 对象留在创建它们的 STA；跨线程只传拥有的路径和状态。
use std::path::PathBuf;
use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize,
};
use windows::Win32::UI::Shell::{IShellItem, SIGDN_FILESYSPATH};

pub(super) struct Apartment;
impl Apartment {
    pub(super) fn new() -> windows::core::Result<Self> {
        // SAFETY: 仅在本模块拥有的专用线程上初始化、释放 COM。
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        }
        Ok(Self)
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() }
    }
}

pub(super) fn item_path(item: &IShellItem) -> windows::core::Result<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    // SAFETY: Shell 分配的字符串在复制原生 UTF-16 后释放，不以显示字符串往返路径。
    unsafe {
        let raw = item.GetDisplayName(SIGDN_FILESYSPATH)?;
        let path = PathBuf::from(std::ffi::OsString::from_wide(raw.as_wide()));
        CoTaskMemFree(Some(raw.0.cast()));
        Ok(path)
    }
}
