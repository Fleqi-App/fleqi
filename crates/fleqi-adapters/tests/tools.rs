//! M3.3 工具设施测试（真实回环 HTTP + 真实 zip + 真实进程检测）：
//! 系统探测、staging 下载校验、安全解压、原子发布、取消/坏包保留旧版本、所有权卸载。

use fleqi_adapters::process::ProcessRunnerPort;
use fleqi_adapters::tools::ToolManager;
use fleqi_application::ports::{InstallProgress, ProcessPort, ToolFacility};
use fleqi_domain::tools::{
    InstalledTool, ToolManifest, ToolOwner, ToolSource, ToolStatus, builtin_tool_manifests,
};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tempfile::TempDir;

/// 回环 HTTP 服务：一次连接一个响应。trickle 模式先写首个分块再按间隔慢速发送，
/// 用于取消路径（客户端在分块之间检查取消标志）。
fn spawn_server(body: Vec<u8>, trickle: Option<Duration>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            // 读掉请求头即可（GET 无 body）。
            let mut buffer = [0u8; 1024];
            let _ = stream.read(&mut buffer);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\nContent-Type: application/zip\r\n\r\n",
                body.len()
            );
            if stream.write_all(head.as_bytes()).is_err() {
                continue;
            }
            if let Some(interval) = trickle {
                let mut sent = 0;
                let chunk = 4096;
                while sent < body.len() {
                    let end = (sent + chunk).min(body.len());
                    if stream.write_all(&body[sent..end]).is_err() {
                        break;
                    }
                    let _ = stream.flush();
                    sent = end;
                    std::thread::sleep(interval);
                }
            } else {
                let _ = stream.write_all(&body);
            }
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    });
    format!("http://127.0.0.1:{port}")
}

/// 构造包含可执行脚本的 zip 包：bin/<name> 打印版本并退出 0。
fn tool_zip(name: &str, version_line: &str) -> Vec<u8> {
    let script = format!("#!/bin/sh\necho {version_line}\n");
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut cursor);
        let options: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().unix_permissions(0o755);
        zip.start_file(format!("pkg/bin/{name}"), options).unwrap();
        zip.write_all(script.as_bytes()).unwrap();
        zip.finish().unwrap();
    }
    cursor.into_inner()
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn manifest_for(id: &str, url: &str, sha256: &str) -> ToolManifest {
    ToolManifest {
        id: id.into(),
        version: "1.0".into(),
        platform: "macos".into(),
        arch: "any".into(),
        executable: format!("bin/{id}"),
        source: ToolSource::Managed {
            url: url.into(),
            sha256: sha256.into(),
        },
        license: Some("MIT".into()),
        capabilities: vec!["cap.demo".into()],
        detection_args: vec![],
    }
}

fn new_manager(root: &Path) -> ToolManager {
    ToolManager::new(root, Arc::new(ProcessRunnerPort) as Arc<dyn ProcessPort>)
}

#[test]
fn system_tool_detected_via_real_process() {
    let root = TempDir::new().unwrap();
    let manager = new_manager(root.path());
    let git = builtin_tool_manifests()
        .into_iter()
        .find(|manifest| manifest.id == "git")
        .unwrap();
    let status = manager.detect(&git);
    let ToolStatus::Available {
        version,
        owner,
        path,
    } = status
    else {
        panic!("本机应能探测到 git：{status:?}");
    };
    assert_eq!(owner, ToolOwner::System);
    assert!(version.starts_with("git version"), "版本输出：{version}");
    assert!(path.contains("git"));
}

#[test]
fn managed_install_downloads_verifies_publishes_and_detects() {
    let root = TempDir::new().unwrap();
    let manager = new_manager(root.path());
    let body = tool_zip("demo-tool", "demo-tool 1.0");
    let base = spawn_server(body.clone(), None);
    let manifest = manifest_for(
        "demo-tool",
        &format!("{base}/demo-tool.zip"),
        &sha256_hex(&body),
    );
    let cancel = AtomicBool::new(false);
    let stages = std::sync::Mutex::new(Vec::new());
    let status = manager
        .install(&manifest, &cancel, &|progress| {
            stages.lock().unwrap().push(progress);
        })
        .expect("安装应成功");
    let stages = stages.into_inner().unwrap();
    // 进度合同（FR-TOOLS-002/AC-COMMON-008）：下载有字节量，阶段边界完整。
    assert!(
        stages
            .iter()
            .any(|p| matches!(p, InstallProgress::Download { bytes, .. } if *bytes > 0))
    );
    assert!(stages.contains(&InstallProgress::Verifying));
    assert!(stages.contains(&InstallProgress::Extracting));
    assert!(stages.contains(&InstallProgress::Publishing));
    let ToolStatus::Available {
        version,
        owner,
        path,
    } = status
    else {
        panic!("安装后应可用：{status:?}");
    };
    assert_eq!(version, "demo-tool 1.0");
    assert_eq!(owner, ToolOwner::Fleqi);
    assert!(path.contains("demo-tool"));
    // 发布目录有所有权标记；staging 清理干净。
    assert!(
        root.path()
            .join("demo-tool")
            .join("fleqi-tool.json")
            .exists()
    );
    assert!(!root.path().join(".staging.demo-tool").exists());
    // 重新探测走正式安装目录。
    assert!(matches!(
        manager.detect(&manifest),
        ToolStatus::Available { .. }
    ));
}

