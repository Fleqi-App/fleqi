import { useState } from "react";
import { FolderOpen, RefreshCw, Settings2, ShieldCheck } from "lucide-react";
import type { ContextSnapshot, Permission, PermissionRecord, PermissionStatus, RecoveryAction } from "@fleqi/contracts";
import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { Card, DescriptionList } from "../../components/Card";
import { EmptyState, InlineStatus, type StatusTone } from "../../components/InlineStatus";
import { useHost } from "../../store/host";
import { isAppError } from "../../adapters/host";
import { fileManagerLabel } from "../../platform-copy";

const PERMISSION_LABEL: Record<Permission, { title: string; purpose: string }> = {
  finderAutomation: { title: "Finder 自动化", purpose: "读取当前 Finder 文件夹与选中项（Apple Events → Finder）" },
  accessibility: { title: "辅助功能", purpose: "读取 Finder 窗口几何与活动状态，用于定位输入条" },
};

const STATUS_TEXT: Record<PermissionStatus, { text: string; tone: StatusTone }> = {
  unknown: { text: "尚未检查", tone: "neutral" },
  allowed: { text: "已授权", tone: "success" },
  needsConsent: { text: "未授权（系统尚未询问）", tone: "warning" },
  denied: { text: "已拒绝", tone: "error" },
  targetNotRunning: { text: "Finder 未运行", tone: "warning" },
  failed: { text: "检查失败", tone: "error" },
};

const RECOVERY_TEXT: Record<RecoveryAction, string | null> = {
  none: null,
  requestExplicitly: "显式申请后系统会弹出授权对话框；只在你点击时申请。",
  openSystemSettings: "在 系统设置 → 隐私与安全性 中授予后，回到这里重新检查。",
  launchTarget: "启动 Finder 后重新检查。",
  recheck: "重新检查以获取当前系统事实。",
};

function PermissionRow({ record, busy, onRequest, onOpenSettings, onRecheck }: {
  record: PermissionRecord;
  busy: boolean;
  onRequest: () => void;
  onOpenSettings: () => void;
  onRecheck: () => void;
}) {
  const meta = PERMISSION_LABEL[record.permission];
  const status = STATUS_TEXT[record.status];
  const hint = RECOVERY_TEXT[record.recovery];
  return (
    <Card
      data-permission={record.permission}
      data-status={record.status}
      title={meta.title}
      description={meta.purpose}
      actions={
        <>
          {record.recovery === "requestExplicitly" && (
            <Button variant="primary" disabled={busy} onClick={onRequest}>
              显式申请
            </Button>
          )}
          {record.recovery === "openSystemSettings" && (
            <Button variant="primary" disabled={busy} onClick={onOpenSettings}>
              打开系统设置
            </Button>
          )}
          <Button disabled={busy} onClick={onRecheck} aria-label={`重新检查${meta.title}`}>
            <RefreshCw aria-hidden="true" className="size-4" />
            重新检查
          </Button>
        </>
      }
    >
      <div className="flex flex-wrap items-center gap-3">
        <InlineStatus tone={busy ? "loading" : status.tone} data-testid={`status-${record.permission}`}>
          {busy ? "正在检查…" : status.text}
        </InlineStatus>
        {record.revoked && <Badge tone="error">授权已被撤销</Badge>}
        {record.procedure === "explicit" && record.status !== "unknown" && <Badge tone="neutral">显式申请</Badge>}
        {record.checkedAt && <span className="text-xs text-muted-foreground">检查时间 {new Date(record.checkedAt).toLocaleString("zh-CN", { hour12: false })}</span>}
      </div>
      {record.error && <p className="mt-2 text-xs text-error">{record.error}</p>}
      {hint && <p className="mt-2 text-xs text-muted-foreground">{hint}</p>}
    </Card>
  );
}

function availabilityText(context: ContextSnapshot, manager: string): { text: string; tone: StatusTone } {
  switch (context.availability.kind) {
    case "available":
      return { text: "有效目录", tone: "success" };
    case "noDirectory":
      return { text: `没有有效目录：${context.availability.reason}`, tone: "warning" };
    case "permissionRequired":
      return { text: `需要 ${manager} 权限`, tone: "warning" };
    case "finderNotRunning":
      return { text: `${manager} 未运行`, tone: "warning" };
    case "selectionOverLimit":
      return { text: `选中 ${context.availability.count} 项，超过 ${context.availability.limit} 项上限，请缩小选区`, tone: "warning" };
    case "failed":
      return { text: `读取失败：${context.availability.message}`, tone: "error" };
  }
}

const VIEW_KIND_TEXT = { physical: "真实文件夹", virtual: "虚拟视图（搜索/智能目录）", desktop: "桌面" } as const;

