import { useEffect, useState } from "react";
import { ArrowLeft, ChevronRight, ExternalLink, ListChecks, Loader2, X } from "lucide-react";
import type { ExecutionPlan, RunRecord } from "@fleqi/contracts";
import { Button } from "../../components/ui/button";
import { Badge } from "../../components/Badge";
import { InlineStatus } from "../../components/InlineStatus";
import { useHost } from "../../store/host";
import { isAppError, newRequestId } from "../../adapters/host";
import { NativeParameters } from "../console/RunsPage";

const terminal = new Set(["succeeded", "failed", "cancelled", "interrupted", "partiallySucceeded"]);
const labels: Record<string, string> = { queued: "排队中", running: "运行中", succeeded: "已完成", failed: "失败", cancelled: "已取消", interrupted: "已中断", awaitingApproval: "等待确认", partiallySucceeded: "部分完成", generating: "生成计划", predicting: "预览影响", awaitingInput: "等待输入", installing: "安装中" };

export interface PlanningView { requestId: string; prompt: string; cancelling: boolean }

export function TaskPanel({ sessionId, initialRunId, planning = null, message = null, onCancelPlanning, onClose }: { sessionId: string; initialRunId?: string | null; planning?: PlanningView | null; message?: string | null; onCancelPlanning?: () => void; onClose: () => void }) {
  const host = useHost();
  const [runs, setRuns] = useState<RunRecord[]>([]);
  const [selected, setSelected] = useState<string | null>(initialRunId ?? null);
  const [detail, setDetail] = useState<RunRecord | null>(null);
  const [plan, setPlan] = useState<ExecutionPlan | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => { setDetail(null); setPlan(null); setSelected(initialRunId ?? null); }, [initialRunId]);
  useEffect(() => {
    let alive = true;
    void host.adapter.runList(sessionId).then((list) => { if (alive) setRuns(list); }, (reason: unknown) => { if (alive) setError(isAppError(reason) ? reason.message : String(reason)); });
    return () => { alive = false; };
  }, [host.adapter, host.eventVersion, sessionId]);
  useEffect(() => {
    if (!selected) return;
    let alive = true;
    void Promise.all([host.adapter.runGet(selected), host.adapter.runPlanGet(selected)]).then(([run, nextPlan]) => {
      if (alive) { setDetail(run); setPlan(nextPlan); setError(null); }
    }, (reason: unknown) => { if (alive) setError(isAppError(reason) ? reason.message : String(reason)); });
    return () => { alive = false; };
  }, [host.adapter, selected, host.eventVersion]);
  const act = async (action: () => Promise<RunRecord>) => {
    setBusy(true); setError(null);
    try { const run = await action(); if (selected !== run.id) setPlan(null); setDetail(run); setSelected(run.id); }
    catch (reason) { setError(isAppError(reason) ? reason.message : String(reason)); }
    finally { setBusy(false); }
  };
  const choose = (id: string) => { setDetail(null); setPlan(null); setSelected(id); };
  return <section aria-label="任务浮层" data-testid="task-panel" className="composer-panel absolute bottom-[78px] right-2 top-2 z-20 w-[min(620px,calc(100%-16px))] overflow-hidden rounded-2xl border border-border/70 bg-card text-foreground shadow-lg">
    {planning ? <div className="flex h-full min-h-0 flex-col" data-testid="planning-status">
      <header className="flex h-11 shrink-0 items-center gap-2 border-b border-border/50 px-3"><Loader2 className="size-4 text-error" data-slot="spinner" /><span className="text-sm font-medium">{planning.cancelling ? "正在取消规划…" : "正在规划"}</span><Button variant="ghost" size="icon-sm" className="ml-auto" onClick={onClose} aria-label="收起规划面板"><X /></Button></header>
      <div className="min-h-0 flex-1 space-y-3 overflow-auto p-4"><p className="text-sm break-words">{planning.prompt}</p><p className="text-xs text-muted-foreground">计划、执行进度和结果会在这里连续显示。</p></div>
      <footer className="flex shrink-0 justify-end border-t border-border/50 p-3"><Button variant="outline" size="sm" data-testid="planning-cancel" disabled={planning.cancelling} onClick={onCancelPlanning}>取消规划</Button></footer>
    </div> : message ? <div className="flex h-full min-h-0 flex-col"><header className="flex h-11 shrink-0 items-center justify-between border-b border-border/50 px-3"><span className="text-sm font-medium">任务回复</span><Button variant="ghost" size="icon-sm" onClick={onClose} aria-label="关闭任务回复"><X /></Button></header><div className="min-h-0 flex-1 overflow-auto whitespace-pre-wrap break-words p-4 text-sm" data-testid="task-panel-message">{message}</div></div> : !selected ? <div className="flex h-full min-h-0 flex-col">
        <header className="flex h-11 shrink-0 items-center justify-between border-b border-border/50 px-3"><span className="flex items-center gap-2 text-sm font-medium"><ListChecks className="size-4" />任务</span><Button variant="ghost" size="icon-sm" onClick={onClose} aria-label="关闭任务浮层"><X /></Button></header>
        <div className="flex-1 overflow-auto p-2">
          {!runs.length && <p className="px-3 py-8 text-center text-xs text-muted-foreground">这个会话还没有任务</p>}
          {runs.map((run) => <Button key={run.id} variant="ghost" onClick={() => choose(run.id)} className="h-auto w-full justify-between gap-3 whitespace-normal rounded-lg px-3 py-3 text-left" data-testid="task-panel-row"><span className="min-w-0"><span className="block truncate text-[13px]">{run.prompt || "任务"}</span><span className="mt-1 block text-[11px] font-normal text-muted-foreground">{labels[run.state] ?? run.state}</span></span><ChevronRight className="size-4 shrink-0" /></Button>)}
        </div>
        <footer className="border-t border-border/50 p-2"><Button variant="ghost" size="sm" onClick={() => void host.adapter.openWindow("console", `runs?session=${encodeURIComponent(sessionId)}`)}><ExternalLink />在控制台查看</Button></footer>
      </div> : <div className="flex h-full min-h-0 flex-col">
        <header className="flex h-11 shrink-0 items-center justify-between border-b border-border/50 px-2"><Button variant="ghost" size="sm" onClick={() => setSelected(null)} aria-label="返回任务列表"><ArrowLeft />任务</Button>{detail && <Badge tone={detail.state === "failed" ? "error" : detail.state === "awaitingApproval" ? "warning" : "neutral"}>{labels[detail.state] ?? detail.state}</Badge>}<Button variant="ghost" size="icon-sm" onClick={onClose} aria-label="关闭任务详情"><X /></Button></header>
        <div className="min-h-0 flex-1 space-y-3 overflow-auto p-4">
          {error && <InlineStatus tone="error">{error}</InlineStatus>}
          {!detail ? <InlineStatus tone="loading">读取任务…</InlineStatus> : <>
            <h3 className="text-sm font-semibold">{detail.prompt}</h3><p className="break-all text-[11px] text-muted-foreground">{detail.directoryDisplay}</p>
            <div aria-label="执行计划" className="space-y-2">{plan?.steps.map((step, index) => <div key={index} className="space-y-2 rounded-lg bg-muted/70 p-3 text-xs">
              {step.expectedOutputs.map((output) => <p key={output} className="break-all">{output}</p>)}
              {step.kind === "native" ? <NativeParameters args={step.args} fields={[]} /> : <pre className="whitespace-pre-wrap break-all">{step.script ?? [step.executableRef ?? step.operation, ...step.args].join(" ")}</pre>}
            </div>)}</div>
            {plan?.effects.map((effect, index) => <p key={index} className="text-xs text-muted-foreground">{effect.explanation}</p>)}
            {plan?.previewCompleteness === "unknown" && <InlineStatus tone="warning">影响范围无法完整预测，请核对命令与目标。</InlineStatus>}
            {detail.output && <pre data-testid="task-panel-output" className="rounded-lg bg-muted/70 p-3 text-xs whitespace-pre-wrap break-all">{detail.output}</pre>}
          </>}
        </div>
        {detail && <footer className="flex shrink-0 justify-end gap-2 border-t border-border/50 p-3">
          {!terminal.has(detail.state) && <Button variant="outline" size="sm" disabled={busy} onClick={() => void act(() => host.adapter.runCancel(detail.id))}>取消任务</Button>}
          {detail.state === "awaitingApproval" && <Button size="sm" disabled={busy || !plan || !plan.steps.length || plan.revision !== detail.planRevision} onClick={() => void act(() => host.adapter.runApprove(newRequestId("panel-approve"), detail.id, detail.planRevision))}>确认执行</Button>}
          {terminal.has(detail.state) && detail.origin !== "capability" && <Button variant="outline" size="sm" disabled={busy} onClick={() => void act(() => host.adapter.runRetry(newRequestId("panel-retry"), detail.id))}>再运行一次</Button>}
        </footer>}
      </div>}
  </section>;
}
