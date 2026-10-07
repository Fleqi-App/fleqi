//! 工具设施适配（M3.3；architecture.md §9.2）：系统工具探测、受管包
//! staging 下载 + SHA-256 校验 + 安全解压 + 预检 + 原子发布、所有权卸载。
//!
//! 约束：不执行远程脚本内容；下载只取字节并校验；解压拒绝越界条目；
//! 失败/取消清理本次 staging 并保留已安装可用版本（FR-TOOLS-002/003）。
//! 安装目录：`<root>/<tool-id>/`，其中 `fleqi-tool.json` 为所有权标记。

use fleqi_application::ports::{ProcessEvent, ProcessPort, ToolFacility};
use fleqi_domain::tools::{InstalledTool, ToolManifest, ToolOwner, ToolSource, ToolStatus};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

const MARKER_FILE: &str = "fleqi-tool.json";

pub struct ToolManager {
    root: PathBuf,
    /// 检测经真实进程执行（与 Run 执行同一端口；无 shell、无拼接）。
    process: Arc<dyn ProcessPort>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Marker {
    tool_id: String,
    version: String,
}

impl ToolManager {
    pub fn new(root: impl Into<PathBuf>, process: Arc<dyn ProcessPort>) -> Self {
        Self {
            root: root.into(),
            process,
        }
    }

    fn install_dir(&self, tool_id: &str) -> PathBuf {
        self.root.join(tool_id)
    }

    fn detect_with(&self, manifest: &ToolManifest, executable: &Path) -> ToolStatus {
        if !executable.exists() {
            return match &manifest.source {
                ToolSource::System => ToolStatus::Unavailable {
                    reason: format!("PATH 中未找到 {}", manifest.executable),
                },
                ToolSource::Managed { .. } => ToolStatus::NotInstalled,
            };
        }
        let cwd = executable
            .parent()
            .map(|dir| dir.to_path_buf())
            .unwrap_or_else(std::env::temp_dir);
        let (tx, rx) = mpsc::channel::<ProcessEvent>();
        let args = manifest.detection_args.clone();
        let spawned = self
            .process
            .spawn(&executable.to_string_lossy(), &args, &cwd, &[], tx);
        let Ok(handle) = spawned else {
            return ToolStatus::Unavailable {
                reason: "无法启动检测进程".into(),
            };
        };
        let mut output = String::new();
        let mut exit: Option<i32> = None;
        let deadline = std::time::Instant::now() + Duration::from_secs(4);
        loop {
            if std::time::Instant::now() >= deadline {
                handle.cancel();
                return ToolStatus::Unavailable {
                    reason: "工具版本检测超时".into(),
                };
            }
            let event = match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(event) => event,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            };
            match event {
                ProcessEvent::Output { bytes, .. } => {
                    if output.len() < 4096 {
                        output.push_str(&String::from_utf8_lossy(
                            &bytes[..bytes.len().min(4096 - output.len())],
                        ));
                    }
                }
                ProcessEvent::Exited { status } => {
                    exit = status;
                    break;
                }
            }
        }
        drop(handle);
        if exit == Some(0) {
            let version = output.trim().lines().next().unwrap_or_default().to_owned();
            ToolStatus::Available {
                version,
                owner: match &manifest.source {
                    ToolSource::System => ToolOwner::System,
                    ToolSource::Managed { .. } => ToolOwner::Fleqi,
                },
                path: executable.to_string_lossy().into_owned(),
            }
        } else {
            ToolStatus::Unavailable {
                reason: format!(
                    "检测命令退出码 {:?}，输出：{}",
                    exit,
                    output.trim().chars().take(120).collect::<String>()
                ),
            }
        }
    }
}

impl ToolFacility for ToolManager {
    fn supports_install(&self) -> bool {
        !cfg!(windows)
    }
    fn detect(&self, manifest: &ToolManifest) -> ToolStatus {
        let executable = match &manifest.source {
            ToolSource::System => match lookup_on_path(&manifest.executable) {
                Some(path) => path,
                None => {
                    return ToolStatus::Unavailable {
                        reason: format!("PATH 中未找到 {}", manifest.executable),
                    };
                }
            },
            ToolSource::Managed { .. } => self.install_dir(&manifest.id).join(&manifest.executable),
        };
        self.detect_with(manifest, &executable)
    }