#[test]
fn bad_checksum_rejected_and_old_version_kept() {
    let root = TempDir::new().unwrap();
    let manager = new_manager(root.path());

    let v1 = tool_zip("demo-tool", "demo-tool 1.0");
    let base = spawn_server(v1.clone(), None);
    let manifest_v1 = manifest_for(
        "demo-tool",
        &format!("{base}/demo-tool.zip"),
        &sha256_hex(&v1),
    );
    let cancel = AtomicBool::new(false);
    assert!(manager.install(&manifest_v1, &cancel, &|_| {}).is_ok());

    // 第二轮：正确下载新包但清单校验值错误 → 拒绝；1.0 仍可用。
    let v2 = tool_zip("demo-tool", "demo-tool 2.0");
    let base2 = spawn_server(v2.clone(), None);
    let bad = manifest_for(
        "demo-tool",
        &format!("{base2}/demo-tool.zip"),
        &"0".repeat(64),
    );
    let error = manager
        .install(&bad, &cancel, &|_| {})
        .expect_err("校验失败应拒绝");
    assert!(error.contains("校验失败"), "错误信息：{error}");
    assert!(!root.path().join(".staging.demo-tool").exists());
    let status = manager.detect(&bad);
    let ToolStatus::Available { version, .. } = status else {
        panic!("旧版本应保留：{status:?}");
    };
    assert_eq!(version, "demo-tool 1.0");
}

#[test]
fn zip_slip_entries_rejected() {
    let root = TempDir::new().unwrap();
    let manager = new_manager(root.path());
    // 恶意包：条目名携带 .. 逃逸。
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut cursor);
        let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
        zip.start_file("../evil.txt", options).unwrap();
        zip.write_all(b"escaped").unwrap();
        zip.finish().unwrap();
    }
    let body = cursor.into_inner();
    let base = spawn_server(body.clone(), None);
    let manifest = manifest_for("evil-tool", &format!("{base}/evil.zip"), &sha256_hex(&body));
    let cancel = AtomicBool::new(false);
    let error = manager
        .install(&manifest, &cancel, &|_| {})
        .expect_err("越界条目应拒绝");
    assert!(
        error.contains("解压失败") || error.contains("越界"),
        "错误信息：{error}"
    );
    assert!(!root.path().join(".staging.evil-tool").exists());
    assert!(!root.path().join("evil.txt").exists());
}

#[test]
fn cancel_mid_download_keeps_previous_version() {
    let root = TempDir::new().unwrap();
    let manager = new_manager(root.path());

    let v1 = tool_zip("demo-tool", "demo-tool 1.0");
    let base = spawn_server(v1.clone(), None);
    let manifest_v1 = manifest_for(
        "demo-tool",
        &format!("{base}/demo-tool.zip"),
        &sha256_hex(&v1),
    );
    let cancel = AtomicBool::new(false);
    assert!(manager.install(&manifest_v1, &cancel, &|_| {}).is_ok());

    // 慢速服务（4KB/50ms）：取消在分块之间生效。
    let big: Vec<u8> = vec![7u8; 512 * 1024];
    let base2 = spawn_server(big.clone(), Some(Duration::from_millis(50)));
    let manifest_v2 = manifest_for("demo-tool", &format!("{base2}/big.zip"), &sha256_hex(&big));
    let flag = Arc::new(AtomicBool::new(false));
    let installer = {
        let manager2 = new_manager(root.path());
        let flag = Arc::clone(&flag);
        let manifest_v2 = manifest_v2.clone();
        std::thread::spawn(move || manager2.install(&manifest_v2, &flag, &|_| {}))
    };
    std::thread::sleep(Duration::from_millis(300));
    flag.store(true, Ordering::Relaxed);
    let error = installer.join().unwrap().expect_err("取消应中断安装");
    assert!(
        error.contains("取消") || error.contains("下载失败"),
        "错误信息：{error}"
    );
    assert!(!root.path().join(".staging.demo-tool").exists());
    let status = manager.detect(&manifest_v2);
    let ToolStatus::Available { version, .. } = status else {
        panic!("取消后旧版本应保留：{status:?}");
    };
    assert_eq!(version, "demo-tool 1.0");
}

#[test]
fn uninstall_removes_only_owned_directory() {
    let root = TempDir::new().unwrap();
    let manager = new_manager(root.path());
    let body = tool_zip("demo-tool", "demo-tool 1.0");
    let base = spawn_server(body.clone(), None);
    let manifest = manifest_for(
        "demo-tool",
        &format!("{base}/demo-tool.zip"),
        &sha256_hex(&body),
    );
    let cancel = AtomicBool::new(false);
    manager.install(&manifest, &cancel, &|_| {}).unwrap();

    // 相邻目录（其它工具）必须原样保留。
    let sibling = root.path().join("other-tool");
    std::fs::create_dir_all(&sibling).unwrap();
    std::fs::write(sibling.join("keep.txt"), b"keep").unwrap();

    let record = InstalledTool {
        manifest: manifest.clone(),
        installed_at: "2026-09-18T00:00:00Z".into(),
        install_dir: root.path().join("demo-tool").to_string_lossy().into_owned(),
    };
    manager.uninstall(&record).expect("受管工具应可卸载");
    assert!(!root.path().join("demo-tool").exists());
    assert_eq!(std::fs::read(sibling.join("keep.txt")).unwrap(), b"keep");

    // 没有所有权标记的目录拒绝卸载。
    let foreign = InstalledTool {
        manifest: manifest.clone(),
        installed_at: String::new(),
        install_dir: sibling.to_string_lossy().into_owned(),
    };
    let error = manager.uninstall(&foreign).expect_err("无标记目录应拒绝");
    assert!(error.contains("所有权"), "错误信息：{error}");
    assert!(sibling.join("keep.txt").exists());
}
