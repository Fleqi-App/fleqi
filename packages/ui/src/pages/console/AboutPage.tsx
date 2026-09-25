import { useEffect, useState } from "react";
import type { DiagnosticsSnapshot } from "@fleqi/contracts";
import { Card, DescriptionList } from "../../components/Card";
import { InlineStatus } from "../../components/InlineStatus";
import { useHost } from "../../store/host";
import { isAppError } from "../../adapters/host";
import { AppUpdater } from "../../components/AppUpdater";

const HOST_STATE_TEXT = { starting: "启动中", ready: "就绪", degraded: "降级运行", stopping: "正在停止" } as const;

export function AboutPage() {
  const host = useHost();
  const build = host.bootstrap?.buildInfo;
  const [diagnostics, setDiagnostics] = useState<DiagnosticsSnapshot | null>(null);
  const [diagnosticsError, setDiagnosticsError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    host.adapter.diagnostics().then(
      (snapshot) => {
        if (!cancelled) setDiagnostics(snapshot);
      },
      (error: unknown) => {
        if (!cancelled) setDiagnosticsError(isAppError(error) ? error.message : String(error));
      },
    );
    return () => {
      cancelled = true;
    };
  }, [host.adapter, host.attempt]);

  return (
    <div className="space-y-4">
      <h2 className="text-xl font-semibold leading-7">关于与更新</h2>
      <Card title="Fleqi" description="开源通用桌面命令助手 · AGPL-3.0-only">
        {build && (
          <DescriptionList
            items={[
              { label: "版本", value: build.version, mono: true, field: "version" },
              { label: "系统", value: `macOS ${build.minimumMacosVersion}+ · ${build.targetArch}` },
              { label: "许可证", value: "AGPL-3.0-only" },
              { label: "项目", value: "github.com/Fleqi-App/fleqi", mono: true },
            ]}
          />
        )}
      </Card>
      <AppUpdater />
      <details className="rounded-xl border border-border">
        <summary className="cursor-pointer px-4 py-3 text-sm text-muted-foreground">诊断信息</summary>
      <Card title="诊断">
        {diagnosticsError && <InlineStatus tone="error">{diagnosticsError}</InlineStatus>}
        {!diagnostics && !diagnosticsError && <InlineStatus tone="loading">正在读取诊断…</InlineStatus>}
        {diagnostics && (
          <DescriptionList
            items={[
              { label: "宿主状态", value: HOST_STATE_TEXT[diagnostics.hostState], field: "diag-host-state" },
              { label: "运行代际", value: diagnostics.generation, mono: true },
              { label: "存储", value: diagnostics.storage.state === "ready" ? `正常 · schema ${diagnostics.storage.schemaVersion}` : `降级：${diagnostics.storage.message ?? ""}`, field: "diag-storage" },
              { label: "数据目录", value: diagnostics.storage.dataDirDisplay, mono: true },
              { label: "最近备份", value: diagnostics.storage.lastBackupDisplay ?? "无", mono: true },
              { label: "日志目录", value: diagnostics.logDirDisplay, mono: true },
              { label: "已应用迁移", value: diagnostics.appliedMigrations.join("、") || "无", mono: true },
              {
                label: "凭据服务",
                value: diagnostics.credentialStore.available ? `可用 · ${diagnostics.credentialStore.namespace}` : `不可用：${diagnostics.credentialStore.message ?? ""}`,
                field: "diag-credentials",
              },
            ]}
          />
        )}
      </Card>
      </details>
    </div>
  );
}
