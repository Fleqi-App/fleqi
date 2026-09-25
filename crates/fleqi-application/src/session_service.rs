//! SessionService（architecture.md §3、§6.1、§8；FR-SESSION-*）：Registry、分页列表、
//! 创建/选择/结束/结束全部/删除/置顶/继续、重启中断标记、16 会话上限。

use fleqi_domain::context::ContextSnapshot;
use fleqi_domain::directory_sync::DirectorySync;
use fleqi_domain::idempotency::{Receipt, ReceiptOutcome, evaluate_receipt};
use fleqi_domain::revision::Revision;
use fleqi_domain::session::{
    ConversationEntry, EntryRole, Session, SessionState, next_state_after_end,
    validate_active_limit,
};
use serde::{Serialize, de::DeserializeOwned};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::dto::{AppError, AppEvent, AppResult};
use crate::ports::{Clock, EventSink, IdGenerator, SessionStore, StorageError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionGroup {
    Active,
    History,
    All,
}

pub struct SessionService {
    store: Arc<dyn SessionStore>,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdGenerator>,
    events: Arc<dyn EventSink>,
    sessions: Mutex<HashMap<String, Session>>,
    request_gate: Mutex<()>,
    end_hook: std::sync::OnceLock<Arc<SessionEndHook>>,
    creation_paused: std::sync::atomic::AtomicBool,
}

type SessionEndHook = dyn Fn(&str) -> AppResult<()> + Send + Sync;

fn storage_error(error: StorageError) -> AppError {
    AppError::storage(error.to_string())
}

impl SessionService {
    /// 加载持久记录；未终结的会话标记为 interrupted（不复活进程）。
    pub fn load(
        store: Arc<dyn SessionStore>,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdGenerator>,
        events: Arc<dyn EventSink>,
    ) -> Result<Self, StorageError> {
        let mut sessions = HashMap::new();
        for mut session in store.load_all()? {
            if session.state.is_active() {
                session.state = SessionState::Interrupted;
                session.terminal_id = None;
                session.ended_at = Some(clock.now_rfc3339());
                session.revision = session.revision.next();
                store.upsert(&session)?;
            }
            sessions.insert(session.id.clone(), session);
        }
        Ok(Self {
            store,
            clock,
            ids,
            events,
            sessions: Mutex::new(sessions),
            request_gate: Mutex::new(()),
            end_hook: std::sync::OnceLock::new(),
            creation_paused: std::sync::atomic::AtomicBool::new(false),
        })
    }

    pub fn set_end_hook(&self, hook: Arc<SessionEndHook>) -> AppResult<()> {
        self.end_hook
            .set(hook)
            .map_err(|_| AppError::conflict("会话结束编排已配置", None))
    }

    /// Close the race between checking idle sessions and replacing the app.
    pub fn pause_creation_for_update(&self) -> AppResult<()> {
        let sessions = self.sessions.lock().expect("sessions");
        if sessions.values().any(|session| session.state.is_active()) {
            return Err(AppError::unavailable("请先结束所有会话，再安装更新"));
        }
        self.creation_paused
            .store(true, std::sync::atomic::Ordering::Release);
        Ok(())
    }

    pub fn resume_creation_after_update(&self) {
        self.creation_paused
            .store(false, std::sync::atomic::Ordering::Release);
    }

    pub fn get(&self, id: &str) -> AppResult<Session> {
        self.sessions
            .lock()
            .expect("sessions")
            .get(id)
            .cloned()
            .ok_or_else(|| AppError::not_found(format!("会话 {id} 不存在")))
    }

    pub fn active_count(&self) -> usize {
        self.sessions
            .lock()
            .expect("sessions")
            .values()
            .filter(|s| s.state.is_active())
            .count()
    }

    /// 置顶在前，组内按最近活动倒序；`offset/limit` 分页。
    pub fn list(&self, group: SessionGroup, offset: usize, limit: usize) -> Vec<Session> {
        let sessions = self.sessions.lock().expect("sessions");
        let mut items: Vec<Session> = sessions
            .values()
            .filter(|s| match group {
                SessionGroup::Active => s.state.is_active(),
                SessionGroup::History => !s.state.is_active(),
                SessionGroup::All => true,
            })
            .cloned()
            .collect();
        items.sort_by(|a, b| {
            b.pinned
                .cmp(&a.pinned)
                .then_with(|| b.last_used_at.cmp(&a.last_used_at))
        });
        items.into_iter().skip(offset).take(limit).collect()
    }

    /// 新建会话：初始上下文来自当前 Finder（可为空，等待选择目录）；不启动 shell。
    pub fn create(
        &self,
        context: Option<&ContextSnapshot>,
        parent_session_id: Option<&str>,
    ) -> AppResult<Session> {
        let mut sessions = self.sessions.lock().expect("sessions");
        let session = self.build_session(&sessions, context, parent_session_id)?;
        self.store.upsert(&session).map_err(storage_error)?;
        sessions.insert(session.id.clone(), session.clone());
        drop(sessions);
        self.emit(&session);
        Ok(session)
    }

    fn build_session(
        &self,
        sessions: &HashMap<String, Session>,
        context: Option<&ContextSnapshot>,
        parent_session_id: Option<&str>,
    ) -> AppResult<Session> {
        if self
            .creation_paused
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(AppError::unavailable("正在安装更新，请稍候"));
        }
        let active = sessions.values().filter(|s| s.state.is_active()).count();
        if let Err(limit) = validate_active_limit(active) {
            return Err(AppError::conflict(
                format!("活跃会话已达上限 {limit}，请先结束已有会话"),
                None,
            ));
        }
        if let Some(parent) = parent_session_id {
            sessions
                .get(parent)
                .ok_or_else(|| AppError::not_found(format!("会话 {parent} 不存在")))?;
        }
        let now = self.clock.now_rfc3339();
        let directory = context
            .and_then(|c| c.directory_ref.as_ref())
            .map(|d| d.display_path.clone());
        let title = directory
            .as_deref()
            .and_then(|d| {
                std::path::Path::new(d)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
            })
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "新会话".to_owned());
        let session = Session {
            id: self.ids.next_id("session"),
            parent_session_id: parent_session_id.map(|s| s.to_owned()),
            title,
            state: SessionState::Active,
            initial_directory: directory.clone(),
            current_directory: directory.clone(),
            target_directory: directory,
            directory_sync: DirectorySync::Synced,
            terminal_id: None,
            pinned: false,
            created_at: now.clone(),
            last_used_at: now,
            ended_at: None,
            revision: Revision::new(1),
        };
        Ok(session)
    }

    /// IPC creation captures context only on the first request; retries replay the original
    /// result even if Finder moved. Both the new session and receipt are committed atomically.
    pub fn create_request(
        &self,
        request_id: &str,
        context: Option<&ContextSnapshot>,
    ) -> AppResult<Session> {
        self.create_with_request(request_id, None, context)
    }

    pub fn continue_request(
        &self,
        request_id: &str,
        history_id: &str,
        context: Option<&ContextSnapshot>,
    ) -> AppResult<Session> {
        self.create_with_request(request_id, Some(history_id), context)
    }

    fn create_with_request(
        &self,
        request_id: &str,
        history_id: Option<&str>,
        context: Option<&ContextSnapshot>,
    ) -> AppResult<Session> {
        validate_request_id(request_id)?;
        let operation = if history_id.is_some() {
            "session_continue"
        } else {
            "session_create"
        };
        let fingerprint = crate::fingerprint::fingerprint(&(operation, history_id));
        let _request = self.request_gate.lock().expect("session requests");
        if let Some(previous) = self.replay(request_id, &fingerprint)? {
            return Ok(previous);
        }
        let mut sessions = self.sessions.lock().expect("sessions");
        if let Some(history_id) = history_id {
            let source = sessions
                .get(history_id)
                .ok_or_else(|| AppError::not_found(format!("会话 {history_id} 不存在")))?;
            if source.state.is_active() {
                return Err(AppError::conflict("只能从已结束的会话继续", None));
            }
        }
        let session = self.build_session(&sessions, context, history_id)?;
        let receipt = receipt(request_id, &fingerprint, &session)?;
        let committed = self
            .store
            .commit_request(std::slice::from_ref(&session), &[], &receipt)
            .map_err(storage_error)?;
        let result = decode_receipt(&committed, &fingerprint)?;
        if committed == receipt {
            sessions.insert(session.id.clone(), session.clone());
            drop(sessions);
            self.emit(&session);
        }
        Ok(result)
    }

    /// Serialize receipt-aware host effects. A successful repeat never re-enters the effect,
    /// including after restart. Effects such as selecting an existing surface are convergent;
    /// creation and deletion use the dedicated transactional methods instead.
    pub fn run_request<T: Serialize + DeserializeOwned>(
        &self,
        request_id: &str,
        operation: &str,
        payload: &impl Serialize,
        execute: impl FnOnce() -> AppResult<T>,
    ) -> AppResult<T> {
        validate_request_id(request_id)?;
        let fingerprint = crate::fingerprint::fingerprint(&(operation, payload));
        let _request = self.request_gate.lock().expect("session requests");
        if let Some(previous) = self.replay(request_id, &fingerprint)? {
            return Ok(previous);
        }
        let result = execute()?;
        let receipt = receipt(request_id, &fingerprint, &result)?;
        let committed = self
            .store
            .commit_request(&[], &[], &receipt)
            .map_err(storage_error)?;
        decode_receipt(&committed, &fingerprint)
    }

    fn replay<T: DeserializeOwned>(
        &self,
        request_id: &str,
        fingerprint: &str,
    ) -> AppResult<Option<T>> {
        self.store
            .find_receipt(request_id)
            .map_err(storage_error)?
            .map(|receipt| decode_receipt(&receipt, fingerprint))
            .transpose()
    }

    /// The expected revision is checked only for a fresh request. Replaying an acknowledged
    /// deletion succeeds even though the session and conversation no longer exist.
    pub fn delete_request(
        &self,
        request_id: &str,
        id: &str,
        expected: Revision,
        stop_active: impl FnOnce(&str) -> AppResult<()>,
    ) -> AppResult<()> {
        validate_request_id(request_id)?;
        let fingerprint = crate::fingerprint::fingerprint(&("session_delete", id, expected));
        let _request = self.request_gate.lock().expect("session requests");
        if let Some(previous) = self.replay(request_id, &fingerprint)? {
            return Ok(previous);
        }
        let original = self.get(id)?;
        if original.revision != expected {
            return Err(AppError::conflict(
                "会话版本已过期",
                Some(original.revision),
            ));
        }
        if original.state.is_active() {
            stop_active(id)?;
        }
        let mut sessions = self.sessions.lock().expect("sessions");
        let latest = sessions
            .get(id)
            .ok_or_else(|| AppError::not_found(format!("会话 {id} 不存在")))?;
        if latest.state.is_active() {
            return Err(AppError::conflict(
                "会话仍在活动，请先结束",
                Some(latest.revision),
            ));
        }
        if !original.state.is_active() && latest.revision != expected {
            return Err(AppError::conflict("会话版本已过期", Some(latest.revision)));
        }
        let receipt = receipt(request_id, &fingerprint, &())?;
        let committed = self
            .store
            .commit_request(&[], &[id.to_owned()], &receipt)
            .map_err(storage_error)?;
        decode_receipt::<()>(&committed, &fingerprint)?;
        sessions.remove(id);
        drop(sessions);
        if committed == receipt {
            self.events.emit(AppEvent::SessionChanged {
                session_id: id.to_owned(),
                revision: Revision::new(0),
                deleted: true,
            });
        }
        Ok(())
    }

    /// 从历史继续：新 ID、关联来源、当前上下文；旧历史保持只读。
    pub fn continue_from(
        &self,
        history_id: &str,
        context: Option<&ContextSnapshot>,
    ) -> AppResult<Session> {
        let source = self.get(history_id)?;
        if source.state.is_active() {
            return Err(AppError::conflict("只能从已结束的会话继续", None));
        }
        self.create(context, Some(history_id))
    }

    pub fn touch(&self, id: &str) -> AppResult<Session> {
        self.update(id, None, |s| {
            s.last_used_at = String::new();
            true
        })
    }

    /// 通用更新：闭包返回 false 表示无变化；`expected` 用于跨窗口一致性。
    pub fn update(
        &self,
        id: &str,
        expected: Option<Revision>,
        mutate: impl FnOnce(&mut Session) -> bool,
    ) -> AppResult<Session> {
        let mut sessions = self.sessions.lock().expect("sessions");
        let session = sessions
            .get_mut(id)
            .ok_or_else(|| AppError::not_found(format!("会话 {id} 不存在")))?;
        if let Some(expected) = expected
            && expected != session.revision
        {
            return Err(AppError::conflict(
                format!("会话版本 {} 已过期", expected),
                Some(session.revision),
            ));
        }
        let mut next = session.clone();
        if !mutate(&mut next) {
            return Ok(session.clone());
        }
        if next.last_used_at.is_empty() {
            next.last_used_at = self.clock.now_rfc3339();
        }
        next.revision = session.revision.next();
        self.store.upsert(&next).map_err(storage_error)?;
        *session = next.clone();
        drop(sessions);
        self.emit(&next);
        Ok(next)
    }

    pub fn set_pinned(
        &self,
        id: &str,
        pinned: bool,
        expected: Option<Revision>,
    ) -> AppResult<Session> {
        self.update(id, expected, |s| {
            if s.pinned == pinned {
                return false;
            }
            s.pinned = pinned;
            true
        })
    }

    pub fn set_terminal(&self, id: &str, terminal_id: Option<String>) -> AppResult<Session> {
        self.update(id, None, |s| {
            s.terminal_id = terminal_id;
            true
        })
    }

    pub fn set_directories(
        &self,
        id: &str,
        current: Option<String>,
        target: Option<String>,
        sync: DirectorySync,
    ) -> AppResult<Session> {
        self.update(id, None, |s| {
            if s.current_directory == current
                && s.target_directory == target
                && s.directory_sync == sync
            {
                return false;
            }
            s.current_directory = current;
            s.target_directory = target;
            s.directory_sync = sync;
            true
        })
    }

    /// 结束：Active → Ending（调用方停止进程后调用 mark_ended）。
    pub fn begin_end(&self, id: &str) -> AppResult<Session> {
        self.get(id)?;
        if let Some(hook) = self.end_hook.get() {
            hook(id)?;
        }
        self.update(id, None, |s| {
            let next = next_state_after_end(s.state);
            if next == s.state {
                return false;
            }
            s.state = next;
            true
        })
    }

    pub fn mark_ended(&self, id: &str, failed: bool) -> AppResult<Session> {
        let now = self.clock.now_rfc3339();
        self.update(id, None, |s| {
            if !s.state.is_active() {
                return false;
            }
            s.state = if failed {
                SessionState::Failed
            } else {
                SessionState::Ended
            };
            s.terminal_id = None;
            s.target_directory = None;
            s.directory_sync = DirectorySync::Synced;
            s.ended_at = Some(now);
            true
        })
    }

    pub fn active_ids(&self) -> Vec<String> {
        self.sessions
            .lock()
            .expect("sessions")
            .values()
            .filter(|s| s.state.is_active())
            .map(|s| s.id.clone())
            .collect()
    }

    /// 删除记录（调用方已结束其进程）；用户文件不在删除范围内。
    pub fn delete(&self, id: &str, expected: Option<Revision>) -> AppResult<()> {
        {
            let sessions = self.sessions.lock().expect("sessions");
            let session = sessions
                .get(id)
                .ok_or_else(|| AppError::not_found(format!("会话 {id} 不存在")))?;
            if let Some(expected) = expected
                && expected != session.revision
            {
                return Err(AppError::conflict("会话版本已过期", Some(session.revision)));
            }
            if session.state.is_active() {
                return Err(AppError::conflict(
                    "会话仍在活动，请先结束",
                    Some(session.revision),
                ));
            }
        }
        self.store.delete(id).map_err(storage_error)?;
        self.sessions.lock().expect("sessions").remove(id);
        self.events.emit(AppEvent::SessionChanged {
            session_id: id.to_owned(),
            revision: Revision::new(0),
            deleted: true,
        });
        Ok(())
    }

    pub fn append_entry(
        &self,
        session_id: &str,
        role: EntryRole,
        content: &str,
        run_id: Option<String>,
    ) -> AppResult<ConversationEntry> {
        let session = self.get(session_id)?;
        let first_user = role == EntryRole::User
            && self
                .store
                .entries(session_id, 1, None)
                .map_err(storage_error)?
                .is_empty();
        let default_title = session
            .initial_directory
            .as_deref()
            .and_then(|d| std::path::Path::new(d).file_name())
            .map(|s| s.to_string_lossy().into_owned());
        let entry = ConversationEntry {
            id: self.ids.next_id("entry"),
            session_id: session_id.to_owned(),
            role,
            content: content.to_owned(),
            run_id,
            created_at: self.clock.now_rfc3339(),
        };
        self.store.append_entry(&entry).map_err(storage_error)?;
        let _ = self.update(session_id, None, |s| {
            if first_user && (s.title == "新会话" || Some(&s.title) == default_title.as_ref()) {
                s.title = content.chars().take(40).collect();
            }
            s.last_used_at = String::new();
            true
        });
        Ok(entry)
    }

    pub fn entries(
        &self,
        session_id: &str,
        limit: usize,
        before: Option<&str>,
    ) -> AppResult<Vec<ConversationEntry>> {
        self.store
            .entries(session_id, limit, before)
            .map_err(storage_error)
    }

    fn emit(&self, session: &Session) {
        self.events.emit(AppEvent::SessionChanged {
            session_id: session.id.clone(),
            revision: session.revision,
            deleted: false,
        });
    }
}

