import { useCallback, useEffect, useState } from "react";
import type { InstallProgress, ToolEntry } from "@fleqi/contracts";
import { Download, RefreshCw, Trash2 } from "lucide-react";

import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { Card, DescriptionList } from "../../components/Card";
import { InlineStatus } from "../../components/InlineStatus";
import { isAppError, newRequestId } from "../../adapters/host";
import { useHost } from "../../store/host";
import { ToolPreparation } from "../../components/ToolPreparation";

/** 进度文案（FR-TOOLS-002）：区分确定字节量与阶段状态。 */
function progressLabel(progress: InstallProgress | null): string {
  if (!progress) return "安装中…";
  switch (progress.stage) {
    case "download":
      return progress.total !== null
        ? `下载中 ${(progress.bytes / 1024).toFixed(0)} / ${(progress.total / 1024).toFixed(0)} KiB`
        : `下载中 ${(progress.bytes / 1024).toFixed(0)} KiB`;
    case "verifying":
      return "校验中…";
    case "extracting":
      return "解压中…";
    case "publishing":
      return "发布中…";
    case "installing":
      return progress.message;
  }
}

function statusBadge(entry: ToolEntry) {
  if (entry.status.kind === "available") {
    return <Badge tone="success">可用 · {entry.status.version || "（无版本输出）"}</Badge>;
  }
  if (entry.status.kind === "notInstalled") return <Badge tone="neutral">未安装</Badge>;
  return <Badge tone="warning">需处理</Badge>;
}

/** 工具页（ui-design.md §9.6）：检测状态、来源与安装位置；安装/卸载真实调用宿主。 */
export function ToolsPage() {
  const host = useHost();
  const [entries, setEntries] = useState<ToolEntry[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [installing, setInstalling] = useState<{ toolId: string; requestId: string; progress: InstallProgress | null } | null>(null);

  const reload = useCallback(async () => {
    setBusy("refresh");
    try {
      setEntries(await host.adapter.toolsList());
      setError(null);
    } catch (reason) {
      setError(isAppError(reason) ? `${reason.code}: ${reason.message}` : String(reason));
    } finally {
      setBusy(null);
    }
  }, [host.adapter]);

  useEffect(() => {
    void reload();
    return host.adapter.subscribe((event) => { if (event.kind === "toolsChanged") void reload(); });
  }, [reload, host.adapter]);

  // 在途安装进度轮询（tools_install_status；结束/取消后任务表已清空，返回 null 停更）。
  const installingRequestId = installing?.requestId;
  useEffect(() => {
    if (!installingRequestId) return;
    const timer = setInterval(() => {
      void host.adapter
        .toolsInstallStatus(installingRequestId)
        .then((progress) => {
          setInstalling((current) =>
            current?.requestId === installingRequestId ? { ...current, progress } : current,
          );
        })
        .catch(() => {});
    }, 400);
    return () => clearInterval(timer);
  }, [installingRequestId, host.adapter]);

  const install = async (entry: ToolEntry) => {
    setBusy(entry.manifest.id);
    setError(null);
    setNotice(null);
    const requestId = newRequestId("tools-install");
    setInstalling({ toolId: entry.manifest.id, requestId, progress: null });
    try {
      const updated = await host.adapter.toolsInstall(requestId, entry.manifest.id);
      setNotice(`已安装 ${entry.manifest.id}（预检通过后发布）`);
      setEntries((current) =>
        current?.map((item) => (item.manifest.id === updated.manifest.id ? { ...item, status: updated.status, installed: updated.installed } : item)) ?? current,
      );
    } catch (reason) {
      setError(isAppError(reason) ? `${reason.code}: ${reason.message}` : String(reason));
    } finally {
      setInstalling(null);
      setBusy(null);
    }
  };

  const cancelInstall = async () => {
    if (!installing) return;
    try {
      await host.adapter.toolsInstallCancel(installing.requestId);
    } catch {
      // 已结束的安装任务不存在：提交结果马上返回。
    }
  };

  const remove = async (entry: ToolEntry) => {
    setBusy(entry.manifest.id);
    setError(null);
    setNotice(null);
    try {
      await host.adapter.toolsRemove(entry.manifest.id);
      setNotice(`已卸载 ${entry.manifest.id}（仅移除应用管理的目录）`);
      setEntries((current) =>
        current?.map((item) =>
          item.manifest.id === entry.manifest.id ? { ...item, status: { kind: "notInstalled" }, installed: null } : item,
        ) ?? current,
      );
    } catch (reason) {
      setError(isAppError(reason) ? `${reason.code}: ${reason.message}` : String(reason));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between gap-3">
        <h2 className="text-xl font-semibold leading-7">工具</h2>
        <Button variant="secondary" onClick={() => void reload()} disabled={busy !== null}>
          <RefreshCw aria-hidden="true" />
          {busy === "refresh" ? "检测中…" : "重新检测"}
        </Button>
      </div>
      <p className="text-sm text-muted-foreground">
        已有工具直接使用；缺失工具由官方包管理器补齐。系统工具仍由原包管理器管理，Fleqi 只卸载自己管理的工具包。
      </p>
      <ToolPreparation />
      {error && <InlineStatus tone="error">{error}</InlineStatus>}
      {notice && <InlineStatus tone="success">{notice}</InlineStatus>}
      {entries === null ? (
        <InlineStatus tone="neutral">正在检测工具…</InlineStatus>
      ) : (
        <div className="space-y-3">
          {entries.map((entry) => (
            <Card
              key={entry.manifest.id}
              title={entry.manifest.id}
              description={
                entry.manifest.source.kind === "system"
                  ? `系统工具 · 影响：${entry.manifest.capabilities.join("、") || "—"}`
                  : `受管包 ${entry.manifest.version} · 影响：${entry.manifest.capabilities.join("、") || "—"}`
              }
              actions={
                <>
                  {statusBadge(entry)}
                  {(entry.manifest.source.kind === "managed" || entry.status.kind !== "available") &&
                    (entry.installed ? (
                      <Button variant="secondary" aria-label={`卸载 ${entry.manifest.id}`} disabled={busy !== null} onClick={() => void remove(entry)}>
                        <Trash2 aria-hidden="true" />
                      </Button>
                    ) : (
                      <>
                        <Button
                          variant="secondary"
                          disabled={busy !== null || (installing !== null && installing.toolId !== entry.manifest.id)}
                          onClick={() => void install(entry)}
                        >
                          <Download aria-hidden="true" />
                          {installing?.toolId === entry.manifest.id ? progressLabel(installing.progress) : "安装"}
                        </Button>
                        {installing?.toolId === entry.manifest.id && (
                          <Button variant="secondary" data-testid="tool-install-cancel" onClick={() => void cancelInstall()}>
                            取消安装
                          </Button>
                        )}
                      </>
                    ))}
                </>
              }
            >
              <DescriptionList
                items={[
                  { label: "来源", value: entry.manifest.source.kind === "system" ? "已有系统工具 / Homebrew 官方仓库" : entry.manifest.source.url, mono: true },
                  {
                    label: "安装位置",
                    value: entry.status.kind === "available" ? entry.status.path : entry.installed ? entry.installed.installDir : "未安装",
                    mono: true,
                  },
                  ...(entry.status.kind === "unavailable" ? [{ label: "原因", value: entry.status.reason, mono: true }] : []),
                ]}
              />
            </Card>
          ))}
        </div>
      )}
    </div>
  );
}
