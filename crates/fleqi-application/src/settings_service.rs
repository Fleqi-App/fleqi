//! SettingsService：requestId 幂等（同 ID 同载荷复用、不同载荷 conflict、并发重复合并）、
//! expectedRevision 校验、阶段字段允许表、提交后才广播（architecture.md §8、§12.2、§12.3）。

use fleqi_domain::idempotency::{Receipt, ReceiptOutcome, evaluate_receipt};
use fleqi_domain::revision::Revision;
use fleqi_domain::settings::{FieldAllowlist, Settings};
use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};

use crate::dto::{
    AppError, AppEvent, AppResult, SettingsSnapshot, SettingsUpdateRequest, StorageState,
};
use crate::fingerprint::fingerprint;
use crate::lifecycle::HostLifecycle;
use crate::ports::{Clock, EventSink, SettingsStore, StorageError};

struct Current {
    settings: Settings,
    revision: Revision,
    persisted: bool,
}

struct InFlight {
    done: Mutex<Option<AppResult<SettingsSnapshot>>>,
    ready: Condvar,
}

pub struct SettingsService {
    store: Arc<dyn SettingsStore>,
    #[allow(dead_code)]
    clock: Arc<dyn Clock>,
    events: Arc<dyn EventSink>,
    lifecycle: Arc<HostLifecycle>,
    allowlist: FieldAllowlist,
    current: Mutex<Current>,
    inflight: Mutex<HashMap<String, Arc<InFlight>>>,
}

impl SettingsService {
    /// 从存储加载；失败时使用未持久化的临时默认值并报告 Degraded。
    pub fn load(
        store: Arc<dyn SettingsStore>,
        clock: Arc<dyn Clock>,
        events: Arc<dyn EventSink>,
        lifecycle: Arc<HostLifecycle>,
        allowlist: FieldAllowlist,
    ) -> (Self, StorageState, Option<String>) {
        let (current, state, message) = match store.load() {
            Ok(Some(persisted)) => (
                Current {
                    settings: persisted.settings,
                    revision: persisted.revision,
                    persisted: true,
                },
                StorageState::Ready,
                None,
            ),
            Ok(None) => (
                Current {
                    settings: Settings::default(),
                    revision: Revision::new(0),
                    persisted: true,
                },
                StorageState::Ready,
                None,
            ),
            Err(error) => (
                Current {
                    settings: Settings::default(),
                    revision: Revision::new(0),
                    persisted: false,
                },
                StorageState::Degraded,
                Some(error.to_string()),
            ),
        };
        let service = Self {
            store,
            clock,
            events,
            lifecycle,
            allowlist,
            current: Mutex::new(current),
            inflight: Mutex::new(HashMap::new()),
        };
        (service, state, message)
    }

    pub fn snapshot(&self) -> SettingsSnapshot {
        let current = self.current.lock().expect("settings 锁");
        SettingsSnapshot {
            revision: current.revision,
            persisted: current.persisted,
            settings: current.settings.clone(),
        }
    }

    pub fn update(&self, request: SettingsUpdateRequest) -> AppResult<SettingsSnapshot> {
        self.lifecycle.ensure_accepts_changes()?;
        if request.request_id.trim().is_empty() {
            return Err(AppError::validation(vec![
                fleqi_domain::settings::FieldError {
                    field: "requestId".into(),
                    code: "invalid".into(),
                    message: "requestId 不能为空".into(),
                },
            ]));
        }

        // 并发重复：同 requestId 在途时等待领头请求的结果，不重复执行。
        let (flight, leader) = {
            let mut inflight = self.inflight.lock().expect("inflight 锁");
            match inflight.get(&request.request_id) {
                Some(existing) => (Arc::clone(existing), false),
                None => {
                    let flight = Arc::new(InFlight {
                        done: Mutex::new(None),
                        ready: Condvar::new(),
                    });
                    inflight.insert(request.request_id.clone(), Arc::clone(&flight));
                    (flight, true)
                }
            }
        };
        if !leader {
            let mut done = flight.done.lock().expect("inflight 结果锁");
            while done.is_none() {
                done = flight.ready.wait(done).expect("inflight 等待");
            }
            return done.clone().expect("已完成");
        }

        let result = self.execute(&request);
        {
            let mut done = flight.done.lock().expect("inflight 结果锁");
            *done = Some(result.clone());
            flight.ready.notify_all();
        }
        self.inflight
            .lock()
            .expect("inflight 锁")
            .remove(&request.request_id);
        result
    }

    fn execute(&self, request: &SettingsUpdateRequest) -> AppResult<SettingsSnapshot> {
        let payload = fingerprint(&(request.expected_revision, &request.patch));
        let existing = self
            .store
            .find_receipt(&request.request_id)
            .map_err(storage_error)?;
        match evaluate_receipt(existing.as_ref(), &request.request_id, &payload) {
            ReceiptOutcome::Replay(json) => {
                return serde_json::from_str::<SettingsSnapshot>(&json)
                    .map_err(|e| AppError::internal(format!("回执结果无法解析：{e}")));
            }
            ReceiptOutcome::Conflict => {
                let current = self.current.lock().expect("settings 锁");
                return Err(AppError::conflict(
                    "同一 requestId 携带了不同载荷",
                    Some(current.revision),
                ));
            }
            ReceiptOutcome::Fresh => {}
        }

        let mut current = self.current.lock().expect("settings 锁");
        if !current.persisted {
            return Err(AppError::storage(
                "存储不可用：设置仍是未持久化的临时默认值，请修复数据库后重试",
            ));
        }
        if request.expected_revision != current.revision {
            return Err(AppError::conflict(
                format!(
                    "期望版本 {} 已过期，当前 {}",
                    request.expected_revision, current.revision
                ),
                Some(current.revision),
            ));
        }
        if request.patch.is_empty() {
            return Ok(SettingsSnapshot {
                revision: current.revision,
                persisted: true,
                settings: current.settings.clone(),
            });
        }
        let next = current
            .settings
            .apply_patch(&request.patch, &self.allowlist)
            .map_err(AppError::validation)?;
        let next_revision = current.revision.next();
        let snapshot = SettingsSnapshot {
            revision: next_revision,
            persisted: true,
            settings: next.clone(),
        };
        let receipt = Receipt {
            request_id: request.request_id.clone(),
            fingerprint: payload,
            result_json: serde_json::to_string(&snapshot)
                .map_err(|e| AppError::internal(e.to_string()))?,
        };
        let committed = self
            .store
            .commit(&next, Some(current.revision), &receipt)
            .map_err(storage_error)?;
        if committed != next_revision {
            return Err(AppError::internal(format!(
                "存储返回的版本 {committed} 与预期 {next_revision} 不一致"
            )));
        }
        current.settings = next;
        current.revision = next_revision;
        drop(current);
        self.events.emit(AppEvent::SettingsChanged {
            revision: next_revision,
        });
        Ok(snapshot)
    }
}

fn storage_error(error: StorageError) -> AppError {
    match error {
        StorageError::RevisionMismatch { current } => {
            AppError::conflict("持久版本已变化", Some(current))
        }
        other => AppError::storage(other.to_string()),
    }
}