fn validate_request_id(request_id: &str) -> AppResult<()> {
    if request_id.trim().is_empty() {
        Err(AppError::validation(vec![
            fleqi_domain::settings::FieldError {
                field: "requestId".into(),
                code: "invalid".into(),
                message: "requestId 不能为空".into(),
            },
        ]))
    } else {
        Ok(())
    }
}

fn receipt<T: Serialize>(request_id: &str, fingerprint: &str, result: &T) -> AppResult<Receipt> {
    Ok(Receipt {
        request_id: request_id.into(),
        fingerprint: fingerprint.into(),
        result_json: serde_json::to_string(result)
            .map_err(|e| AppError::internal(e.to_string()))?,
    })
}

fn decode_receipt<T: DeserializeOwned>(receipt: &Receipt, fingerprint: &str) -> AppResult<T> {
    match evaluate_receipt(Some(receipt), &receipt.request_id, fingerprint) {
        ReceiptOutcome::Replay(json) => serde_json::from_str(&json)
            .map_err(|e| AppError::storage(format!("会话回执无法解析：{e}"))),
        ReceiptOutcome::Conflict => Err(AppError::conflict(
            "同一 requestId 携带了不同操作或载荷",
            None,
        )),
        ReceiptOutcome::Fresh => Err(AppError::internal("已保存的会话回执无效")),
    }
}
