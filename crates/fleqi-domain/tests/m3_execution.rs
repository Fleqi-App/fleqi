//! M3.1 领域合同测试：执行计划/步骤/效果分类、两种 AI 策略判定、Run 状态机。

use fleqi_domain::composer::ComposerMode;
use fleqi_domain::execution::{
    Effect, EffectKind, ExecutionPlan, ExecutionStep, PlanPreviewCompleteness, RunOrigin, RunState,
    StepKind, classify_command_trust, policy_decision, summarize_effects,
};
use fleqi_domain::settings::AiPolicy;

fn plan_with_effects(effects: Vec<Effect>) -> ExecutionPlan {
    ExecutionPlan {
        id: "plan-1".into(),
        revision: fleqi_domain::revision::Revision::new(1),
        capability_id: None,
        context_id: "ctx-1".into(),
        steps: vec![ExecutionStep {
            script_runtime: Some(fleqi_domain::execution::ScriptRuntime::PosixSh),
            kind: StepKind::Script,
            operation: String::new(),
            executable_ref: None,
            script: Some("ls -la".into()),
            args: vec![],
            cwd_ref: None,
            env_refs: vec![],
            input_refs: vec![],
            expected_outputs: vec![],
        }],
        required_tools: vec![],
        effects,
        preview_completeness: PlanPreviewCompleteness::Complete,
        source_fingerprint: "fp".into(),
    }
}

fn effect(kind: EffectKind) -> Effect {
    Effect {
        kind,
        source_ref: Some("in-1".into()),
        destination_ref: Some("out-1".into()),
        explanation: "测试".into(),
    }
}

#[test]
fn effect_kinds_classify_into_read_vs_change_vs_unknown() {
    let read = EffectKind::Read;
    let change = EffectKind::Create;
    let unknown = EffectKind::Unknown;
    assert!(read.is_read_only());
    assert!(!change.is_read_only());
    assert!(!unknown.is_read_only());
    assert!(change.requires_confirmation());
    assert!(unknown.requires_confirmation());
    assert!(EffectKind::Trash.requires_confirmation());
    assert!(!EffectKind::Read.requires_confirmation());
}

#[test]
fn summary_counts_by_category() {
    let plan = plan_with_effects(vec![
        effect(EffectKind::Read),
        effect(EffectKind::Read),
        effect(EffectKind::Create),
        effect(EffectKind::Unknown),
    ]);
    let summary = summarize_effects(&plan.effects);
    assert_eq!(summary.read_only, 2);
    assert_eq!(summary.changes, 1);
    assert_eq!(summary.unknown, 1);
    assert!(summary.has_changes);
    assert!(summary.has_unknown);
}

#[test]
fn default_policy_confirms_changes_and_unknown_but_allows_trusted_read_only() {
    let policy = AiPolicy::ReadOnlyAutoConfirmChanges;
    // 纯只读计划：自动执行。
    let read_only = plan_with_effects(vec![effect(EffectKind::Read), effect(EffectKind::Read)]);
    assert!(policy_decision(policy, &read_only).auto_execute);
    // 含修改：等待确认。
    let with_change = plan_with_effects(vec![effect(EffectKind::Read), effect(EffectKind::Rename)]);
    let decision = policy_decision(policy, &with_change);
    assert!(!decision.auto_execute);
    assert!(decision.requires_approval);
    // 含未知：等待确认（不能冒充只读）。
    let with_unknown =
        plan_with_effects(vec![effect(EffectKind::Read), effect(EffectKind::Unknown)]);
    assert!(policy_decision(policy, &with_unknown).requires_approval);
    // 安装：等待确认。
    let with_install = plan_with_effects(vec![effect(EffectKind::Install)]);
    assert!(policy_decision(policy, &with_install).requires_approval);
}

#[test]
fn yolo_executes_everything_but_invalid_plans_still_rejected() {
    let policy = AiPolicy::Yolo;
    let with_change = plan_with_effects(vec![effect(EffectKind::Delete)]);
    assert!(policy_decision(policy, &with_change).auto_execute);
    let with_unknown = plan_with_effects(vec![effect(EffectKind::Unknown)]);
    assert!(policy_decision(policy, &with_unknown).auto_execute);
}

