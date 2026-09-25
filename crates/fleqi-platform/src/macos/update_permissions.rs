//! Reauthorize after an installed build changes, regardless of how it arrived.
//! Only this application's TCC records are reset. No credential or user-data store
//! is removed, and an unchanged build never repeats a successful reset.
use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
    process::Command,
};

const BUNDLE_ID: &str = "app.fleqi.desktop";
const MARKER: &str = "permission-build.txt";

fn enclosing_app(executable: &Path) -> Option<&Path> {
    let macos = executable.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    (macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && app.extension()? == "app")
        .then_some(app)
}

fn build_identity(app: &Path) -> Result<String, String> {
    let output = Command::new("/usr/bin/codesign")
        .args(["-dv", "--verbose=4"])
        .arg(app)
        .output()
        .map_err(|e| format!("无法读取安装版本签名：{e}"))?;
    if !output.status.success() {
        return Err("无法读取安装版本的签名标识".into());
    }
    let text = String::from_utf8_lossy(&output.stderr);
    if !text
        .lines()
        .any(|line| line == format!("Identifier={BUNDLE_ID}"))
    {
        return Err("安装包标识不匹配，未更改任何权限".into());
    }
    let hash = text
        .lines()
        .find_map(|line| line.strip_prefix("CDHash="))
        .filter(|hash| matches!(hash.len(), 40 | 64) && hash.bytes().all(|c| c.is_ascii_hexdigit()))
        .ok_or("安装包缺少有效的构建签名摘要")?;
    Ok(format!("{BUNDLE_ID}:{hash}"))
}

fn reset_tcc() -> Result<(), String> {
    let output = Command::new("/usr/bin/tccutil")
        .args(["reset", "All", BUNDLE_ID])
        .output()
        .map_err(|e| format!("无法重置本应用权限：{e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "macOS 权限重置失败：{}",
            String::from_utf8_lossy(&output.stderr)
                .trim()
                .chars()
                .take(240)
                .collect::<String>()
        ))
    }
}

fn reset_changed_build(
    data_dir: &Path,
    identity: &str,
    reset: impl FnOnce() -> Result<(), String>,
) -> Result<bool, String> {
    let marker = data_dir.join(MARKER);
    match std::fs::read_to_string(&marker) {
        Ok(previous) if previous.trim() == identity => return Ok(false),
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(format!("无法读取权限重置记录：{error}")),
    }
    reset()?;
    std::fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
    let temporary = data_dir.join(".permission-build.pending");
    std::fs::write(&temporary, format!("{identity}\n"))
        .map_err(|e| format!("无法保存权限重置记录：{e}"))?;
    std::fs::rename(&temporary, &marker).map_err(|e| format!("无法发布权限重置记录：{e}"))?;
    Ok(true)
}

pub fn reset_for_installed_build(data_dir: &Path) -> Result<bool, String> {
    let executable: PathBuf = std::env::current_exe().map_err(|e| e.to_string())?;
    let Some(app) = enclosing_app(&executable) else {
        return Ok(false);
    };
    let identity = build_identity(app)?;
    reset_changed_build(data_dir, &identity, reset_tcc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn resets_once_for_a_new_build_and_again_after_a_real_change() {
        let root = tempfile::tempdir().unwrap();
        let calls = Cell::new(0);
        let reset = || {
            calls.set(calls.get() + 1);
            Ok(())
        };
        assert!(reset_changed_build(root.path(), "build-a", reset).unwrap());
        assert!(!reset_changed_build(root.path(), "build-a", reset).unwrap());
        assert!(reset_changed_build(root.path(), "build-b", reset).unwrap());
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn failed_reset_preserves_previous_record_and_can_retry() {
        let root = tempfile::tempdir().unwrap();
        reset_changed_build(root.path(), "previous", || Ok(())).unwrap();
        assert!(reset_changed_build(root.path(), "next", || Err("refused".into())).is_err());
        assert_eq!(
            std::fs::read_to_string(root.path().join(MARKER))
                .unwrap()
                .trim(),
            "previous"
        );
        assert!(reset_changed_build(root.path(), "next", || Ok(())).unwrap());
    }

    #[test]
    fn development_executables_are_not_installed_updates() {
        assert!(enclosing_app(Path::new("/repo/target/debug/fleqi-desktop")).is_none());
        assert_eq!(
            enclosing_app(Path::new(
                "/Applications/Fleqi.app/Contents/MacOS/fleqi-desktop"
            )),
            Some(Path::new("/Applications/Fleqi.app"))
        );
    }
}
