//! PermissionService：无提示检测、显式申请（同一权限至多一个在途）、撤销推导、
//! 代际失效（architecture.md §12.2、§12.4）。权限是系统事实，不从 SQLite 恢复。

use fleqi_domain::permissions::{Permission, PermissionProcedure, PermissionRecord};
use fleqi_domain::revision::Revision;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use crate::dto::{
    AppError, AppEvent, AppResult, OperationState, PermissionOperation, PermissionSnapshot,
};
use crate::lifecycle::HostLifecycle;
use crate::ports::{Clock, EventSink, PermissionPort, PermissionProbe, Spawner};

struct State {
    records: BTreeMap<u8, PermissionRecord>,
    revision: Revision,
    inflight: HashMap<u8, PermissionOperation>,
    operations: u64,
}

fn key(permission: Permission) -> u8 {
    match permission {
        Permission::FinderAutomation => 0,
        Permission::Accessibility => 1,
    }
}

pub struct PermissionService {
    port: Arc<dyn PermissionPort>,
    clock: Arc<dyn Clock>,
    events: Arc<dyn EventSink>,
    lifecycle: Arc<HostLifecycle>,
    spawner: Arc<dyn Spawner>,
    state: Mutex<State>,
}

impl PermissionService {
    pub fn new(
        port: Arc<dyn PermissionPort>,
        clock: Arc<dyn Clock>,
        events: Arc<dyn EventSink>,
        lifecycle: Arc<HostLifecycle>,
        spawner: Arc<dyn Spawner>,
    ) -> Arc<Self> {
        let records = Permission::ALL
            .iter()
            .map(|p| (key(*p), PermissionRecord::unknown(*p)))
            .collect();
        Arc::new(Self {
            port,
            clock,
            events,
            lifecycle,
            spawner,
            state: Mutex::new(State {
                records,
                revision: Revision::new(0),
                inflight: HashMap::new(),
                operations: 0,
            }),
        })
    }

    pub fn snapshot(&self) -> PermissionSnapshot {
        let state = self.state.lock().expect("permissions 锁");
        PermissionSnapshot {
            revision: state.revision,
            records: state.records.values().cloned().collect(),
        }
    }

    /// 无提示重检全部权限（阻塞调用，宿主放在阻塞线程）。
    pub fn check_all(&self) -> PermissionSnapshot {
        for permission in Permission::ALL {
            self.check(permission);
        }
        self.snapshot()
    }

    pub fn check(&self, permission: Permission) -> PermissionRecord {
        let probe = self.port.check(permission, PermissionProcedure::Passive);
        self.apply(permission, probe, PermissionProcedure::Passive)
    }

    /// 显式申请：立即返回 operationId，结果异步更新并广播。宿主停止后的迟到结果丢弃。
    pub fn request(self: &Arc<Self>, permission: Permission) -> AppResult<PermissionOperation> {
        if !self.lifecycle.state().accepts_changes()
            && self.lifecycle.state() != fleqi_domain::lifecycle::HostState::Degraded
        {
            return Err(AppError::unavailable("宿主正在停止，不再申请权限"));
        }
        let operation = {
            let mut state = self.state.lock().expect("permissions 锁");
            if let Some(existing) = state.inflight.get(&key(permission)) {
                return Ok(existing.clone());
            }
            state.operations += 1;
            let operation = PermissionOperation {
                operation_id: format!("perm-op-{}", state.operations),
                permission,
                state: OperationState::InProgress,
                started_at: self.clock.now_rfc3339(),
            };
            state.inflight.insert(key(permission), operation.clone());
            operation
        };
        let generation = self.lifecycle.generation();
        let service = Arc::clone(self);
        self.spawner.spawn(Box::new(move || {
            let probe = service
                .port
                .check(permission, PermissionProcedure::Explicit);
            if !service.lifecycle.generation().is_current(generation) {
                service
                    .state
                    .lock()
                    .expect("permissions 锁")
                    .inflight
                    .remove(&key(permission));
                return;
            }
            service.apply(permission, probe, PermissionProcedure::Explicit);
            service
                .state
                .lock()
                .expect("permissions 锁")
                .inflight
                .remove(&key(permission));
        }));
        Ok(operation)
    }

    pub fn open_system_settings(&self, permission: Permission) -> AppResult<()> {
        self.port
            .open_system_settings(permission)
            .map_err(|e| AppError::platform(e, true))
    }

    pub fn inflight(&self, permission: Permission) -> Option<PermissionOperation> {
        self.state
            .lock()
            .expect("permissions 锁")
            .inflight
            .get(&key(permission))
            .cloned()
    }

    fn apply(
        &self,
        permission: Permission,
        probe: PermissionProbe,
        procedure: PermissionProcedure,
    ) -> PermissionRecord {
        let now = self.clock.now_rfc3339();
        let revision = {
            let mut state = self.state.lock().expect("permissions 锁");
            let previous = state
                .records
                .get(&key(permission))
                .cloned()
                .unwrap_or_else(|| PermissionRecord::unknown(permission));
            let mut record = previous.transition(probe.status, procedure, &now);
            if let Some(error) = probe.error {
                record = record.with_error(error);
            }
            state.records.insert(key(permission), record);
            state.revision = state.revision.next();
            state.revision
        };
        self.events.emit(AppEvent::PermissionsChanged { revision });
        self.state
            .lock()
            .expect("permissions 锁")
            .records
            .get(&key(permission))
            .cloned()
            .expect("已写入")
    }
}
