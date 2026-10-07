//! 执行计划、Run 与 AI 策略（architecture.md §4、§7；FR-RUN/FR-POLICY）。
//! 两种策略只作用于 AI；origin 由调用端口确定，模型不能自报 manual。

use crate::revision::Revision;
use crate::settings::AiPolicy;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum StepKind {
    Native,
    Process,
    Script,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum ScriptRuntime {
    PosixSh,
    WindowsPowerShell,
}

impl ScriptRuntime {
    pub fn current() -> Self {
        if cfg!(windows) {
            Self::WindowsPowerShell
        } else {
            Self::PosixSh
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ExecutionStep {
    /// 旧计划缺失时不猜测 Windows 解释器；必须重新规划。
    #[serde(default)]
    pub script_runtime: Option<ScriptRuntime>,
    pub kind: StepKind,
    /// native 步骤的操作名（如 fs.copy）；process/script 为空。
    pub operation: String,
    /// process 步骤的可执行文件路径。
    pub executable_ref: Option<String>,
    /// script 步骤的显式脚本（解释器由计划声明）。
    pub script: Option<String>,
    pub args: Vec<String>,
    pub cwd_ref: Option<String>,
    pub env_refs: Vec<String>,
    pub input_refs: Vec<String>,
    pub expected_outputs: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum EffectKind {
    Read,
    Create,
    Copy,
    Move,
    Rename,
    Modify,
    Overwrite,
    Trash,
    Delete,
    NetworkWrite,
    SystemChange,
    Install,
    Unknown,
}

impl EffectKind {
    pub fn is_read_only(self) -> bool {
        matches!(self, EffectKind::Read)
    }

    /// 修改/未知/安装/网络写入/系统变更都要求确认（默认策略）。
    pub fn requires_confirmation(self) -> bool {
        !self.is_read_only()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Effect {
    pub kind: EffectKind,
    pub source_ref: Option<String>,
    pub destination_ref: Option<String>,
    pub explanation: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum PlanPreviewCompleteness {
    /// 计划可推导全部影响。
    Complete,
    /// 自由脚本等无法静态预知全部影响；unknown 保留。
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ExecutionPlan {
    pub id: String,
    pub revision: Revision,
    pub capability_id: Option<String>,
    pub context_id: String,
    pub steps: Vec<ExecutionStep>,
    pub required_tools: Vec<String>,
    pub effects: Vec<Effect>,
    pub preview_completeness: PlanPreviewCompleteness,
    pub source_fingerprint: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectSummary {
    pub read_only: usize,
    pub changes: usize,
    pub unknown: usize,
    pub has_changes: bool,
    pub has_unknown: bool,
}

pub fn summarize_effects(effects: &[Effect]) -> EffectSummary {
    let mut summary = EffectSummary {
        read_only: 0,
        changes: 0,
        unknown: 0,
        has_changes: false,
        has_unknown: false,
    };
    for effect in effects {
        match effect.kind {
            EffectKind::Read => summary.read_only += 1,
            EffectKind::Unknown => {
                summary.unknown += 1;
                summary.has_unknown = true;
            }
            _ => {
                summary.changes += 1;
                summary.has_changes = true;
            }
        }
    }
    summary
}

/// 策略判定（FR-POLICY-001/002）：模型自称"只读"不参与判定——只看效果分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyDecision {
    pub auto_execute: bool,
    pub requires_approval: bool,
}

pub fn policy_decision(policy: AiPolicy, plan: &ExecutionPlan) -> PolicyDecision {
    match policy {
        AiPolicy::Yolo => PolicyDecision {
            auto_execute: true,
            requires_approval: false,
        },
        AiPolicy::ReadOnlyAutoConfirmChanges => {
            let summary = summarize_effects(&plan.effects);
            let trusted_read_only = !summary.has_changes
                && !summary.has_unknown
                && !plan.steps.is_empty()
                && plan.preview_completeness == PlanPreviewCompleteness::Complete
                && plan.steps.iter().all(|step| match step.kind {
                    StepKind::Script => {
                        step.script_runtime != Some(ScriptRuntime::WindowsPowerShell)
                            && step
                                .script
                                .as_deref()
                                .is_some_and(|script| classify_command_trust(script).is_read_only())
                    }
                    StepKind::Process => step.executable_ref.as_ref().is_some_and(|executable| {
                        classify_command_trust(&format!("{} {}", executable, step.args.join(" ")))
                            .is_read_only()
                    }),
                    StepKind::Native => matches!(
                        step.operation.as_str(),
                        "CAP-FILE-009"
                            | "CAP-FILE-010"
                            | "CAP-FILE-011"
                            | "CAP-FILE-012"
                            | "CAP-FILE-013"
                            | "CAP-FILE-014"
                            | "CAP-FILE-015"
                            | "CAP-FILE-016"
                            | "CAP-FILE-017"
                            | "CAP-FILE-018"
                            | "CAP-FILE-019"
                            | "CAP-FILE-020"
                            | "CAP-DEV-001"
                            | "CAP-DEV-005"
                            | "CAP-DEV-006"
                            | "CAP-CALC-001"
                            | "CAP-CALC-002"
                            | "CAP-CALC-003"
                            | "CAP-CALC-004"
                            | "CAP-TOOLS-001"
                            | "CAP-TOOLS-002"
                            | "CAP-SYSTEM-009"
                            | "CAP-SYSTEM-010"
                            | "CAP-SYSTEM-011"
                            | "CAP-SYSTEM-012"
                            | "CAP-SYSTEM-013"
                            | "CAP-NETWORK-002"
                            | "CAP-NETWORK-003"
                            | "CAP-PDF-008"
                            | "CAP-TEXT-007"
                            | "CAP-TEXT-008"
                            | "CAP-TEXT-009"
                            | "CAP-TEXT-010"
                            | "CAP-TEXT-011"
                            | "CAP-PDF-006"
                            | "CAP-PDF-007"
                            | "CAP-IMAGE-005"
                            | "CAP-IMAGE-006"
                            | "CAP-MEDIA-005"
                            | "CAP-MEDIA-006"
                            | "CAP-MEDIA-007"
                            | "CAP-MEDIA-008"
                            | "CAP-ZIP-002"
                            | "CAP-TEXT-001"
                            | "CAP-TEXT-003"
                            | "CAP-TEXT-006"
                            | "file.list"
                            | "file.inspect"
                            | "text.read"
                            | "zip.list"
                            | "pdf.inspect"
                            | "media.inspect"
                            | "image.inspect"
                            | "calculate"
                    ),
                });
            PolicyDecision {
                auto_execute: trusted_read_only,
                requires_approval: !trusted_read_only,
            }
        }
    }
}

/// 已审定的只读命令结构（Process/Script 步骤信任判定辅助）。
/// 未知命令一律 Unknown，不冒充只读（NFR-SEC-002）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandTrust {
    /// 已知只读。
    ReadOnly,
    /// 已知会修改。
    Changes,
    /// 无法可靠分类。
    Unknown,
}

impl CommandTrust {
    pub fn is_read_only(self) -> bool {
        matches!(self, CommandTrust::ReadOnly)
    }
}

/// 以命令首词（绝对路径取文件名）对照已审定只读命令集；参数不参与放行判断。
pub fn classify_command_trust(command: &str) -> CommandTrust {
    // 任意复合 shell 语法、替换、重定向都不能用首词放行。模型申报的 effects
    // 不构成只读证明；复杂但实际只读的命令仍可在影响预览后由用户确认。
    if command.contains([
        ';', '&', '|', '>', '<', '$', '`', '\n', '\r', '\\', '(', ')', '{', '}',
    ]) {
        return CommandTrust::Unknown;
    }
    let first = command.split_whitespace().next().unwrap_or("");
    if first.contains("/../") || first.contains("/./") {
        return CommandTrust::Unknown;
    }
    if first.contains('/')
        && !first.starts_with("/bin/")
        && !first.starts_with("/usr/bin/")
        && !first.starts_with("/usr/sbin/")
    {
        return CommandTrust::Unknown;
    }
    let name = first.rsplit('/').next().unwrap_or(first);
    match name {
        "ls" | "pwd" | "file" | "du" | "df" | "wc" | "head" | "tail" | "cat" | "stat" | "which"
        | "whoami" | "uname" | "sw_vers" | "sysctl" | "date" | "echo" | "printf" | "git"
        | "grep" | "find" | "md5" | "shasum" => {
            if name == "git" {
                if command.split_whitespace().any(|arg| {
                    arg == "-c"
                        || ["--output", "--exec-path", "--ext-diff", "--textconv"]
                            .iter()
                            .any(|flag| arg.starts_with(flag))
                }) {
                    return CommandTrust::Unknown;
                }
                // git 子命令区分读写。
                let sub = command.split_whitespace().nth(1).unwrap_or("");
                match sub {
                    "status" | "log" | "diff" | "show" | "rev-parse" | "ls-files" | "blame" => {
                        CommandTrust::ReadOnly
                    }
                    "branch" if command.split_whitespace().count() == 2 => CommandTrust::ReadOnly,
                    "remote" if command.split_whitespace().skip(2).all(|arg| arg == "-v") => {
                        CommandTrust::ReadOnly
                    }
                    _ => CommandTrust::Unknown,
                }
            } else if (name == "file"
                && command
                    .split_whitespace()
                    .any(|arg| arg == "-C" || arg == "--compile"))
                || (name == "sysctl"
                    && (command.contains('=')
                        || command
                            .split_whitespace()
                            .any(|arg| matches!(arg, "-w" | "-f"))))
            {
                CommandTrust::Changes
            } else if name == "date"
                && command
                    .split_whitespace()
                    .skip(1)
                    .any(|arg| !arg.starts_with('+') && !matches!(arg, "-u" | "-R" | "-I"))
            {
                CommandTrust::Unknown
            } else if name == "find" {
                // find 默认只读；带 -delete/-exec 为修改。
                if ["-delete", "-exec", "-ok", "-fprint", "-fls"]
                    .iter()
                    .any(|flag| command.contains(flag))
                {
                    CommandTrust::Changes
                } else {
                    CommandTrust::ReadOnly
                }
            } else if name == "echo" {
                // echo 带重定向是修改；重定向由 shell 处理，此处从文本识别。
                if command.contains('>') {
                    CommandTrust::Changes
                } else {
                    CommandTrust::ReadOnly
                }
            } else {
                CommandTrust::ReadOnly
            }
        }
        "rm" | "mv" | "cp" | "mkdir" | "rmdir" | "touch" | "chmod" | "chown" | "ln" | "tee"
        | "kill" | "pkill" | "defaults" | "networksetup" | "osascript" | "npm" | "pnpm"
        | "brew" | "curl" | "wget" | "rsync" | "ditto" | "xattr" | "chflags" => {
            CommandTrust::Changes
        }
        "" => CommandTrust::Unknown,
        _ => CommandTrust::Unknown,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum RunOrigin {
    Ai,
    Capability,
}

impl RunOrigin {
    pub fn to_str(self) -> &'static str {
        match self {
            RunOrigin::Ai => "ai",
            RunOrigin::Capability => "capability",
        }
    }
}

/// Run 状态（FR-RUN-001）：摘要状态独立于 Run 状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum RunState {
    Planning,
    AwaitingInput,
    AwaitingApproval,
    Installing,
    Queued,
    Running,
    Succeeded,
    PartiallySucceeded,
    Failed,
    Cancelled,
    Interrupted,
}

impl RunState {
    pub fn can_transition_to(self, next: RunState) -> bool {
        if self == next {
            return true;
        }
        matches!(
            (self, next),
            (RunState::Planning, RunState::AwaitingInput)
                | (RunState::Planning, RunState::AwaitingApproval)
                | (RunState::Planning, RunState::Queued)
                | (RunState::Planning, RunState::Running)
                | (RunState::Planning, RunState::Failed)
                | (RunState::Planning, RunState::Cancelled)
                | (RunState::AwaitingInput, RunState::Planning)
                | (RunState::AwaitingApproval, RunState::Installing)
                | (RunState::AwaitingApproval, RunState::Queued)
                | (RunState::AwaitingApproval, RunState::Cancelled)
                | (RunState::Installing, RunState::Queued)
                | (RunState::Installing, RunState::Failed)
                | (RunState::Installing, RunState::Cancelled)
                | (RunState::Queued, RunState::Running)
                | (RunState::Queued, RunState::Cancelled)
                | (RunState::Running, RunState::Succeeded)
                | (RunState::Running, RunState::PartiallySucceeded)
                | (RunState::Running, RunState::Failed)
                | (RunState::Running, RunState::Cancelled)
        )
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            RunState::Succeeded
                | RunState::PartiallySucceeded
                | RunState::Failed
                | RunState::Cancelled
                | RunState::Interrupted
        )
    }
}