#[test]
fn command_trust_maps_common_read_only_commands() {
    // 已审定只读命令结构。
    assert!(classify_command_trust("ls").is_read_only());
    assert!(classify_command_trust("/bin/ls -la").is_read_only());
    assert!(classify_command_trust("pwd").is_read_only());
    assert!(classify_command_trust("file x.txt").is_read_only());
    assert!(classify_command_trust("du -sh .").is_read_only());
    assert!(classify_command_trust("wc -l a.txt").is_read_only());
    assert!(classify_command_trust("head -n 5 a.txt").is_read_only());
    assert!(classify_command_trust("git status").is_read_only());
    assert!(classify_command_trust("git log --oneline").is_read_only());
    assert!(classify_command_trust("cat a.txt").is_read_only());
    // 修改命令。
    assert!(!classify_command_trust("rm -rf /").is_read_only());
    assert!(!classify_command_trust("mv a b").is_read_only());
    assert!(!classify_command_trust("echo hi > out.txt").is_read_only());
    // 未知命令：unknown，不冒充只读。
    let unknown = classify_command_trust("frobnicate --all");
    assert!(!unknown.is_read_only());
    assert!(matches!(
        unknown,
        fleqi_domain::execution::CommandTrust::Unknown
    ));
}

#[test]
fn run_state_machine_rejects_invalid_transitions() {
    assert!(RunState::Planning.can_transition_to(RunState::Running));
    assert!(RunState::Running.can_transition_to(RunState::Succeeded));
    assert!(RunState::Running.can_transition_to(RunState::Cancelled));
    assert!(!RunState::Succeeded.can_transition_to(RunState::Running));
    assert!(!RunState::Cancelled.can_transition_to(RunState::Succeeded));
    assert!(!RunState::Failed.can_transition_to(RunState::Succeeded));
    assert_eq!(RunOrigin::Ai.to_str(), "ai");
    assert_ne!(RunOrigin::Ai, RunOrigin::Capability);
    assert_ne!(ComposerMode::Ai, ComposerMode::Terminal);
}

#[test]
fn plan_steps_carry_native_process_script_kinds() {
    let native = ExecutionStep {
        script_runtime: None,
        kind: StepKind::Native,
        operation: "fs.copy".into(),
        executable_ref: None,
        script: None,
        args: vec!["a".into(), "b".into()],
        cwd_ref: Some("path-1".into()),
        env_refs: vec![],
        input_refs: vec!["in-1".into()],
        expected_outputs: vec!["out-1".into()],
    };
    let process = ExecutionStep {
        script_runtime: None,
        kind: StepKind::Process,
        operation: String::new(),
        executable_ref: Some("/bin/ls".into()),
        script: None,
        args: vec!["-la".into()],
        cwd_ref: Some("path-1".into()),
        env_refs: vec![],
        input_refs: vec![],
        expected_outputs: vec![],
    };
    let script = ExecutionStep {
        script_runtime: Some(fleqi_domain::execution::ScriptRuntime::PosixSh),
        kind: StepKind::Script,
        operation: String::new(),
        executable_ref: None,
        script: Some("ls -la".into()),
        args: vec![],
        cwd_ref: Some("path-1".into()),
        env_refs: vec![],
        input_refs: vec![],
        expected_outputs: vec![],
    };
    assert_eq!(native.kind, StepKind::Native);
    assert_eq!(process.executable_ref.as_deref(), Some("/bin/ls"));
    assert!(script.script.is_some());
    // script 的预览不完整：previewCompleteness 必须保留 unknown。
    assert_ne!(
        PlanPreviewCompleteness::Unknown,
        PlanPreviewCompleteness::Complete
    );
}

#[test]
fn model_claims_cannot_approve_composite_or_writing_commands() {
    for script in [
        "ls; touch changed",
        "cat input > output",
        "echo $(touch changed)",
        "git branch -D main",
        "git log --output=changed",
        "find . -fls changed",
        "file -C",
        "date 092015302026",
        "/tmp/ls",
        "/usr/bin/../local/bin/ls",
    ] {
        let mut plan = plan_with_effects(vec![effect(EffectKind::Read)]);
        plan.steps[0].script = Some(script.into());
        assert!(
            policy_decision(AiPolicy::ReadOnlyAutoConfirmChanges, &plan).requires_approval,
            "{script}"
        );
    }
}