    fn install(
        &self,
        manifest: &ToolManifest,
        cancel: &AtomicBool,
        progress: &dyn Fn(fleqi_application::ports::InstallProgress),
    ) -> Result<ToolStatus, String> {
        use fleqi_application::ports::InstallProgress;
        if matches!(manifest.source, ToolSource::System) {
            crate::system_tools::install(&self.root, &self.process, manifest, cancel, progress)?;
            return Ok(self.detect(manifest));
        }
        let ToolSource::Managed { url, sha256 } = &manifest.source else {
            unreachable!()
        };
        manifest
            .validate()
            .map_err(|message| format!("清单无效：{message}"))?;
        let staging = self.root.join(format!(".staging.{}", manifest.id));
        // 本次 staging 必须是干净的；上一轮失败遗留直接清理（不碰正式目录）。
        let _ = std::fs::remove_dir_all(&staging);
        let cleanup = |staging: &Path| {
            let _ = std::fs::remove_dir_all(staging);
        };
        std::fs::create_dir_all(&staging).map_err(|e| {
            cleanup(&staging);
            format!("创建 staging 失败：{e}")
        })?;

        // 1) 下载到 staging（流式 + 分块取消 + 增量哈希）。
        let archive = staging.join("package.zip");
        let download = download_to(url, &archive, cancel, progress);
        if let Err(message) = download {
            cleanup(&staging);
            return Err(format!("下载失败：{message}"));
        }
        progress(InstallProgress::Verifying);
        let actual = file_sha256(&archive).map_err(|e| {
            cleanup(&staging);
            format!("读取下载包失败：{e}")
        })?;
        if !actual.eq_ignore_ascii_case(sha256) {
            cleanup(&staging);
            return Err("校验失败：SHA-256 与清单不一致".into());
        }

        // 2) 安全解压（越界条目拒绝；整个包任何条目不安全即失败）。
        progress(InstallProgress::Extracting);
        let payload = staging.join("payload");
        let report = extract_package(&archive, &payload);
        if !report.is_ok() {
            cleanup(&staging);
            return Err(format!("解压失败：{:?}", report.bad_entries()));
        }

        // 3) 定位并预检可执行文件（FR-TOOLS-005：坏包/错误架构不得发布）。
        let executable_dir =
            locate_executable_dir(&payload, &manifest.executable).ok_or_else(|| {
                cleanup(&staging);
                "包内未找到清单声明的可执行文件".to_owned()
            })?;
        let staged_executable = executable_dir.join(&manifest.executable);
        make_executable(&staged_executable);
        let probed = self.detect_with(manifest, &staged_executable);
        if !matches!(probed, ToolStatus::Available { .. }) {
            cleanup(&staging);
            return Err(format!("包预检未通过：{probed:?}"));
        }

        // 4) 原子发布：旧目录先移走，成功后删除；失败回滚保留旧版本。
        progress(InstallProgress::Publishing);
        let final_dir = self.install_dir(&manifest.id);
        let marker = Marker {
            tool_id: manifest.id.clone(),
            version: manifest.version.clone(),
        };
        if let Err(e) = std::fs::write(
            executable_dir.join(MARKER_FILE),
            serde_json::to_string(&marker).unwrap_or_default(),
        ) {
            cleanup(&staging);
            return Err(format!("写入所有权标记失败：{e}"));
        }
        let backup = self.root.join(format!(".old.{}", manifest.id));
        let _ = std::fs::remove_dir_all(&backup);
        let had_old = final_dir.exists();
        if had_old && std::fs::rename(&final_dir, &backup).is_err() {
            cleanup(&staging);
            return Err("移走旧版本失败，已保留现行版本".into());
        }
        if std::fs::rename(&executable_dir, &final_dir).is_err() {
            if had_old {
                let _ = std::fs::rename(&backup, &final_dir);
            }
            cleanup(&staging);
            return Err("发布新版本失败，已回滚".into());
        }
        let _ = std::fs::remove_dir_all(&backup);
        cleanup(&staging);
        Ok(self.detect_with(manifest, &final_dir.join(&manifest.executable)))
    }

