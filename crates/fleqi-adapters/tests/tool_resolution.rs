//! 受管工具页状态与能力执行器的同一路径解析；不依赖用户安装目录。
#![cfg(unix)]
use fleqi_adapters::{
    image_operations::{set_tool_resolver, tool},
    process::ProcessRunnerPort,
    tools::ToolManager,
};
use fleqi_application::{
    dto::AppEvent,
    ports::{Clock, EventSink, InstallProgress, StorageError, ToolFacility, ToolStore},
    tool_service::ToolService,
};
use fleqi_domain::tools::{InstalledTool, ToolManifest, ToolSource, ToolStatus};
use std::{
    path::Path,
    sync::{Arc, Mutex, atomic::AtomicBool},
};

#[derive(Default)]
struct Store(Mutex<Vec<InstalledTool>>);
impl ToolStore for Store {
    fn load_all(&self) -> Result<Vec<InstalledTool>, StorageError> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn upsert(&self, tool: &InstalledTool) -> Result<(), StorageError> {
        let mut tools = self.0.lock().unwrap();
        tools.retain(|entry| entry.manifest.id != tool.manifest.id);
        tools.push(tool.clone());
        Ok(())
    }
    fn delete(&self, id: &str) -> Result<(), StorageError> {
        self.0
            .lock()
            .unwrap()
            .retain(|entry| entry.manifest.id != id);
        Ok(())
    }
}
struct TestClock;
impl Clock for TestClock {
    fn now_rfc3339(&self) -> String {
        "2026-09-20T00:00:00Z".into()
    }
}
struct NoEvents;
impl EventSink for NoEvents {
    fn emit(&self, _: AppEvent) {}
}

struct TracedFacility {
    inner: ToolManager,
    detected: Mutex<Vec<String>>,
}
impl ToolFacility for TracedFacility {
    fn detect(&self, manifest: &ToolManifest) -> ToolStatus {
        self.detected
            .lock()
            .unwrap()
            .push(manifest.executable.clone());
        self.inner.detect(manifest)
    }
    fn install(
        &self,
        manifest: &ToolManifest,
        cancel: &AtomicBool,
        progress: &dyn Fn(InstallProgress),
    ) -> Result<ToolStatus, String> {
        self.inner.install(manifest, cancel, progress)
    }
    fn uninstall(&self, tool: &InstalledTool) -> Result<(), String> {
        self.inner.uninstall(tool)
    }
}

fn manifest(id: &str, executable: &str) -> ToolManifest {
    ToolManifest {
        id: id.into(),
        version: "1.0".into(),
        platform: "macos".into(),
        arch: "any".into(),
        executable: executable.into(),
        source: ToolSource::Managed {
            url: "https://example.com/fixture.zip".into(),
            sha256: "a".repeat(64),
        },
        license: None,
        capabilities: vec![],
        detection_args: vec!["--version".into()],
    }
}

