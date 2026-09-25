import { NativeSelect } from "../../components/ui/native-select";
import { useCallback, useEffect, useRef, useState } from "react";
import type { CapabilityField, ConversationEntry, ExecutionPlan, RunRecord } from "@fleqi/contracts";
import { Play, RotateCcw, Square } from "lucide-react";

import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { Card, DescriptionList } from "../../components/Card";
import { EmptyState, InlineStatus } from "../../components/InlineStatus";
import { isAppError, newRequestId } from "../../adapters/host";
import { useHost } from "../../store/host";

const TERMINAL_STATES = new Set(["succeeded", "failed", "cancelled", "interrupted", "partiallySucceeded"]);

function stateBadge(state: string) {
  if (state === "succeeded") return <Badge tone="success">成功</Badge>;
  if (state === "failed") return <Badge tone="error">失败</Badge>;
  if (state === "cancelled") return <Badge tone="neutral">已取消</Badge>;
  if (state === "awaitingApproval") return <Badge tone="warning">等待确认</Badge>;
  if (state === "running") return <Badge tone="warning">运行中</Badge>;
  const labels: Record<string, string> = { queued: "排队中", generating: "生成计划", predicting: "预览影响", installing: "安装依赖", interrupted: "已中断", partiallySucceeded: "部分成功", awaitingInput: "等待输入" };
  return <Badge tone={state === "partiallySucceeded" ? "warning" : "neutral"}>{labels[state] ?? state}</Badge>;
}

