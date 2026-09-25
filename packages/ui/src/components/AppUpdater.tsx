import { useEffect, useState } from "react";
import type { AppUpdateStatus } from "@fleqi/contracts";
import { useHost } from "../store/host";
import { isAppError } from "../adapters/host";
import { Button } from "./Button";
import { Card } from "./Card";
import { InlineStatus } from "./InlineStatus";

const LABELS = {
  idle: "可检查新版本", checking: "正在检查更新…", current: "当前已是最新版本",
  available: "发现新版本", downloading: "正在下载并校验更新…", installing: "正在安装，即将重新启动…", failed: "更新未完成",
} as const;

export function AppUpdater() {
  const { adapter } = useHost();
  const [status, setStatus] = useState<AppUpdateStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  useEffect(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    const refresh = async () => {
      try { const next = await adapter.appUpdateStatus(); if (!disposed) setStatus(next); }
      catch (cause) { if (!disposed) setError(isAppError(cause) ? cause.message : String(cause)); }
      finally { if (!disposed) timer = setTimeout(() => void refresh(), 1000); }
    };
    void refresh();
    return () => { disposed = true; clearTimeout(timer); };
  }, [adapter]);
  const act = async (install: boolean) => {
    setPending(true); setError(null);
    try {
      if (install) await adapter.appUpdateInstall();
      else setStatus(await adapter.appUpdateCheck());
    } catch (cause) { setError(isAppError(cause) ? cause.message : String(cause)); }
    finally { setPending(false); }
  };
  const busy = pending || status?.phase === "checking" || status?.phase === "downloading" || status?.phase === "installing";
  return <Card title="应用更新" description="启动时自动检查；安装前验证更新签名。">
    <div className="space-y-3" aria-live="polite">
      <InlineStatus tone={busy ? "loading" : status?.phase === "failed" ? "error" : "neutral"}>
        {status ? LABELS[status.phase] : "正在读取更新状态…"}{status?.version ? ` · ${status.version}` : ""}
      </InlineStatus>
      {status?.phase === "downloading" && <p className="text-xs text-muted-foreground">
        已下载 {(status.downloadedBytes / 1048576).toFixed(1)} MB{status.totalBytes ? ` / ${(status.totalBytes / 1048576).toFixed(1)} MB` : ""}
      </p>}
      {status?.notes && <p className="whitespace-pre-wrap text-sm">{status.notes}</p>}
      {(error || status?.error) && <InlineStatus tone="error">{error || status?.error}</InlineStatus>}
      <div className="flex gap-2">
        <Button disabled={busy || adapter.kind === "preview"} onClick={() => void act(false)}>检查更新</Button>
        {status?.version && (status.phase === "available" || status.phase === "failed") &&
          <Button variant="primary" disabled={busy} onClick={() => void act(true)}>安装并重启</Button>}
      </div>
      <p className="text-xs text-muted-foreground">安装前请结束所有会话。更新成功后需要重新授予 macOS 权限；下载失败不会清除权限。普通重启不会重复要求授权。</p>
    </div>
  </Card>;
}
