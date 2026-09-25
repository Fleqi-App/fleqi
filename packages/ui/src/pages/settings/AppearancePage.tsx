import { useEffect, useState, type ReactNode } from "react";
import type { AppError, MotionMode, SettingsPatch, Theme } from "@fleqi/contracts";
import { isAppError, newRequestId } from "../../adapters/host";
import { Button } from "../../components/Button";
import { Card } from "../../components/Card";
import { InlineStatus } from "../../components/InlineStatus";
import { SegmentedControl } from "../../components/SegmentedControl";
import { Switch } from "../../components/Switch";
import { useHost } from "../../store/host";

type SaveState =
  | { status: "idle" }
  | { status: "saving"; draft: unknown }
  | { status: "saved" }
  | { status: "failed"; draft: unknown; error: AppError }
  | { status: "conflict"; draft: unknown; error: AppError };

type M1Field = "theme" | "transparency" | "motionMode";

/**
 * 单字段保存流程（ui-design.md §10.1）：改动后显示保存中，宿主返回快照才标已保存；
 * 失败显示行内重试并恢复实际生效值，草稿保留；冲突展示最新值与未保存编辑，不悄悄覆盖。
 */
function useFieldSave(field: M1Field) {
  const host = useHost();
  const [state, setState] = useState<SaveState>({ status: "idle" });
  const effective = host.bootstrap?.settings[field];

  useEffect(() => {
    if (state.status === "saved") {
      const timer = setTimeout(() => setState({ status: "idle" }), 1800);
      return () => clearTimeout(timer);
    }
    return undefined;
  }, [state.status]);

  const save = async (value: unknown, requestId = newRequestId(`settings-${field}`)) => {
    const revision = host.bootstrap?.settings.revision;
    if (revision == null) return;
    setState({ status: "saving", draft: value });
    try {
      await host.updateSettings({ [field]: value } as SettingsPatch, revision, requestId);
      setState({ status: "saved" });
    } catch (error) {
      const appError: AppError = isAppError(error) ? error : { code: "internal", message: String(error), retryable: true };
      if (appError.code === "conflict") {
        host.reload();
        setState({ status: "conflict", draft: value, error: appError });
      } else {
        setState({ status: "failed", draft: value, error: appError });
      }
    }
  };

  const draft = state.status === "saving" || state.status === "failed" || state.status === "conflict" ? state.draft : undefined;
  return { field, state, effective, draft, save, discard: () => setState({ status: "idle" }) };
}

type FieldSave = ReturnType<typeof useFieldSave>;

function SaveStatus({ save, label, format }: { save: FieldSave; label: string; format: (value: unknown) => string }) {
  const { state, effective, field } = save;
  const retry = () => (state.status === "failed" || state.status === "conflict" ? save.save(state.draft) : undefined);
  return (
    <div className="mt-2 min-h-5 text-xs" data-testid={`save-${field}`} data-save-state={state.status}>
      {state.status === "saving" && <InlineStatus tone="loading">保存中…</InlineStatus>}
      {state.status === "saved" && <InlineStatus tone="success">已保存</InlineStatus>}
      {state.status === "failed" && (
        <div className="flex flex-wrap items-center gap-2">
          <InlineStatus tone="error">
            保存失败（{state.error.code}）：{state.error.message}；当前生效值 {format(effective)}
          </InlineStatus>
          {state.error.retryable && (
            <Button onClick={retry} aria-label={`重试保存${label}`}>
              重试
            </Button>
          )}
          <Button onClick={save.discard}>放弃草稿</Button>
        </div>
      )}
      {state.status === "conflict" && (
        <div className="flex flex-wrap items-center gap-2">
          <InlineStatus tone="warning">
            其他窗口已修改：最新值 {format(effective)}，你的草稿 {format(state.draft)}
          </InlineStatus>
          <Button onClick={retry} aria-label={`用草稿重试${label}`}>
            用草稿重试
          </Button>
          <Button onClick={save.discard}>使用最新值</Button>
        </div>
      )}
    </div>
  );
}

function FieldRow({ id, label, description, control, status }: { id: string; label: string; description: string; control: ReactNode; status: ReactNode }) {
  return (
    <div className="border-b border-border py-3 last:border-b-0">
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0">
          <label htmlFor={id} className="text-sm font-medium">
            {label}
          </label>
          <p id={`${id}-description`} className="mt-0.5 text-xs text-muted-foreground">
            {description}
          </p>
        </div>
        <div className="shrink-0">{control}</div>
      </div>
      {status}
    </div>
  );
}

const THEME_TEXT: Record<Theme, string> = { dark: "深色", light: "浅色", system: "跟随系统" };
const MOTION_TEXT: Record<MotionMode, string> = { system: "跟随系统", reduce: "减少动态" };

export function AppearancePage() {
  const host = useHost();
  const settings = host.bootstrap?.settings;
  const theme = useFieldSave("theme");
  const transparency = useFieldSave("transparency");
  const motion = useFieldSave("motionMode");
  if (!settings) return null;
  const persisted = settings.persisted;

  const themeValue = (theme.draft as Theme | undefined) ?? settings.theme;
  const transparencyValue = (transparency.draft as boolean | undefined) ?? settings.transparency;
  const motionValue = (motion.draft as MotionMode | undefined) ?? settings.motionMode;

  return (
    <div className="space-y-4">
      <h2 className="text-xl font-semibold leading-7">外观</h2>
      {!persisted && <InlineStatus tone="warning">存储不可用：修改不会保存，显示的是临时默认值。</InlineStatus>}
      <Card description={`设置版本 ${settings.revision}；每项改动都由宿主验证并返回真实结果。`}>
        <FieldRow
          id="theme"
          label="主题"
          description="深色为基线；跟随系统时按系统外观切换。主题变化不清空终端。"
          control={
            <SegmentedControl
              id="theme"
              aria-label="主题"
              aria-describedby="theme-description"
              value={themeValue}
              disabled={!persisted || theme.state.status === "saving"}
              options={(Object.keys(THEME_TEXT) as Theme[]).map((value) => ({ value, label: THEME_TEXT[value] }))}
              onChange={(value) => void theme.save(value)}
            />
          }
          status={<SaveStatus save={theme} label="主题" format={(v) => THEME_TEXT[v as Theme] ?? String(v)} />}
        />
        <FieldRow
          id="transparency"
          label="透明材质"
          description="系统减少透明度及平台材质能力优先，可退为不透明表面。"
          control={
            <Switch
              id="transparency"
              aria-label="透明材质"
              checked={transparencyValue}
              disabled={!persisted || transparency.state.status === "saving"}
              onCheckedChange={(checked) => void transparency.save(checked)}
            />
          }
          status={<SaveStatus save={transparency} label="透明材质" format={(v) => (v ? "开" : "关")} />}
        />
        <FieldRow
          id="motionMode"
          label="动态效果"
          description="跟随系统或减少动态；不提供忽略系统减少动态的强制动画选项。"
          control={
            <SegmentedControl
              id="motionMode"
              aria-label="动态效果"
              aria-describedby="motionMode-description"
              value={motionValue}
              disabled={!persisted || motion.state.status === "saving"}
              options={(Object.keys(MOTION_TEXT) as MotionMode[]).map((value) => ({ value, label: MOTION_TEXT[value] }))}
              onChange={(value) => void motion.save(value)}
            />
          }
          status={<SaveStatus save={motion} label="动态效果" format={(v) => MOTION_TEXT[v as MotionMode] ?? String(v)} />}
        />
      </Card>
    </div>
  );
}
