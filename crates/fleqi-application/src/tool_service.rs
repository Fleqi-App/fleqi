//! ToolService（M3.3；FR-TOOLS-001..005）：工具检测/缺失列表、受管安装、
//! 所有权卸载与刷新。清单来自内建系统探测登记与已安装记录；不虚构下载站。
//! 安装成功后重新探测并把状态落库；失败不改变已安装版本。

use crate::dto::{AppError, AppEvent, AppResult};
use crate::ports::{Clock, EventSink, InstallProgress, ToolFacility, ToolStore};
use fleqi_domain::tools::{InstalledTool, ToolManifest, ToolSource, ToolStatus};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub struct ToolService {
    store: Arc<dyn ToolStore>,
    facility: Arc<dyn ToolFacility>,
    clock: Arc<dyn Clock>,
    events: Arc<dyn EventSink>,
    /// 额外受管清单（导入/配置加入；内建只有系统探测登记）。
    extra_manifests: Vec<ToolManifest>,
    installation: Mutex<()>,
    preparation: Mutex<ToolPreparation>,
    prepare_cancel: AtomicBool,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ToolPreparation {
    pub running: bool,
    pub cancelled: bool,
    pub completed: usize,
    pub total: usize,
    pub current_tool: Option<String>,
    pub progress: Option<InstallProgress>,
    pub errors: Vec<String>,
}

/// tools_list 条目：清单 + 当前真实状态 + 已安装记录（UI-TOOLS 展示来源）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ToolEntry {
    pub manifest: ToolManifest,
    pub status: ToolStatus,
    pub installed: Option<InstalledTool>,
}

impl ToolService {
    pub fn new(
        store: Arc<dyn ToolStore>,
        facility: Arc<dyn ToolFacility>,
        clock: Arc<dyn Clock>,
        events: Arc<dyn EventSink>,
    ) -> Self {
        Self {
            store,
            facility,
            clock,
            events,
            extra_manifests: Vec::new(),
            installation: Mutex::new(()),
            preparation: Mutex::new(ToolPreparation::default()),
            prepare_cancel: AtomicBool::new(false),
        }
    }

    pub fn preparation(&self) -> ToolPreparation {
        self.preparation.lock().expect("tool preparation").clone()
    }

    pub fn cancel_preparation(&self) {
        self.prepare_cancel.store(true, Ordering::Release);
    }

    /// First launch and explicit retry share the same bounded, cancellable queue.
    pub fn prepare(self: &Arc<Self>) -> ToolPreparation {
        if !self.facility.supports_install() {
            return self.preparation();
        }
        let manifests = fleqi_domain::tools::builtin_tool_manifests();
        {
            let mut state = self.preparation.lock().expect("tool preparation");
            if state.running {
                return state.clone();
            }
            *state = ToolPreparation {
                running: true,
                total: manifests.len(),
                ..Default::default()
            };
            self.prepare_cancel.store(false, Ordering::Release);
        }
        let service = self.clone();
        std::thread::spawn(move || {
            for manifest in manifests {
                if service.prepare_cancel.load(Ordering::Acquire) {
                    break;
                }
                {
                    let mut state = service.preparation.lock().expect("tool preparation");
                    state.current_tool = Some(manifest.id.clone());
                    state.progress = None;
                }
                // Available tools do not invoke an installer or perform a network request.
                let existing = service.resolve_executable(&manifest.executable);
                let result = if matches!(&existing, Ok(Some(_))) {
                    Ok(())
                } else if let Err(error) = existing {
                    Err(error)
                } else {
                    service
                        .install(&manifest.id, &service.prepare_cancel, &|progress| {
                            service
                                .preparation
                                .lock()
                                .expect("tool preparation")
                                .progress = Some(progress)
                        })
                        .map(|_| ())
                };
                let mut state = service.preparation.lock().expect("tool preparation");
                state.completed += 1;
                if let Err(error) = result {
                    state
                        .errors
                        .push(format!("{}：{}", manifest.id, error.message));
                }
            }
            {
                let mut state = service.preparation.lock().expect("tool preparation");
                state.running = false;
                state.cancelled = service.prepare_cancel.load(Ordering::Acquire);
                state.current_tool = None;
                state.progress = None;
            }
            service.events.emit(AppEvent::ToolsChanged {
                tool_id: "preparation".into(),
            });
        });
        self.preparation()
    }

