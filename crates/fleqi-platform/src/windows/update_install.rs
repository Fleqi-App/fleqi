//! Windows 安装包（MSIX/MSI）与 macOS `.app` 归档分开处理。
//! 这里不替换已安装的应用，也不把归档解压到磁盘。

use std::io::Cursor;
use std::path::Component;

pub fn install_verified(bytes: &[u8], version: &str) -> Result<(), String> {
    if bytes.is_empty() {
        return Err(format!(
            "更新载荷为空（版本 {version}）。Windows 打包（MSIX/MSI）与 macOS .app 归档分开处理，当前安装保持不变。"
        ));
    }
    if macos_app_archive(bytes) {
        return Err(format!(
            "版本 {version} 的载荷是 macOS 应用包 Fleqi.app，不是 Windows 安装包（MSIX/MSI）。当前安装保持不变。"
        ));
    }
    Err(format!(
        "版本 {version} 的 Windows 打包（MSIX/MSI）与 macOS .app 归档分开处理，当前安装保持不变。"
    ))
}

fn macos_app_archive(bytes: &[u8]) -> bool {
    if bytes.len() < 2 || bytes[0] != 0x1f || bytes[1] != 0x8b {
        return false;
    }
    let decoder = flate2::read::GzDecoder::new(Cursor::new(bytes));
    let mut archive = tar::Archive::new(decoder);
    let Ok(mut entries) = archive.entries() else {
        return false;
    };
    let Some(entry) = entries.next() else {
        return false;
    };
    let Ok(entry) = entry else {
        return false;
    };
    let Ok(path) = entry.path() else {
        return false;
    };
    matches!(
        path.components().next(),
        Some(Component::Normal(name)) if name == "Fleqi.app"
    )
}
