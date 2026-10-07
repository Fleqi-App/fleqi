//! 自动 cd 协议状态机（architecture.md §5.2、ui-design §3.3、FR-CTX-002..007）。
//! 区分"Finder 目标目录"与"shell 已实际进入的目录"；忙碌/编辑行非空/前台非 shell 时 pending；
//! 只应用最新目标；过期回执不覆盖；等待中的 `!` 命令遇目标变化撤销自动投递并保留草稿。

use crate::revision::Revision;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum DirectorySync {
    Synced,
    Pending,
    Syncing,
    Failed,
}

/// 终端适配器报告的安全性证据；不能仅靠输出文本像提示符判定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellReadiness {
    pub prompt_ready: bool,
    pub edit_line_empty: bool,
    pub foreground_is_shell: bool,
    /// 正在投递用户按键。
    pub delivering: bool,
}

impl ShellReadiness {
    pub fn is_safe(&self) -> bool {
        self.prompt_ready && self.edit_line_empty && self.foreground_is_shell && !self.delivering
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct QueuedLine {
    pub request_id: String,
    pub session_id: String,
    pub context_revision: Revision,
    /// 提交时显示的目标目录（显示路径；宿主另持原生路径）。
    pub target: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncOutcome {
    Confirmed { cwd: String },
    Failed { cwd: String, message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncDecision {
    /// 由 terminal 模块生成的 builtin cd 控制消息（不是 AI 命令）。
    SendCd {
        target: String,
        revision: Revision,
    },
    Pending {
        target: String,
    },
    Synced {
        cwd: String,
    },
    /// 同步成功且提交时目标未变：发送一次等待中的 `!` 命令。
    SyncedAndSend {
        cwd: String,
        line: QueuedLine,
    },
    Failed {
        cwd: String,
        message: String,
    },
}

#[derive(Debug, Clone)]
pub struct SyncMachine {
    state: DirectorySync,
    current: String,
    target: Option<(String, Revision)>,
    inflight: Option<(String, Revision)>,
    queued: Option<QueuedLine>,
    withdrawn: Option<QueuedLine>,
    last_error: Option<String>,
}

impl SyncMachine {
    pub fn new(current_cwd: impl Into<String>) -> Self {
        Self {
            state: DirectorySync::Synced,
            current: current_cwd.into(),
            target: None,
            inflight: None,
            queued: None,
            withdrawn: None,
            last_error: None,
        }
    }

    pub fn state(&self) -> DirectorySync {
        self.state
    }

    pub fn current(&self) -> &str {
        &self.current
    }

    pub fn pending_target(&self) -> Option<&str> {
        self.target.as_ref().map(|(t, _)| t.as_str())
    }

    pub fn queued_line(&self) -> Option<&QueuedLine> {
        self.queued.as_ref()
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// 取出因目标变化/后台而撤销的命令（草稿恢复给 UI）。
    pub fn take_withdrawn_line(&mut self) -> Option<QueuedLine> {
        self.withdrawn.take()
    }

    fn withdraw_queued(&mut self) {
        if let Some(line) = self.queued.take() {
            self.withdrawn = Some(line);
        }
    }

    /// Finder 目标变化；`visible` 为 false 时（后台会话）不改目标。
    pub fn target_changed(
        &mut self,
        target: &str,
        revision: Revision,
        readiness: &ShellReadiness,
        visible: bool,
    ) -> SyncDecision {
        if !visible {
            return SyncDecision::Pending {
                target: self.current.clone(),
            };
        }
        let changed = self
            .target
            .as_ref()
            .map(|(t, _)| t != target)
            .unwrap_or(true);
        if changed
            && self
                .queued
                .as_ref()
                .map(|q| q.target != target)
                .unwrap_or(false)
        {
            self.withdraw_queued();
        }
        if target == self.current && self.inflight.is_none() {
            self.target = None;
            self.state = DirectorySync::Synced;
            return SyncDecision::Synced {
                cwd: self.current.clone(),
            };
        }
        self.target = Some((target.to_owned(), revision));
        self.try_send(readiness)
    }

    fn try_send(&mut self, readiness: &ShellReadiness) -> SyncDecision {
        let Some((target, revision)) = self.target.clone() else {
            return SyncDecision::Synced {
                cwd: self.current.clone(),
            };
        };
        if readiness.is_safe() && self.inflight.is_none() {
            self.inflight = Some((target.clone(), revision));
            self.state = DirectorySync::Syncing;
            SyncDecision::SendCd { target, revision }
        } else {
            self.state = DirectorySync::Pending;
            SyncDecision::Pending { target }
        }
    }

    /// shell 报告到达安全提示符；后台（visible=false）不投递。
    pub fn shell_became_safe(
        &mut self,
        readiness: &ShellReadiness,
        visible: bool,
    ) -> Option<SyncDecision> {
        if !visible || self.target.is_none() || self.inflight.is_some() || !readiness.is_safe() {
            return None;
        }
        Some(self.try_send(readiness))
    }

    pub fn retry(&mut self, readiness: &ShellReadiness, visible: bool) -> Option<SyncDecision> {
        if self.state != DirectorySync::Failed {
            return None;
        }
        self.last_error = None;
        self.shell_became_safe(readiness, visible)
    }

    /// 只有执行器确认尚未执行时才释放在途请求；最新目标和命令草稿继续保留。
    pub fn on_cd_cancelled(&mut self, revision: Revision) {
        if self
            .inflight
            .as_ref()
            .is_some_and(|(_, current)| *current == revision)
        {
            self.inflight = None;
            self.state = if self.target.is_some() {
                DirectorySync::Pending
            } else {
                DirectorySync::Synced
            };
        }
    }

    /// cd 回执：核对版本；过期回执忽略。
    pub fn on_cd_result(
        &mut self,
        revision: Revision,
        outcome: SyncOutcome,
    ) -> Option<SyncDecision> {
        let (target, inflight_revision) = self.inflight.clone()?;
        if inflight_revision != revision {
            return None;
        }
        self.inflight = None;
        match outcome {
            SyncOutcome::Confirmed { cwd } => {
                self.current = cwd.clone();
                self.last_error = None;
                let latest_is_this = self
                    .target
                    .as_ref()
                    .map(|(t, r)| *t == target && *r == revision)
                    .unwrap_or(false);
                if latest_is_this {
                    self.target = None;
                }
                if self.target.is_some() {
                    // 同步期间目标又变：保持 pending，等待下一次安全提示符。
                    self.state = DirectorySync::Pending;
                    return Some(SyncDecision::Synced { cwd });
                }
                self.state = DirectorySync::Synced;
                // 目标变化时的撤销已在 target_changed 完成（queued.target 与新目标不符即撤销）；
                // 能走到这里说明确认的同步就是提交时的目标版本，自动投递一次。
                match self.queued.take() {
                    Some(line) => Some(SyncDecision::SyncedAndSend { cwd, line }),
                    None => Some(SyncDecision::Synced { cwd }),
                }
            }
            SyncOutcome::Failed { cwd, message } => {
                self.current = cwd.clone();
                self.state = DirectorySync::Failed;
                self.last_error = Some(message.clone());
                Some(SyncDecision::Failed { cwd, message })
            }
        }
    }

    /// 每会话仅保留一个等待发送的操作栏命令。
    pub fn queue_line(&mut self, line: QueuedLine) -> Result<(), QueuedLine> {
        if let Some(existing) = &self.queued {
            return Err(existing.clone());
        }
        self.queued = Some(line);
        Ok(())
    }

    pub fn cancel_queued(&mut self) -> Option<QueuedLine> {
        self.queued.take()
    }

    /// 会话退到后台或 keepAll 隐藏：撤销尚未投递的自动 cd 与 queuedLine；在途 cd 的回执仍只更新真实 cwd。
    pub fn went_background(&mut self) -> Option<QueuedLine> {
        self.target = None;
        if self.inflight.is_none() {
            self.state = DirectorySync::Synced;
        }
        let line = self.queued.take();
        if let Some(l) = &line {
            self.withdrawn = Some(l.clone());
        }
        line
    }

    /// 用户在终端自行 cd：更新真实目录，不拉回 Finder。
    pub fn manual_cwd_changed(&mut self, cwd: &str) {
        self.current = cwd.to_owned();
        if self.inflight.is_none() && self.target.is_none() {
            self.state = DirectorySync::Synced;
        }
    }
}