    /// 注册额外受管清单（导入路径；清单必须通过校验）。
    pub fn register_manifests(&mut self, manifests: Vec<ToolManifest>) -> AppResult<()> {
        for manifest in &manifests {
            manifest.validate().map_err(|message| {
                AppError::validation(vec![fleqi_domain::settings::FieldError {
                    field: "manifests".into(),
                    code: "invalid".into(),
                    message,
                }])
            })?;
        }
        self.extra_manifests.extend(manifests);
        Ok(())
    }

    fn all_manifests(&self) -> Vec<ToolManifest> {
        let installed = self
            .store
            .load_all()
            .map_err(|e| AppError::storage(e.to_string()))
            .ok()
            .unwrap_or_default();
        let mut manifests = fleqi_domain::tools::builtin_tool_manifests();
        manifests.extend(self.extra_manifests.iter().cloned());
        // 已安装记录里的清单即使不在当前登记中也列出（状态以探测为准）。
        for tool in installed {
            if !manifests
                .iter()
                .any(|manifest| manifest.id == tool.manifest.id)
            {
                manifests.push(tool.manifest);
            }
        }
        manifests
    }

    /// 检测并汇总：每个清单的真实状态（探测真实执行，不读缓存值）。
    pub fn list(&self) -> AppResult<Vec<ToolEntry>> {
        let installed = self
            .store
            .load_all()
            .map_err(|e| AppError::storage(e.to_string()))?;
        Ok(self
            .all_manifests()
            .into_iter()
            .map(|manifest| {
                let installed = installed
                    .iter()
                    .find(|tool| tool.manifest.id == manifest.id)
                    .cloned();
                let managed_status = installed
                    .as_ref()
                    .map(|tool| self.facility.detect(&tool.manifest));
                let status = match managed_status {
                    Some(status @ ToolStatus::Available { .. }) => status,
                    _ => self.facility.detect(&manifest),
                };
                ToolEntry {
                    manifest,
                    status,
                    installed,
                }
            })
            .collect())
    }

