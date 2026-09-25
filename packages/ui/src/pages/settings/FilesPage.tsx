import { useCallback, useEffect, useState } from "react";
import type { SettingsPatch } from "@fleqi/contracts";

import { Button } from "../../components/Button";
import { Card, DescriptionList } from "../../components/Card";
import { EmptyState, InlineStatus } from "../../components/InlineStatus";
import { SegmentedControl } from "../../components/SegmentedControl";
import { isAppError, newRequestId } from "../../adapters/host";
import { useHost } from "../../store/host";

/** 文件与工具（ui-design.md §10.2）：输出位置与重名策略真实保存；输入历史查看与清理。 */
export function FilesPage() {
  const host = useHost();
  const settings = host.bootstrap?.settings;
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
  const [history, setHistory] = useState<string[] | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const save = async (field: string, patch: SettingsPatch) => {
    if (!settings) return;
    setBusy(field);
    setError(null);
    setSaved(null);
    try {
      await host.updateSettings(patch, settings.revision, newRequestId(`files-${field}`));
      setSaved(field);
      setTimeout(() => setSaved(null), 1800);
    } catch (reason) {
      setError(isAppError(reason) ? `${reason.code}: ${reason.message}` : String(reason));
    } finally {
      setBusy(null);
    }
  };

  const reloadHistory = useCallback(async () => {
    try {
      setHistory(await host.adapter.historyList());
    } catch (reason) {
      setError(isAppError(reason) ? `${reason.code}: ${reason.message}` : String(reason));
    }
  }, [host.adapter]);

  useEffect(() => {
    void reloadHistory();
  }, [reloadHistory]);

  if (!settings) return null;
  return (
    <div className="space-y-4">
      <h2 className="text-xl font-semibold leading-7">文件与工具</h2>
      <Card title="格式转换">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <label htmlFor="conversionSourceHandling" className="text-sm">转换成功后</label>
          <SegmentedControl id="conversionSourceHandling" aria-label="转换后原文件" value={settings.conversionSourceHandling} disabled={busy !== null} options={[
            { value: "keep" as const, label: "保留原文件" },
            { value: "trashAfterSuccess" as const, label: "原文件移入回收站" },
          ]} onChange={(value) => void save("conversionSourceHandling", { conversionSourceHandling: value })} />
        </div>
        <p className="mt-3 text-xs text-muted-foreground">仅在转换成功后处理原文件；每次转换可单独选择。</p>
        {saved === "conversionSourceHandling" && <InlineStatus tone="success" className="mt-2">已保存</InlineStatus>}
      </Card>
      <Card title="同名文件处理">
        <div className="border-b border-border py-3">
          <div className="flex items-start justify-between gap-4">
            <div>
              <label htmlFor="nameConflict">遇到同名文件时</label>
              <p id="nameConflict-description" className="mt-0.5 text-xs text-muted-foreground">
                保留两份会添加“(1)”后缀；替换仅在新文件生成成功后进行。
              </p>
            </div>
            <SegmentedControl
              id="nameConflict"
              aria-label="遇到同名文件时"
              value={settings.nameConflict}
              disabled={busy !== null}
              options={[
                { value: "uniqueName" as const, label: "保留两份" },
                { value: "overwrite" as const, label: "替换已有文件" },
              ]}
              onChange={(value) => void save("nameConflict", { nameConflict: value })}
            />
          </div>
          {saved === "nameConflict" && <InlineStatus tone="success" className="mt-2">已保存</InlineStatus>}
        </div>
        <div className="py-3">
          <DescriptionList
            items={[
              {
                label: "输出位置",
                value: settings.outputLocation.kind === "besideSource" ? "原文件所在文件夹" : settings.outputLocation.displayPath,
                mono: settings.outputLocation.kind !== "besideSource",
              },
            ]}
          />
        </div>
        {error && <InlineStatus tone="error" className="mt-2">{error}</InlineStatus>}
      </Card>

      <Card
        title="输入历史"
        description="保留最近 200 条。清理记录不会删除文件。"
        actions={
          <Button
            variant="secondary"
            disabled={history === null || history.length === 0}
            onClick={() => {
              void (async () => {
                try {
                  await host.adapter.historyClear();
                  setNotice("输入历史已清理（仅记录）");
                  await reloadHistory();
                } catch (reason) {
                  setError(isAppError(reason) ? `${reason.code}: ${reason.message}` : String(reason));
                }
              })();
            }}
          >
            清理记录
          </Button>
        }
      >
        {notice && <InlineStatus tone="success">{notice}</InlineStatus>}
        {history === null ? (
          <InlineStatus tone="neutral">正在读取…</InlineStatus>
        ) : history.length === 0 ? (
          <EmptyState title="暂无输入历史" />
        ) : (
          <ul className="max-h-64 space-y-1 overflow-auto font-mono text-xs">
            {history.map((entry, index) => (
              <li key={`${index}-${entry}`} className="truncate rounded px-1 py-0.5 odd:bg-muted/40">
                {entry}
              </li>
            ))}
          </ul>
        )}
      </Card>
    </div>
  );
}
