import { useState } from "react";

import { Card, DescriptionList } from "../../components/Card";
import { InlineStatus } from "../../components/InlineStatus";
import { SegmentedControl } from "../../components/SegmentedControl";
import { isAppError, newRequestId } from "../../adapters/host";
import { useHost } from "../../store/host";

/** 任务与诊断（ui-design.md §10.2）：aiPolicy 真实保存；上限/保留项以说明呈现（不生成未定义控件）。 */
export function TasksPage() {
  const host = useHost();
  const settings = host.bootstrap?.settings;
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  const savePolicy = async (aiPolicy: "yolo" | "readOnlyAutoConfirmChanges") => {
    if (!settings) return;
    setBusy(true);
    setError(null);
    setSaved(false);
    try {
      await host.updateSettings({ aiPolicy } as never, settings.revision, newRequestId("tasks-ai-policy"));
      setSaved(true);
      setTimeout(() => setSaved(false), 1800);
    } catch (reason) {
      setError(isAppError(reason) ? `${reason.code}: ${reason.message}` : String(reason));
    } finally {
      setBusy(false);
    }
  };

  if (!settings) return null;
  return (
    <div className="space-y-4">
      <h2 className="text-xl font-semibold leading-7">任务与诊断</h2>
      <Card description="AI 策略只约束 AI 计划；`!` 与终端面板输入始终按普通终端直通。">
        <div className="flex items-start justify-between gap-4">
          <div>
            <label htmlFor="aiPolicy">AI 确认策略</label>
            <p id="aiPolicy-description" className="mt-0.5 text-xs text-muted-foreground">
              readOnlyAutoConfirmChanges：只读计划自动执行，修改/未知先确认；yolo：全部 AI 操作免确认（仍不绕过系统权限与外部认证）。
            </p>
          </div>
          <SegmentedControl
            id="aiPolicy"
            aria-label="AI 确认策略"
            value={settings.aiPolicy}
            disabled={busy}
            options={[
              { value: "readOnlyAutoConfirmChanges" as const, label: "只读自动" },
              { value: "yolo" as const, label: "全部免确认" },
            ]}
            onChange={(value) => void savePolicy(value)}
          />
        </div>
        {saved && <InlineStatus tone="success" className="mt-2">已保存</InlineStatus>}
        {error && <InlineStatus tone="error" className="mt-2">{error}</InlineStatus>}
      </Card>
      <Card description="以下为产品限制的当前生效值（需求 §2.1）；首版不开放编辑，以说明呈现。">
        <DescriptionList
          items={[
            { label: "terminalFontSize", value: String(settings.terminalFontSize), mono: true },
            { label: "屏幕 scrollback", value: "10000 行（固定）" },
            { label: "目录跟随", value: "当前会话跟随 Finder；忙碌时等待安全提示符" },
            { label: "输入历史上限", value: "200 条" },
            { label: "普通结束历史保留", value: "30 天（置顶会话不自动清理）" },
            { label: "AI 并发 / 活跃会话上限", value: "4 / 16（达上限排队，不取消旧任务）" },
            { label: "Run / Session 日志上限", value: "50 MiB / 100 MiB" },
          ]}
        />
      </Card>
    </div>
  );
}