    fn uninstall(&self, installed: &InstalledTool) -> Result<(), String> {
        let dir = Path::new(&installed.install_dir);
        let marker_path = dir.join(MARKER_FILE);
        let marker: Marker = std::fs::read_to_string(&marker_path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .ok_or_else(|| "该目录没有 Fleqi 所有权标记，可能不是应用管理的工具".to_owned())?;
        if marker.tool_id != installed.manifest.id {
            return Err("所有权标记与记录不一致，拒绝卸载".into());
        }
        if !dir.starts_with(&self.root) {
            return Err("安装目录不在应用工具根目录内，拒绝卸载".into());
        }
        std::fs::remove_dir_all(dir).map_err(|e| format!("卸载失败：{e}"))
    }
}

/// PATH 查找（不含当前目录）。
pub(crate) fn lookup_on_path(name: &str) -> Option<PathBuf> {
    if name.contains(['/', '\\', ':']) {
        return None;
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut names = vec![name.to_owned()];
    if cfg!(windows) && Path::new(name).extension().is_none() {
        names.extend(
            std::env::var("PATHEXT")
                .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
                .split(';')
                .filter(|extension| {
                    extension.starts_with('.') && !extension.contains(['/', '\\', ':'])
                })
                .map(|extension| format!("{name}{extension}")),
        );
    }
    crate::environment::executable_paths(&path)
        .into_iter()
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|candidate| candidate.is_file())
}

/// 流式下载：分块写入 + 增量哈希 + 取消检查；只允许 https 与回环 http
/// （与领域校验一致，双保险）。分块回调下载进度（长度未知时 total 为 None）。
pub(crate) fn download_to(
    url: &str,
    destination: &Path,
    cancel: &AtomicBool,
    progress: &dyn Fn(fleqi_application::ports::InstallProgress),
) -> Result<(), String> {
    use fleqi_application::ports::InstallProgress;
    let allowed = url.starts_with("https://")
        || url.starts_with("http://127.0.0.1")
        || url.starts_with("http://localhost")
        || url.starts_with("http://[::1]");
    if !allowed {
        return Err("仅允许 HTTPS 来源（回环地址除外）".into());
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| e.to_string())?;
    let mut response = client
        .get(url)
        .send()
        .map_err(|e| format!("请求失败：{e}"))?
        .error_for_status()
        .map_err(|e| format!("来源返回错误状态：{e}"))?;
    let total = response.content_length();
    let mut file = std::fs::File::create(destination).map_err(|e| e.to_string())?;
    let mut buffer = [0u8; 64 * 1024];
    let mut downloaded: u64 = 0;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("已取消".into());
        }
        let read = response.read(&mut buffer).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        std::io::Write::write_all(&mut file, &buffer[..read]).map_err(|e| e.to_string())?;
        downloaded += read as u64;
        progress(InstallProgress::Download {
            bytes: downloaded,
            total,
        });
    }
    Ok(())
}

pub(crate) fn file_sha256(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// 解压结果：全部条目安全写入才视为成功。
#[derive(Debug, Default)]
struct ExtractReport {
    entries: usize,
    bad: Vec<String>,
}

impl ExtractReport {
    fn is_ok(&self) -> bool {
        self.entries > 0 && self.bad.is_empty()
    }
    fn bad_entries(&self) -> &[String] {
        &self.bad
    }
}

/// 安全解压：拒绝 `..`、绝对路径与设备路径条目（AC-CAP-011 同一规则）。
fn extract_package(archive: &Path, target: &Path) -> ExtractReport {
    let mut report = ExtractReport::default();
    let Ok(file) = std::fs::File::open(archive) else {
        report.bad.push("无法打开压缩包".into());
        return report;
    };
    let Ok(mut zip) = zip::ZipArchive::new(file) else {
        report.bad.push("压缩包格式无效".into());
        return report;
    };
    let _ = std::fs::create_dir_all(target);
    for index in 0..zip.len() {
        let Ok(mut entry) = zip.by_index(index) else {
            report.bad.push(format!("条目 {index} 读取失败"));
            continue;
        };
        let name = entry.name().to_owned();
        let safe = !name.split('/').any(|part| part == "..")
            && !name.starts_with('/')
            && !name.contains('\\');
        if !safe {
            report.bad.push(format!("越界条目拒绝：{name}"));
            continue;
        }
        let destination = target.join(&name);
        if !destination.starts_with(target) {
            report.bad.push(format!("越界条目拒绝：{name}"));
            continue;
        }
        let written = (|| -> std::io::Result<()> {
            if entry.is_dir() {
                std::fs::create_dir_all(&destination)?;
            } else {
                if let Some(dir) = destination.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                let mut output = std::fs::File::create(&destination)?;
                std::io::copy(&mut entry, &mut output)?;
            }
            Ok(())
        })();
        match written {
            Ok(()) => report.entries += 1,
            Err(e) => report.bad.push(format!("{name}: {e}")),
        }
    }
    report
}

/// 定位包含清单可执行文件的目录：包根或单一顶层目录。
fn locate_executable_dir(payload: &Path, executable: &str) -> Option<PathBuf> {
    if payload.join(executable).exists() {
        return Some(payload.to_path_buf());
    }
    let mut directories = std::fs::read_dir(payload)
        .ok()?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.path());
    let first = directories.next()?;
    if directories.next().is_some() {
        return None; // 多个顶层目录的包结构不明确，拒绝
    }
    if first.join(executable).exists() {
        Some(first)
    } else {
        None
    }
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(metadata) = std::fs::metadata(path) {
        let mut permissions = metadata.permissions();
        permissions.set_mode(permissions.mode() | 0o755);
        let _ = std::fs::set_permissions(path, permissions);
    }
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) {}
