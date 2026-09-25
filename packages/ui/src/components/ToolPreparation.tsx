import { useCallback, useEffect, useState } from "react";
import type { InstallProgress, ToolPreparation as Preparation } from "@fleqi/contracts";
import { useHost } from "../store/host";
import { isAppError } from "../adapters/host";
import { Button } from "./Button";
import { Card } from "./Card";
import { InlineStatus } from "./InlineStatus";

export function installProgressLabel(progress: InstallProgress | null): string {
  if (!progress) return "检测中…";
  switch (progress.stage) {
    case "download": return progress.total ? `下载中 ${Math.round(progress.bytes * 100 / progress.total)}%` : `已下载 ${(progress.bytes / 1048576).toFixed(1)} MB`;
    case "verifying": return "校验中…";
    case "extracting": return "解压中…";
    case "publishing": return "完成安装…";
    case "installing": return progress.message;
  }
}

export function ToolPreparation({ compact = false, onOpen }: { compact?: boolean; onOpen?: () => void }) {
  const host = useHost();
  const [state, setState] = useState<Preparation | null>(null);
  const [error, setError] = useState<string | null>(null);
  const reload = useCallback(() => host.adapter.toolsPrepareStatus().then(setState, (reason: unknown) => setError(isAppError(reason) ? reason.message : String(reason))), [host.adapter]);
  useEffect(() => { void reload(); return host.adapter.subscribe((event) => { if (event.kind === "toolsChanged") void reload(); }); }, [host.adapter, reload]);
  useEffect(() => {
    if (!state?.running) return;
    const timer = setInterval(() => void reload(), 700);
    return () => clearInterval(timer);
  }, [state?.running, reload]);
  const label = state?.running ? `工具准备 ${state.completed}/${state.total}` : state?.errors.length ? "工具需处理" : state?.total && state.completed === state.total ? "工具已就绪" : "工具准备";
  if (compact) return <Button className="ml-auto" onClick={onOpen} data-testid="tools-preparation-summary">{label}</Button>;
  return <Card title={label} description="首次启动自动检测并补装缺失工具；已有的 Git、FFmpeg 等直接复用。" actions={state?.running
    ? <Button onClick={() => void host.adapter.toolsPrepareCancel().then(reload)}>取消准备</Button>
    : <Button onClick={() => { setError(null); void host.adapter.toolsPrepare().then(setState, (reason: unknown) => setError(isAppError(reason) ? reason.message : String(reason))); }}>继续工具准备</Button>}>
    {state?.running && <div role="status" className="space-y-2"><p className="text-sm">{state.currentTool ?? "检测环境"}</p><p className="break-words text-xs text-muted-foreground">{installProgressLabel(state.progress)}</p><progress className="h-1.5 w-full accent-foreground" value={state.completed} max={state.total || 1} aria-label="工具准备进度" /></div>}
    {state?.cancelled && <InlineStatus tone="neutral">已取消，可随时继续；已有工具仍可使用。</InlineStatus>}
    {error && <InlineStatus tone="error">{error}</InlineStatus>}
    {!!state?.errors.length && <ul className="mt-2 space-y-1 text-xs text-warning">{state.errors.map((message, index) => <li key={index}>{message}</li>)}</ul>}
  </Card>;
}