    /// 按可执行文件名解析实际可用路径，而非清单 ID 或推测的安装目录。
    /// 仅探测匹配候选；已安装受管版本优先，缺失时才探测同名系统工具。
    pub fn resolve_executable(&self, executable: &str) -> AppResult<Option<std::path::PathBuf>> {
        use std::path::{Component, Path, PathBuf};
        let mut components = Path::new(executable).components();
        if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
            return Err(AppError::unavailable("工具解析需要单个可执行文件名"));
        }
        let installed = self
            .store
            .load_all()
            .map_err(|e| AppError::storage(e.to_string()))?;
        // 不能沿用 all_manifests 的 ID 去重：受管包与内建系统登记可能同名。
        let mut candidates = installed
            .into_iter()
            .map(|tool| tool.manifest)
            .chain(self.extra_manifests.iter().cloned())
            .chain(fleqi_domain::tools::builtin_tool_manifests())
            .filter(|manifest| {
                Path::new(&manifest.executable).file_name()
                    == Some(std::ffi::OsStr::new(executable))
            })
            .collect::<Vec<_>>();
        candidates.sort_by_key(|manifest| matches!(manifest.source, ToolSource::System));
        let mut seen = std::collections::BTreeSet::new();
        for manifest in candidates {
            if !seen.insert((
                manifest.id.clone(),
                manifest.executable.clone(),
                matches!(manifest.source, ToolSource::System),
            )) {
                continue;
            }
            if let ToolStatus::Available { path, .. } = self.facility.detect(&manifest)
                && !path.is_empty()
            {
                return Ok(Some(PathBuf::from(path)));
            }
        }
        Ok(None)
    }

    /// 受管安装（系统工具不由应用安装）。成功后重新探测、落库并广播；
    /// 失败/取消保留旧版本（端口保证 staging 清理）。进度经 `progress` 回报。
    pub fn install(
        &self,
        tool_id: &str,
        cancel: &AtomicBool,
        progress: &dyn Fn(InstallProgress),
    ) -> AppResult<ToolEntry> {
        if !self.facility.supports_install() {
            return Err(AppError::unavailable(
                "当前平台暂不提供工具自动安装，请安装工具后重新检测",
            ));
        }
        let _guard = self.installation.lock().expect("tool installation");
        if cancel.load(Ordering::Acquire) {
            return Err(AppError::unavailable("已取消"));
        }
        let manifest = self
            .all_manifests()
            .into_iter()
            .find(|manifest| manifest.id == tool_id)
            .ok_or_else(|| AppError::not_found(format!("工具 {tool_id} 不在目录中")))?;
        match &manifest.source {
            ToolSource::System => {
                let existing = self.facility.detect(&manifest);
                let status = if matches!(existing, ToolStatus::Available { .. }) {
                    existing
                } else {
                    self.facility
                        .install(&manifest, cancel, progress)
                        .map_err(AppError::unavailable)?
                };
                if !matches!(status, ToolStatus::Available { .. }) {
                    return Err(AppError::unavailable(format!(
                        "安装后的工具仍不可用：{status:?}"
                    )));
                }
                self.events.emit(AppEvent::ToolsChanged {
                    tool_id: manifest.id.clone(),
                });
                Ok(ToolEntry {
                    manifest,
                    status,
                    installed: None,
                })
            }
            ToolSource::Managed { .. } => {
                let status = self
                    .facility
                    .install(&manifest, cancel, progress)
                    .map_err(AppError::unavailable)?;
                if let ToolStatus::Available { .. } = status {
                    let record = InstalledTool {
                        install_dir: self.facility_install_dir(&manifest),
                        manifest: manifest.clone(),
                        installed_at: self.clock.now_rfc3339(),
                    };
                    self.store
                        .upsert(&record)
                        .map_err(|e| AppError::storage(e.to_string()))?;
                }
                let installed = self
                    .store
                    .load_all()
                    .map_err(|e| AppError::storage(e.to_string()))?
                    .into_iter()
                    .find(|tool| tool.manifest.id == manifest.id);
                self.events.emit(AppEvent::ToolsChanged {
                    tool_id: manifest.id.clone(),
                });
                Ok(ToolEntry {
                    status,
                    installed,
                    manifest,
                })
            }
        }
    }

    /// 卸载：只允许应用拥有的受管包；系统工具明确拒绝（FR-TOOLS-003）。
    pub fn remove(&self, tool_id: &str) -> AppResult<()> {
        let installed = self
            .store
            .load_all()
            .map_err(|e| AppError::storage(e.to_string()))?;
        let record = installed
            .iter()
            .find(|tool| tool.manifest.id == tool_id)
            .cloned()
            .ok_or_else(|| AppError::not_found(format!("工具 {tool_id} 没有受管安装记录")))?;
        self.facility
            .uninstall(&record)
            .map_err(|message| AppError::conflict(message, None))?;
        self.store
            .delete(tool_id)
            .map_err(|e| AppError::storage(e.to_string()))?;
        self.events.emit(AppEvent::ToolsChanged {
            tool_id: tool_id.to_owned(),
        });
        Ok(())
    }

    /// 受管安装目录由设施层探测结果推导（服务不猜测磁盘布局）。
    fn facility_install_dir(&self, manifest: &ToolManifest) -> String {
        self.facility
            .detect(manifest)
            .available_path()
            .and_then(|path| {
                path.strip_suffix(&manifest.executable)
                    .map(|dir| dir.trim_end_matches('/').to_owned())
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::StorageError;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicBool;

    struct FakeStore(Mutex<Vec<InstalledTool>>);
    impl ToolStore for FakeStore {
        fn upsert(&self, tool: &InstalledTool) -> Result<(), StorageError> {
            let mut tools = self.0.lock().unwrap();
            tools.retain(|existing| existing.manifest.id != tool.manifest.id);
            tools.push(tool.clone());
            Ok(())
        }
        fn load_all(&self) -> Result<Vec<InstalledTool>, StorageError> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn delete(&self, tool_id: &str) -> Result<(), StorageError> {
            self.0
                .lock()
                .unwrap()
                .retain(|tool| tool.manifest.id != tool_id);
            Ok(())
        }
    }

    struct FakeFacility {
        installs: Mutex<Vec<String>>,
        uninstalls: Mutex<Vec<String>>,
        fail_install: bool,
    }
    impl ToolFacility for FakeFacility {
        fn detect(&self, manifest: &ToolManifest) -> ToolStatus {
            match &manifest.source {
                ToolSource::System => ToolStatus::Available {
                    version: "git version 9".into(),
                    owner: fleqi_domain::tools::ToolOwner::System,
                    path: "/usr/bin/git".into(),
                },
                ToolSource::Managed { .. } => {
                    if self
                        .installs
                        .lock()
                        .unwrap()
                        .iter()
                        .any(|id| id == &manifest.id)
                    {
                        ToolStatus::Available {
                            version: manifest.version.clone(),
                            owner: fleqi_domain::tools::ToolOwner::Fleqi,
                            path: format!("/data/tools/{}/{}", manifest.id, manifest.executable),
                        }
                    } else {
                        ToolStatus::NotInstalled
                    }
                }
            }
        }
        fn install(
            &self,
            manifest: &ToolManifest,
            _cancel: &AtomicBool,
            _progress: &dyn Fn(InstallProgress),
        ) -> Result<ToolStatus, String> {
            if self.fail_install {
                return Err("校验失败".into());
            }
            self.installs.lock().unwrap().push(manifest.id.clone());
            Ok(self.detect(manifest))
        }
        fn uninstall(&self, installed: &InstalledTool) -> Result<(), String> {
            self.installs
                .lock()
                .unwrap()
                .retain(|id| id != &installed.manifest.id);
            self.uninstalls
                .lock()
                .unwrap()
                .push(installed.manifest.id.clone());
            Ok(())
        }
    }

    struct FixedClock;
    impl Clock for FixedClock {
        fn now_rfc3339(&self) -> String {
            "2026-09-18T00:00:00Z".into()
        }
    }

    struct NoEvents;
    impl EventSink for NoEvents {
        fn emit(&self, _event: AppEvent) {}
    }

    fn managed_manifest(id: &str) -> ToolManifest {
        ToolManifest {
            id: id.into(),
            version: "1.0".into(),
            platform: "macos".into(),
            arch: "aarch64".into(),
            executable: "bin/tool".into(),
            source: ToolSource::Managed {
                url: "https://example.com/tool.zip".into(),
                sha256: "a".repeat(64),
            },
            license: None,
            capabilities: vec![],
            detection_args: vec!["--version".into()],
        }
    }

    fn service(fail_install: bool) -> ToolService {
        let facility = Arc::new(FakeFacility {
            installs: Mutex::new(Vec::new()),
            uninstalls: Mutex::new(Vec::new()),
            fail_install,
        });
        ToolService::new(
            Arc::new(FakeStore(Mutex::new(Vec::new()))),
            Arc::clone(&facility) as Arc<dyn ToolFacility>,
            Arc::new(FixedClock),
            Arc::new(NoEvents),
        )
    }

    #[test]
    fn list_reports_builtin_system_tools_with_real_detection_shape() {
        let tools = service(false);
        let entries = tools.list().unwrap();
        let git = entries
            .iter()
            .find(|entry| entry.manifest.id == "git")
            .expect("内建登记包含 git");
        assert!(matches!(git.status, ToolStatus::Available { .. }));
    }

    #[test]
    fn preparation_installs_only_missing_tools_and_does_not_reinstall_on_retry() {
        struct MissingGit {
            installed: AtomicBool,
            calls: Mutex<Vec<String>>,
        }
        impl ToolFacility for MissingGit {
            fn detect(&self, manifest: &ToolManifest) -> ToolStatus {
                if manifest.id == "git" && !self.installed.load(Ordering::Acquire) {
                    ToolStatus::NotInstalled
                } else {
                    ToolStatus::Available {
                        version: "test".into(),
                        owner: fleqi_domain::tools::ToolOwner::System,
                        path: format!("/tools/{}", manifest.id),
                    }
                }
            }
            fn install(
                &self,
                manifest: &ToolManifest,
                _: &AtomicBool,
                _: &dyn Fn(InstallProgress),
            ) -> Result<ToolStatus, String> {
                self.calls.lock().unwrap().push(manifest.id.clone());
                self.installed.store(true, Ordering::Release);
                Ok(self.detect(manifest))
            }
            fn uninstall(&self, _: &InstalledTool) -> Result<(), String> {
                panic!("must not remove existing tools")
            }
        }
        let facility = Arc::new(MissingGit {
            installed: AtomicBool::new(false),
            calls: Mutex::new(vec![]),
        });
        let tools = Arc::new(ToolService::new(
            Arc::new(FakeStore(Mutex::new(vec![]))),
            facility.clone(),
            Arc::new(FixedClock),
            Arc::new(NoEvents),
        ));
        for _ in 0..2 {
            tools.prepare();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while tools.preparation().running && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            let status = tools.preparation();
            assert!(!status.running);
            assert!(status.errors.is_empty());
            assert_eq!(status.completed, status.total);
        }
        assert_eq!(*facility.calls.lock().unwrap(), ["git"]);
    }

    #[test]
    fn managed_install_persists_and_remove_goes_through_facility() {
        let mut tools = service(false);
        tools
            .register_manifests(vec![managed_manifest("demo")])
            .unwrap();
        let cancel = AtomicBool::new(false);
        let entry = tools.install("demo", &cancel, &|_| {}).unwrap();
        assert!(matches!(entry.status, ToolStatus::Available { .. }));
        assert!(entry.installed.is_some());
        // 第二次 list 仍能看到已安装状态（来自持久化记录 + 探测）。
        let listed = tools.list().unwrap();
        assert!(
            listed
                .iter()
                .find(|entry| entry.manifest.id == "demo")
                .unwrap()
                .installed
                .is_some()
        );
        tools.remove("demo").unwrap();
        let listed = tools.list().unwrap();
        let demo = listed
            .iter()
            .find(|entry| entry.manifest.id == "demo")
            .unwrap();
        assert!(demo.installed.is_none());
        assert!(matches!(demo.status, ToolStatus::NotInstalled));
    }

    #[test]
    fn existing_system_tool_is_reused_and_failure_keeps_store_empty() {
        let tools = service(false);
        let cancel = AtomicBool::new(false);
        let existing = tools.install("git", &cancel, &|_| {}).unwrap();
        assert!(matches!(existing.status, ToolStatus::Available { .. }));
        assert!(existing.installed.is_none());

        let mut failing = service(true);
        failing
            .register_manifests(vec![managed_manifest("bad")])
            .unwrap();
        let error = failing.install("bad", &cancel, &|_| {}).unwrap_err();
        assert_eq!(error.code, crate::dto::ErrorCode::Unavailable);
        let listed = failing.list().unwrap();
        let bad = listed
            .iter()
            .find(|entry| entry.manifest.id == "bad")
            .expect("登记的受管清单出现在列表");
        assert!(bad.installed.is_none());
    }
}