fn executable(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn resolves_actual_managed_binary_by_executable_and_probes_only_matching_manifests() {
    let directory = tempfile::tempdir().unwrap();
    // ID differs from the executable; the package keeps its binary below bin/.
    let package = manifest("video-package", "bin/ffmpeg");
    let binary = directory.path().join("video-package/bin/ffmpeg");
    executable(&binary, "printf 'managed fixture ffmpeg\\n'");
    let store = Arc::new(Store::default());
    store
        .upsert(&InstalledTool {
            manifest: package.clone(),
            installed_at: "2026-09-20T00:00:00Z".into(),
            install_dir: directory.path().join("video-package").display().to_string(),
        })
        .unwrap();
    let facility = Arc::new(TracedFacility {
        inner: ToolManager::new(directory.path(), Arc::new(ProcessRunnerPort)),
        detected: Mutex::new(vec![]),
    });
    let mut service = ToolService::new(
        store,
        facility.clone(),
        Arc::new(TestClock),
        Arc::new(NoEvents),
    );
    service
        .register_manifests(vec![
            package,
            manifest("irrelevant-package", "bin/must-not-run"),
        ])
        .unwrap();
    assert_eq!(
        service.resolve_executable("ffmpeg").unwrap(),
        Some(binary.clone())
    );
    assert_eq!(*facility.detected.lock().unwrap(), vec!["bin/ffmpeg"]);
    assert!(
        service
            .resolve_executable("video-package")
            .unwrap()
            .is_none()
    );
    assert!(service.resolve_executable("../ffmpeg").is_err());
    assert!(service.resolve_executable("/usr/bin/ffmpeg").is_err());
    assert_eq!(*facility.detected.lock().unwrap(), vec!["bin/ffmpeg"]);

    let service = Arc::new(service);
    let weak = Arc::downgrade(&service);
    let callback_service = weak.clone();
    set_tool_resolver(Arc::new(move |name| {
        callback_service.upgrade().map_or(Ok(None), |tools| {
            tools
                .resolve_executable(name)
                .map_err(|error| error.message)
        })
    }))
    .unwrap();
    assert_eq!(
        Arc::strong_count(&service),
        1,
        "resolver must not keep ToolService alive"
    );
    let resolved = tool("ffmpeg").unwrap();
    assert_eq!(resolved, binary);
    let output = std::process::Command::new(&resolved)
        .arg("--version")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"managed fixture ffmpeg\n");

    // A failed probe may not keep returning its earlier Available path.
    executable(&binary, "exit 7");
    let unavailable = service.resolve_executable("ffmpeg").unwrap();
    assert_ne!(unavailable, Some(binary.clone()));
    assert!(
        facility
            .detected
            .lock()
            .unwrap()
            .iter()
            .all(|name| name == "bin/ffmpeg" || name == "ffmpeg")
    );
    drop(service);
    assert!(weak.upgrade().is_none());
    assert!(
        tool("sh").unwrap().is_absolute(),
        "dropped host falls back to system tools"
    );

    let not_executable = directory.path().join("not-executable");
    std::fs::write(&not_executable, "not a program").unwrap();
    set_tool_resolver(Arc::new(move |_| Ok(Some(not_executable.clone())))).unwrap();
    assert!(tool("sh").unwrap().ends_with("sh"));
    assert!(tool("../sh").is_err());
    assert!(tool("fleqi-definitely-absent-tool").is_err());

    let alias = directory.path().join("dispatch-alias");
    std::os::unix::fs::symlink("/bin/sh", &alias).unwrap();
    let configured_alias = alias.clone();
    set_tool_resolver(Arc::new(move |_| Ok(Some(configured_alias.clone())))).unwrap();
    let resolved = tool("dispatch-alias").unwrap();
    assert_eq!(
        resolved, alias,
        "preserve argv[0] identity of executable symlinks"
    );
    let output = std::process::Command::new(resolved)
        .args(["-c", "printf '%s' \"$0\""])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, alias.as_os_str().as_encoded_bytes());
}

#[test]
fn persisted_managed_manifest_survives_collision_with_builtin_tool_id() {
    let directory = tempfile::tempdir().unwrap();
    let package = manifest("ffmpeg", "bin/ffmpeg");
    let binary = directory.path().join("ffmpeg/bin/ffmpeg");
    executable(&binary, "printf 'persisted managed ffmpeg\\n'");
    let store = Arc::new(Store::default());
    store
        .upsert(&InstalledTool {
            manifest: package,
            installed_at: "2026-09-20T00:00:00Z".into(),
            install_dir: directory.path().join("ffmpeg").display().to_string(),
        })
        .unwrap();
    let facility = Arc::new(TracedFacility {
        inner: ToolManager::new(directory.path(), Arc::new(ProcessRunnerPort)),
        detected: Mutex::new(vec![]),
    });
    // No imported catalog after restart: installed record must still beat builtin ffmpeg.
    let service = ToolService::new(
        store,
        facility.clone(),
        Arc::new(TestClock),
        Arc::new(NoEvents),
    );
    assert_eq!(service.resolve_executable("ffmpeg").unwrap(), Some(binary));
    assert_eq!(*facility.detected.lock().unwrap(), vec!["bin/ffmpeg"]);
}
