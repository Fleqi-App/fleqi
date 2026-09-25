//! Install a signature-verified update on the destination filesystem, retaining
//! the previous bundle until replacement succeeds. No elevated shell commands.
use std::{
    io::Cursor,
    path::{Component, Path},
    process::Command,
};

pub fn install_verified(bytes: &[u8], expected_version: &str) -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let current = executable
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .filter(|path| path.extension().is_some_and(|extension| extension == "app"))
        .ok_or("请使用安装在应用程序文件夹中的 Fleqi 更新")?;
    install_at(bytes, current, |staged| {
        validate_bundle(staged, expected_version)
    })
}

fn validate_bundle(bundle: &Path, version: &str) -> Result<(), String> {
    let verified = Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(bundle)
        .output()
        .map_err(|error| error.to_string())?;
    if !verified.status.success() {
        return Err("新应用的代码签名无效，已保留原版本".into());
    }
    for (key, expected) in [
        ("CFBundleIdentifier", "app.fleqi.desktop"),
        ("CFBundleShortVersionString", version),
    ] {
        let value = Command::new("/usr/bin/plutil")
            .args(["-extract", key, "raw", "-o", "-"])
            .arg(bundle.join("Contents/Info.plist"))
            .output()
            .map_err(|error| error.to_string())?;
        if !value.status.success() || String::from_utf8_lossy(&value.stdout).trim() != expected {
            return Err(format!("更新包的 {key} 不匹配，已保留原版本"));
        }
    }
    Ok(())
}

fn install_at(
    bytes: &[u8],
    current: &Path,
    validate: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<(), String> {
    install_with(bytes, current, validate, |source, target| {
        std::fs::rename(source, target)
    })
}

fn install_with(
    bytes: &[u8],
    current: &Path,
    validate: impl FnOnce(&Path) -> Result<(), String>,
    publish: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
) -> Result<(), String> {
    let parent = current.parent().ok_or("应用路径无效")?;
    let staging = tempfile::Builder::new()
        .prefix(".fleqi-update-")
        .tempdir_in(parent)
        .map_err(|error| format!("无法写入应用所在文件夹，请手动安装更新：{error}"))?;
    let extracted = staging.path().join("Fleqi.app");
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(Cursor::new(bytes)));
    let mut size = 0u64;
    for entry in archive.entries().map_err(|error| error.to_string())? {
        let mut entry = entry.map_err(|error| error.to_string())?;
        let path = entry
            .path()
            .map_err(|error| error.to_string())?
            .into_owned();
        let mut parts = path.components();
        let root = parts.next();
        if root != Some(Component::Normal(std::ffi::OsStr::new("Fleqi.app")))
            || !parts.all(|part| matches!(part, Component::Normal(_)))
            || !(entry.header().entry_type().is_file() || entry.header().entry_type().is_dir())
        {
            return Err("更新包包含不安全的路径或链接，已保留原版本".into());
        }
        size = size.checked_add(entry.size()).ok_or("更新包过大")?;
        if size > 512 * 1024 * 1024 {
            return Err("更新包解压大小超出限制".into());
        }
        entry
            .unpack_in(staging.path())
            .map_err(|error| error.to_string())?;
    }
    if !extracted.join("Contents/MacOS/fleqi-desktop").is_file() {
        return Err("更新包缺少应用程序".into());
    }
    validate(&extracted)?;
    let backup = staging.path().join("previous.app");
    std::fs::rename(current, &backup)
        .map_err(|error| format!("无法替换当前应用，原版本已保留：{error}"))?;
    if let Err(error) = publish(&extracted, current) {
        if let Err(restore_error) = std::fs::rename(&backup, current) {
            let recovery = staging.keep();
            return Err(format!(
                "更新失败：{error}；恢复失败：{restore_error}。原版本保留在 {}",
                recovery.join("previous.app").display()
            ));
        }
        return Err(format!("更新失败，已恢复原版本：{error}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn archive(link: bool) -> Vec<u8> {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut archive = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_mode(0o755);
        header.set_size(if link { 0 } else { 3 });
        if link {
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_link_name("/tmp/escape").unwrap();
        }
        header.set_cksum();
        archive
            .append_data(
                &mut header,
                "Fleqi.app/Contents/MacOS/fleqi-desktop",
                if link { &b""[..] } else { &b"new"[..] },
            )
            .unwrap();
        archive.into_inner().unwrap().finish().unwrap()
    }
    #[test]
    fn validated_bundle_replaces_old_and_failed_validation_preserves_it() {
        let parent = tempfile::tempdir().unwrap();
        let current = parent.path().join("Fleqi.app");
        std::fs::create_dir(&current).unwrap();
        std::fs::write(current.join("old"), "old").unwrap();
        assert!(install_at(&archive(false), &current, |_| Err("wrong signature".into())).is_err());
        assert_eq!(std::fs::read_to_string(current.join("old")).unwrap(), "old");
        assert!(install_at(&archive(true), &current, |_| Ok(())).is_err());
        assert!(current.join("old").exists());
        let failed = install_with(
            &archive(false),
            &current,
            |_| Ok(()),
            |_, _| Err(std::io::Error::other("simulated publish failure")),
        );
        assert!(failed.unwrap_err().contains("已恢复原版本"));
        assert_eq!(std::fs::read_to_string(current.join("old")).unwrap(), "old");
        install_at(&archive(false), &current, |_| Ok(())).unwrap();
        assert_eq!(
            std::fs::read(current.join("Contents/MacOS/fleqi-desktop")).unwrap(),
            b"new"
        );
        assert!(!current.join("old").exists());
        assert_eq!(std::fs::read_dir(parent.path()).unwrap().count(), 1);
    }

    #[test]
    #[ignore = "requires a locally built, signed release archive"]
    fn real_release_archive_installs_in_isolated_directory() {
        let archive = std::env::var_os("FLEQI_UPDATE_PACKAGE").expect("release archive path");
        let version = std::env::var("FLEQI_UPDATE_VERSION").expect("release version");
        let parent = tempfile::tempdir().unwrap();
        let current = parent.path().join("Fleqi.app");
        std::fs::create_dir(&current).unwrap();
        std::fs::write(current.join("old-version"), "old").unwrap();
        let bytes = std::fs::read(archive).unwrap();
        install_at(&bytes, &current, |staged| validate_bundle(staged, &version)).unwrap();
        assert!(!current.join("old-version").exists());
        validate_bundle(&current, &version).unwrap();
    }
}
