//! Linux FreeDesktop 回收站：移入后原路径消失，恢复后文件回来。
#![cfg(target_os = "linux")]

use fleqi_adapters::capabilities::FileCapabilities;
use std::path::PathBuf;

#[test]
fn linux_trash_rejects_symlinks_without_moving_their_targets() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("file.txt");
    let folder = root.path().join("folder");
    std::fs::write(&file, b"keep-file").unwrap();
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("child.txt"), b"keep-child").unwrap();
    for (name, target) in [("file-link", &file), ("folder-link", &folder)] {
        let link = root.path().join(name);
        std::os::unix::fs::symlink(target, &link).unwrap();
        let report = FileCapabilities.trash(vec![link.clone()]);
        assert_eq!(report.succeeded, 0, "{report:?}");
        assert!(
            report.items[0]
                .message
                .as_ref()
                .unwrap()
                .contains("符号链接")
        );
        assert_eq!(std::fs::read_link(link).unwrap(), *target);
    }
    assert_eq!(std::fs::read(file).unwrap(), b"keep-file");
    assert_eq!(
        std::fs::read(folder.join("child.txt")).unwrap(),
        b"keep-child"
    );
}

#[test]
fn linux_trash_roundtrip_uses_freedesktop_directory() {
    // /tmp 可能是 tmpfs；正常往返样本必须与回收站位于同一文件系统。
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap()).join(".local/share"));
    std::fs::create_dir_all(&data).unwrap();
    let root = tempfile::tempdir_in(&data).unwrap();
    let source = root.path().join(format!(
        "fleqi-trash-{}.txt",
        root.path().file_name().unwrap().to_string_lossy()
    ));
    std::fs::write(&source, b"keep-me").unwrap();
    let report = FileCapabilities.trash(vec![source.clone()]);
    assert_eq!(report.succeeded, 1, "{report:?}");
    assert!(!source.exists());
    let restored = report
        .restore_paths
        .first()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("missing"));
    assert!(
        restored
            .components()
            .any(|part| part.as_os_str() == "Trash"),
        "{}",
        restored.display()
    );
    FileCapabilities
        .restore_from_trash(&restored, &source)
        .expect("restore");
    assert_eq!(std::fs::read(&source).unwrap(), b"keep-me");
}
