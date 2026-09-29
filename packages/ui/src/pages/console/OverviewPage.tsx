import { useEffect, useState } from "react";
import { FolderSearch, Keyboard, Server, ShieldCheck } from "lucide-react";
import type { ProviderView } from "@fleqi/contracts";
import { Button } from "../../components/Button";

import { InlineStatus, type StatusTone } from "../../components/InlineStatus";
import { fileManagerLabel } from "../../platform-copy";
import { useHost } from "../../store/host";
import { CapabilityOverview } from "./CapabilityOverview";
import { ToolPreparation } from "../../components/ToolPreparation";

const HOST_STATE: Record<string, { text: string; tone: StatusTone }> = {
  starting: { text: "启动中", tone: "loading" },
  ready: { text: "就绪", tone: "success" },
  degraded: { text: "降级运行（可进入设置与诊断）", tone: "warning" },
  stopping: { text: "正在停止", tone: "warning" },
};

export function OverviewPage({ onNavigate }: { onNavigate: (page: string) => void }) {
  const host = useHost();
  const boot = host.bootstrap;
  const [providers, setProviders] = useState<ProviderView[] | null>(null);
  const [providerError, setProviderError] = useState(false);
  useEffect(() => {
    let cancelled = false;
    host.adapter
      .providerList()
      .then((list) => {
        if (!cancelled) setProviders(list);
        if (!cancelled) setProviderError(false);
      })
      .catch(() => {
        if (!cancelled) setProviderError(true);
      });
    return () => {
      cancelled = true;
    };
  }, [host.adapter, host.eventVersion]);
  if (!boot) return null;
  const manager = fileManagerLabel(boot.buildInfo.targetOs);
  const hostState = HOST_STATE[boot.hostState] ?? HOST_STATE.starting!;
  const finder = boot.permissions.records.find((r) => r.permission === "finderAutomation");
  const finderAllowed = finder?.status === "allowed";
  // 唤起就绪 = followFinder 已选，或 manual 且宿主注册成功的快捷键存在（settings.hotkey 只存注册成功的候选）。
  const activationReady = boot.settings.activation === "followFinder" || boot.settings.hotkey !== null;
  const hotkeyLabel = boot.settings.hotkey ? [...boot.settings.hotkey.modifiers, boot.settings.hotkey.key].join("+") : null;
  // 与 provider_service 消费语义一致：任一端点记录即可用（无记录才是"尚未配置模型端点"）。
  const providerReady = (providers?.length ?? 0) > 0;

  const readiness = [
    { label: `${manager} 权限`, icon: ShieldCheck, value: finderAllowed ? "已授权" : "待检查", ready: finderAllowed, action: () => onNavigate("permissions") },
    { label: "工作目录", icon: FolderSearch, value: boot.context?.availability.kind === "available" ? "已就绪" : "未读取", ready: boot.context?.availability.kind === "available", action: () => onNavigate("permissions") },
    { label: "唤起方式", icon: Keyboard, value: boot.settings.activation === "followFinder" ? `随 ${manager}` : hotkeyLabel ? `快捷键 ${hotkeyLabel}` : "未配置", ready: activationReady, testId: "activation-status", action: () => void host.adapter.openWindow("settings", "general") },
    { label: "模型 API", icon: Server, value: providerError ? "读取失败" : providers === null ? "读取中" : providerReady ? `已配置 ${providers.length} 个端点` : "未配置", ready: providerReady, testId: "provider-status", action: () => void host.adapter.openWindow("settings", "models") },
  ];
  return (
    <div className="overview-board" data-testid="overview-board">
      <h2 className="sr-only">概览</h2>
      <CapabilityOverview onNavigate={onNavigate} />
      <div className="overview-status-grid grid min-h-0 grid-cols-2 gap-3">
        <section className="overview-status rounded-xl border border-border bg-card p-4" aria-label="使用准备">
          <h3 className="mb-2 text-sm font-medium">使用准备</h3>
          <div className="space-y-1">
            {readiness.map(({ label, icon: Icon, value, ready, testId, action }) => (
              <button key={label} onClick={action} className="flex w-full min-w-0 items-center gap-2 rounded-md py-1 text-left text-xs hover:bg-accent-surface" aria-label={`${label}：${value}`}>
                <Icon className="size-3.5 shrink-0 text-muted-foreground" />
                <span className="shrink-0">{label}</span>
                <span title={value} data-testid={testId} className={`ml-auto truncate ${ready ? "text-success" : "text-warning"}`}>{value}</span>
              </button>
            ))}
          </div>
        </section>
        <section className="overview-status rounded-xl border border-border bg-card p-4" aria-label="平台能力">
          <div className="mb-2 flex items-center justify-between gap-2"><h3 className="text-sm font-medium">平台能力</h3><button className="text-xs text-muted-foreground hover:text-foreground" onClick={() => onNavigate("permissions")}>查看详情</button></div>
          <ul className="space-y-1" data-testid="platform-capabilities">
            {boot.platform.items.map((item) => (
              <li key={item.id} className="flex min-w-0 items-center justify-between gap-2 py-1 text-xs" data-capability={item.id} data-state={item.state} title={item.reason ?? undefined}>
                <span className="truncate">{({ finderContext: `${manager} 上下文`, accessibilityGeometry: "窗口跟随", directoryPicker: "目录选择", credentialStore: "安全凭据" } as Record<string, string>)[item.id] ?? item.id}</span>
                <span className={`shrink-0 ${item.state === "supported" ? "text-success" : "text-warning"}`}>{item.state === "supported" ? "可用" : item.state === "permissionRequired" ? "待授权" : "需检查"}</span>
              </li>
            ))}
          </ul>
        </section>
      </div>
      <footer className="overview-footer flex min-w-0 items-center gap-3 text-xs">
        <InlineStatus tone={hostState.tone} data-testid="host-state" data-host-state={boot.hostState}>{hostState.text}</InlineStatus>
        <span className={boot.storage.state === "ready" ? "text-muted-foreground" : "text-error"} data-testid="storage-state" data-storage-state={boot.storage.state}>{boot.storage.state === "ready" ? "存储正常" : "存储降级"}</span>
        <span className="sr-only" data-field="settings-revision">{boot.settings.revision}{boot.settings.persisted ? "" : "（未持久化）"}</span>
        <span className="sr-only" data-field="version">{boot.buildInfo.version}</span>
        <ToolPreparation compact onOpen={() => onNavigate("tools")} />
        <Button onClick={() => void host.adapter.openWindow("settings", "files")}>转换设置</Button>
      </footer>
    </div>
  );
}