export function ContextSection() {
  const host = useHost();
  const manager = fileManagerLabel(host.bootstrap?.buildInfo.targetOs);
  const context = host.bootstrap?.context ?? null;
  const [busy, setBusy] = useState<"refresh" | "pick" | null>(null);
  const [notice, setNotice] = useState<{ tone: StatusTone; text: string } | null>(null);

  const run = async (kind: "refresh" | "pick") => {
    setBusy(kind);
    setNotice(null);
    try {
      if (kind === "refresh") {
        await host.refreshContext();
      } else {
        const result = await host.pickDirectory();
        if (result.kind === "cancelled") setNotice({ tone: "neutral", text: "已取消选择；当前上下文保持不变。" });
        else if (result.kind === "failed") setNotice({ tone: "error", text: `选择失败：${result.message}` });
        else setNotice({ tone: "success", text: "已使用所选文件夹作为工作上下文。" });
      }
    } catch (error) {
      setNotice({ tone: "error", text: isAppError(error) ? `${error.code}: ${error.message}` : String(error) });
    } finally {
      setBusy(null);
    }
  };

  return (
    <Card
      title={`${manager} 上下文`}
      description="当前用于任务的目录与选区快照；虚拟视图不猜测目录，超限不截取。"
      actions={
        <>
          <Button disabled={busy !== null} onClick={() => run("refresh")} aria-label={`刷新 ${manager} 上下文`}>
            <RefreshCw aria-hidden="true" className="size-4" />
            刷新
          </Button>
          <Button variant="primary" disabled={busy !== null} onClick={() => run("pick")}>
            <FolderOpen aria-hidden="true" className="size-4" />
            选择文件夹…
          </Button>
        </>
      }
    >
      {busy && (
        <InlineStatus tone="loading" className="mb-3">
          {busy === "refresh" ? `正在读取 ${manager}…` : "等待选择文件夹…"}
        </InlineStatus>
      )}
      {notice && (
        <InlineStatus tone={notice.tone} className="mb-3" data-testid="context-notice">
          {notice.text}
        </InlineStatus>
      )}
      {!context ? (
        <EmptyState title={`尚未读取 ${manager} 上下文`} description="刷新会无提示核对权限后读取当前窗口；没有权限时可选择文件夹。" />
      ) : (
        <div data-testid="context-snapshot" data-context-id={context.id} data-availability={context.availability.kind}>
          <InlineStatus tone={availabilityText(context, manager).tone} className="mb-3">
            {availabilityText(context, manager).text}
          </InlineStatus>
          <DescriptionList
            items={[
              { label: "来源", value: context.source === "finder" ? manager : "文件夹选择", field: "context-source" },
              { label: "视图", value: VIEW_KIND_TEXT[context.viewKind] },
              { label: "目录", value: context.directoryRef?.displayPath ?? "—", mono: true, field: "context-directory" },
              {
                label: "选中项",
                value: context.selectionComplete ? `${context.selectedItems.length} 项` : "不完整（超限或读取失败）",
                field: "context-selection-count",
              },
              { label: "窗口", value: context.sourceWindowId ?? "—", mono: true },
              { label: "快照", value: `${context.id} · 版本 ${context.revision} · ${new Date(context.capturedAt).toLocaleTimeString("zh-CN", { hour12: false })}`, mono: true },
            ]}
          />
          {context.selectedItems.length > 0 && (
            <ul className="mt-3 max-h-40 space-y-1 overflow-auto rounded-lg border border-border p-2 font-mono text-xs" aria-label="选中项">
              {context.selectedItems.slice(0, 50).map((item) => (
                <li key={item.id} className="truncate" title={item.displayPath}>
                  {item.kind === "directory" ? "📁" : "📄"} {item.displayPath}
                </li>
              ))}
              {context.selectedItems.length > 50 && <li className="text-muted-foreground">… 其余 {context.selectedItems.length - 50} 项</li>}
            </ul>
          )}
        </div>
      )}
    </Card>
  );
}

export function PermissionsPage() {
  const host = useHost();
  const [busy, setBusy] = useState<Permission | "all" | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const records = host.bootstrap?.permissions.records ?? [];

  const guard = async (key: Permission | "all", work: () => Promise<unknown>) => {
    setBusy(key);
    setNotice(null);
    try {
      await work();
    } catch (error) {
      setNotice(isAppError(error) ? `${error.code}: ${error.message}` : String(error));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-xl font-semibold leading-7">权限与自检</h2>
          <p className="text-xs text-muted-foreground">状态来自系统重新检测，不因点击申请变绿；普通检查不会弹出授权对话框。</p>
        </div>
        <Button disabled={busy !== null} onClick={() => guard("all", () => host.checkPermissions())}>
          <ShieldCheck aria-hidden="true" className="size-4" />
          重新检查全部
        </Button>
      </div>
      {notice && (
        <InlineStatus tone="error" data-testid="permissions-notice">
          {notice}
        </InlineStatus>
      )}
      {records.map((record) => (
        <PermissionRow
          key={record.permission}
          record={record}
          busy={busy === "all" || busy === record.permission}
          onRequest={() => guard(record.permission, () => host.requestPermission(record.permission))}
          onOpenSettings={() => guard(record.permission, () => host.openSystemSettings(record.permission))}
          onRecheck={() => guard(record.permission, () => host.checkPermissions())}
        />
      ))}
      <ContextSection />
      <p className="text-xs text-muted-foreground">
        <Settings2 aria-hidden="true" className="mr-1 inline size-3.5" />
        无需 Finder 权限的本地管理与历史查看保持可用；权限被撤销时相关能力会标注影响并提供手动选择目录。
      </p>
    </div>
  );
}
