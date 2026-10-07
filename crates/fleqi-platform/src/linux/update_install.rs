//! Linux 更新安装。
//!
//! 不解压、不替换已安装的应用。macOS 的 `Fleqi.app` tar.gz 与空载荷都会拒绝，
//! 其它字节同样不会被安装。检测只读取内存中的 gzip/tar 头。

use std::io::Cursor;
use std::path::{Component, Path};

pub fn install_verified(bytes: &[u8], version: &str) -> Result<(), String> {
    rejects_empty_or_macos_bundle(bytes, version)?;
    Err(left_in_place(version))
}

/// 空载荷与根目录为 `Fleqi.app` 的 macOS 包返回错误。
/// 其它输入返回 `Ok`，表示“不是这两种需要点名的载荷”；调用方仍不得安装。
pub fn rejects_empty_or_macos_bundle(bytes: &[u8], version: &str) -> Result<(), String> {
    if bytes.is_empty() {
        return Err(format!(
            "版本 {version} 的更新载荷为空。Linux 软件包格式不是 macOS 应用包，当前安装保持原样。"
        ));
    }
    match gzip_tar_root(bytes) {
        Ok(Some(root)) if root == "Fleqi.app" => Err(format!(
            "版本 {version} 是 macOS 应用包 Fleqi.app。Linux 软件包格式不是 macOS 应用包，当前安装保持原样。"
        )),
        Ok(_) => Ok(()),
        Err(message) => Err(format!("版本 {version}：{message}")),
    }
}

fn left_in_place(version: &str) -> String {
    format!(
        "版本 {version} 无法安装：Linux 软件包格式不是 macOS 应用包（Fleqi.app）。当前安装保持原样。"
    )
}

/// `Ok(Some(name))` 是第一个安全相对路径的根名称。无法识别为 gzip tar 时返回 `Ok(None)`。
fn gzip_tar_root(bytes: &[u8]) -> Result<Option<String>, String> {
    let decoder = flate2::read::GzDecoder::new(Cursor::new(bytes));
    let mut archive = tar::Archive::new(decoder);
    let entries = match archive.entries() {
        Ok(entries) => entries,
        Err(_) => return Ok(None),
    };
    let mut root = None;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => return Ok(root),
        };
        let path = match entry.path() {
            Ok(path) => path.into_owned(),
            Err(_) => return Ok(root),
        };
        if !safe_archive_path(&path) {
            return Err("更新包包含不安全的路径，当前安装保持原样。".into());
        }
        if root.is_none()
            && let Some(name) = first_normal_component(&path)
        {
            root = Some(name.to_string_lossy().into_owned());
        }
    }
    Ok(root)
}

fn safe_archive_path(path: &Path) -> bool {
    path.components()
        .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

fn first_normal_component(path: &Path) -> Option<&std::ffi::OsStr> {
    path.components().find_map(|component| match component {
        Component::Normal(name) => Some(name),
        _ => None,
    })
}
