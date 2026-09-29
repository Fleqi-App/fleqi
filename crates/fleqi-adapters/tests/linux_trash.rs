//! Linux FreeDesktop 回收站：移入后原路径消失，恢复后文件回来。
#![cfg(target_os = "linux")]

use fleqi_adapters::capabilities::FileCapabilities;
use std::path::PathBuf;

#[test]
fn linux_trash_roundtrip_uses_freedesktop_directory() {
    let home = PathBuf::from(std::env::var("HOME").expect("HOME"));
    let root = home.join(format!(".cache/fleqi-trash-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let source = root.join("note.txt");
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
    let _ = std::fs::remove_dir_all(&root);
}