/** Run 列表与详情（ui-design.md §9.2/§9.3）：真实状态、输出与确认/取消/重试。 */
export function RunsPage() {
  const host = useHost();
  const [sessions, setSessions] = useState<{ id: string; title: string }[]>([]);
  const [sessionId, setSessionId] = useState<string>(() => new URLSearchParams(window.location.hash.split("?")[1]).get("session") ?? "");
  const [runs, setRuns] = useState<RunRecord[] | null>(null);
  const [selected, setSelected] = useState<string | null>(() => new URLSearchParams(window.location.hash.split("?")[1]).get("run"));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [plan, setPlan] = useState<ExecutionPlan | null>(null);
  const [planFields, setPlanFields] = useState<CapabilityField[]>([]);
  const [planError, setPlanError] = useState<string | null>(null);
  const requestVersion = useRef(0);
  const [conversation, setConversation] = useState<ConversationEntry[]>([]);
  useEffect(() => {
    let current = true;
    setConversation([]);
    if (sessionId) void host.adapter.sessionEntries(sessionId).then((entries) => { if (current) setConversation(entries); }, (reason: unknown) => { if (current) setError(isAppError(reason) ? reason.message : String(reason)); });
    return () => { current = false; };
  }, [host.adapter, sessionId, host.eventVersion]);
  useEffect(() => {
    const apply = () => {
      const params = new URLSearchParams(window.location.hash.split("?")[1]);
      if (params.get("session")) setSessionId(params.get("session")!);
      if (params.get("run")) setSelected(params.get("run"));
    };
    window.addEventListener("hashchange", apply);
    return () => window.removeEventListener("hashchange", apply);
  }, []);

  useEffect(() => {
    setPlan(null);
    setPlanError(null);
    if (!selected) return;
    let current = true;
    void host.adapter.runPlanGet(selected).then((value) => {
      if (current) {
        setPlan(value);
        setPlanFields([]);
        if (value.capabilityId) void host.adapter.capabilityForm(value.capabilityId).then((form) => { if (current) setPlanFields(form.fields); }, () => {});
      }
    }, (reason: unknown) => {
      if (current) setPlanError(isAppError(reason) ? reason.message : String(reason));
    });
    return () => { current = false; };
  }, [selected, host.adapter, host.eventVersion]);

  useEffect(() => {
    void (async () => {
      try {
        const list = await host.adapter.sessionList();
        const options = [...list.active, ...list.history].map((session) => ({ id: session.id, title: session.title }));
        setSessions(options);
        setSessionId((current) => current || options[0]?.id || "");
      } catch (reason) {
        setError(isAppError(reason) ? `${reason.code}: ${reason.message}` : String(reason));
      }
    })();
  }, [host.adapter, host.eventVersion]);

  const reload = useCallback(async () => {
    const version = ++requestVersion.current;
    if (!sessionId) {
      setRuns([]);
      return;
    }
    try {
      const list = await host.adapter.runList(sessionId);
      if (requestVersion.current !== version) return;
      setRuns(list);
      setError(null);
    } catch (reason) {
      if (requestVersion.current !== version) return;
      setError(isAppError(reason) ? `${reason.code}: ${reason.message}` : String(reason));
    }
  }, [host.adapter, sessionId]);

  useEffect(() => {
    setRuns(null);
    void reload();
    return () => { requestVersion.current += 1; };
  }, [reload]);
  useEffect(() => { void reload(); }, [reload, host.eventVersion]);

  const act = async (action: () => Promise<RunRecord>) => {
    setBusy(true);
    setError(null);
    try {
      const result = await action();
      setSelected(result.id);
      if (result.sessionId !== sessionId) setSessionId(result.sessionId);
      else await reload();
    } catch (reason) {
      setError(isAppError(reason) ? `${reason.code}: ${reason.message}` : String(reason));
    } finally {
      setBusy(false);
    }
  };

  const detail = runs?.find((run) => run.id === selected) ?? null;

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between gap-3">
        <h2 className="text-xl font-semibold leading-7">任务</h2>
        {sessions.length > 0 && (
          <label className="flex items-center gap-2 text-xs text-muted-foreground">
            会话
            <NativeSelect
              className="appearance-none rounded-md border border-border bg-input px-2 py-1 text-xs"
              value={sessionId}
              onChange={(event) => {
                setSessionId(event.target.value);
                setSelected(null);
              }}
            >
              {sessions.map((session) => (
                <option key={session.id} value={session.id}>
                  {session.title}
                </option>
              ))}
            </NativeSelect>
          </label>
        )}
      </div>
      {error && <InlineStatus tone="error">{error}</InlineStatus>}
      {sessions.length === 0 ? (
        <EmptyState title="暂无会话" description="在输入条开始对话后，AI 任务会出现在这里。" />
      ) : runs === null ? (
        <InlineStatus tone="neutral">正在读取任务…</InlineStatus>
      ) : runs.length === 0 ? (
        <EmptyState title="该会话暂无任务" description="通过输入条提交自然语言任务，或使用 ! 前缀直通终端（不产生 Run）。" />
      ) : (
        <div className="space-y-2">
          {runs.map((run) => (
            <button
              key={run.id}
              type="button"
              onClick={() => setSelected(run.id)}
              className={`w-full rounded-lg border px-3 py-2 text-left text-sm transition-colors ${
                selected === run.id ? "border-primary bg-muted/60" : "border-border hover:bg-muted/40"
              }`}
              data-testid="run-row"
              data-run-state={run.state}
            >
              <div className="flex items-center justify-between gap-2">
                <span className="truncate font-medium">{run.prompt || "（无提示）"}</span>
                {stateBadge(run.state)}
              </div>
              <div className="mt-1 flex gap-3 font-mono text-xs text-muted-foreground">
                <span>{run.id}</span>
                <span>rev {run.planRevision}</span>
                <span>{run.updatedAt}</span>
              </div>
            </button>
          ))}
        </div>
      )}
      {conversation.length > 0 && <Card title="会话记录（最近 100 条）"><div className="max-h-96 space-y-3 overflow-auto">{conversation.map((entry) => <article key={entry.id} className="rounded-lg border border-border p-3"><p className="text-xs text-muted-foreground">{{ user: "用户", manualCommand: "终端命令", assistant: "助手", system: "系统" }[entry.role]} · {entry.createdAt}</p><p className="mt-1 whitespace-pre-wrap break-words text-sm">{entry.content}</p>{entry.runId && <Button variant="secondary" onClick={() => setSelected(entry.runId)}>查看关联任务</Button>}</article>)}</div></Card>}
      {detail && (
        <Card
          title="任务详情"
          description={detail.prompt}
          actions={
            <>
              {detail.state === "awaitingApproval" && (
                <Button
                  variant="primary"
                  disabled={busy || !plan || plan.revision !== detail.planRevision || plan.steps.length === 0}
                  onClick={() => void act(() => host.adapter.runApprove(newRequestId("run-approve"), detail.id, detail.planRevision))}
                >
                  <Play aria-hidden="true" />
                  确认执行
                </Button>
              )}
              {!TERMINAL_STATES.has(detail.state) && (
                <Button variant="secondary" disabled={busy} onClick={() => void act(() => host.adapter.runCancel(detail.id))}>
                  <Square aria-hidden="true" />
                  取消
                </Button>
              )}
              {TERMINAL_STATES.has(detail.state) && detail.origin !== "capability" && (
                <Button variant="secondary" disabled={busy} onClick={() => void act(() => host.adapter.runRetry(newRequestId("run-retry"), detail.id))}>
                  <RotateCcw aria-hidden="true" />
                  重试
                </Button>
              )}
            </>
          }
        >
          {detail.origin === "capability" && TERMINAL_STATES.has(detail.state) && plan?.capabilityId && <Button variant="secondary" onClick={() => { window.location.hash = `#/console/library?capability=${encodeURIComponent(plan.capabilityId!)}`; }}>重新选择输入与参数</Button>}
          <section className="mb-4 space-y-2" aria-label="执行计划">
            <h4 className="text-sm font-medium">执行计划与影响</h4>
            {planError ? <InlineStatus tone="warning">{planError}</InlineStatus> : !plan ? <InlineStatus tone="loading">读取计划…</InlineStatus> : <>
              {plan.steps.map((step, index) => <div key={index} className="rounded-lg border border-border p-3">
                <p className="mb-1 text-xs text-muted-foreground">步骤 {index + 1} · {step.cwdRef ? "输出目标见下方" : `目录 ${detail.directoryDisplay || "历史未记录"}`}</p>
                {step.expectedOutputs.map((output) => <p key={output} className="mb-2 break-all text-xs">{output}</p>)}
                {step.kind === "native" ? <NativeParameters args={step.args} fields={planFields} /> : <pre className="overflow-auto whitespace-pre-wrap break-all font-mono text-xs">{step.script ?? [step.executableRef ?? step.operation, ...step.args].join(" ")}</pre>}
              </div>)}
              {plan.effects.map((effect, index) => <p key={index} className="break-all text-xs text-muted-foreground">{effect.explanation}</p>)}
              {plan.previewCompleteness === "unknown" && <InlineStatus tone="warning">影响范围无法完整预测，请核对命令与目标。</InlineStatus>}
              {plan.steps.length === 0 && <InlineStatus tone="warning">没有可核查的执行步骤，无法确认执行。</InlineStatus>}
            </>}
          </section>
          <DescriptionList
            items={[
              { label: "状态", value: stateBadge(detail.state) },
              { label: "策略", value: detail.policy === "yolo" ? "直接执行" : "只读自动执行，变更需要确认" },
              { label: "计划版本", value: detail.planRevision, mono: true },
              { label: "执行目录", value: detail.directoryDisplay || "历史记录未保存目录", mono: true },
              { label: "退出码", value: detail.exitStatus === null ? "—" : String(detail.exitStatus), mono: true },
              { label: "更新时间", value: detail.updatedAt, mono: true },
            ]}
          />
          {detail.stepResults.length > 0 && <ol aria-label="逐步执行结果" className="mt-3 space-y-2">
            {detail.stepResults.map((step) => <li key={step.index} className="rounded-lg border border-border p-2 text-sm">
              <span className="mr-2">步骤 {step.index + 1}</span>{stateBadge(step.state)}
              {step.exitStatus !== null && <span className="ml-2 text-xs text-muted-foreground">退出码 {step.exitStatus}</span>}
              {step.message && <p className="mt-1 break-all text-xs text-muted-foreground">{step.message}</p>}
            </li>)}
          </ol>}
          <pre className="mt-3 max-h-72 overflow-auto rounded-md bg-muted/60 p-2 font-mono text-xs whitespace-pre-wrap" data-testid="run-output">
            {detail.output || "（暂无输出）"}
          </pre>
        </Card>
      )}
    </div>
  );
}


export function NativeParameters({ args, fields }: { args: string[]; fields: CapabilityField[] }) {
  let parameters: unknown;
  try { parameters = JSON.parse(args[0] ?? "{}"); } catch { return <pre className="whitespace-pre-wrap break-all text-xs">{args.join(" ")}</pre>; }
  if (!parameters || typeof parameters !== "object" || Array.isArray(parameters)) return <p className="text-warning">参数格式不可读取</p>;
  const entries = Object.entries(parameters as Record<string, unknown>).filter(([key]) => !key.startsWith("_"));
  return entries.length ? <dl className="grid grid-cols-[minmax(6rem,auto)_1fr] gap-x-4 gap-y-2 text-xs">{entries.map(([key, value]) => {
    const field = fields.find((field) => field.key === key);
    const secret = field?.kind === "password" || (typeof value === "string" && value.startsWith("secret-ref:"));
    return <div className="contents" key={key}><dt className="text-muted-foreground">{field?.label ?? key}</dt><dd className="whitespace-pre-wrap break-all">{secret ? value ? "已提供（仅本次有效）" : "未提供" : String(value || "未指定")}</dd></div>;
  })}</dl> : <p className="text-xs text-muted-foreground">使用已固定的当前上下文，无附加参数。</p>;
}
