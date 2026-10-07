import { Input } from "../../components/ui/input";
import { useEffect, useState } from "react";
import { Activity, FolderCog, Info, Palette, Server, ShieldCheck, SlidersHorizontal } from "lucide-react";

import { Button } from "../../components/Button";
import { InlineStatus } from "../../components/InlineStatus";
import { SegmentedControl } from "../../components/SegmentedControl";
import { Switch } from "../../components/Switch";
import { isAppError, newRequestId } from "../../adapters/host";
import { useHost } from "../../store/host";
import { fileManagerLabel } from "../../platform-copy";
import { Card } from "../../components/Card";
import { AppShell } from "../../components/AppShell";
import { AboutPage } from "../console/AboutPage";
import { PermissionsPage } from "../console/PermissionsPage";
import { AppearancePage } from "./AppearancePage";
import { FilesPage } from "./FilesPage";
import { ModelsPage } from "./ModelsPage";
import { TasksPage } from "./TasksPage";

/** M2 通用页：barEnabled/activation/hideBehavior/气泡与建议真实保存（复用外观页的保存流）。 */
function GeneralPage() {
  const host = useHost();
  const manager = fileManagerLabel(host.bootstrap?.buildInfo.targetOs);
  const settings = host.bootstrap?.settings;
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<{ field?: string; message: string } | null>(null);
  const [saved, setSaved] = useState<string | null>(null);

  const save = async (field: string, patch: Record<string, unknown>) => {
    if (!settings) return;
    setBusy(field);
    setError(null);
    setSaved(null);
    try {
      await host.updateSettings(patch as never, settings.revision, newRequestId(`general-${field}`));
      setSaved(field);
      setTimeout(() => setSaved(null), 1800);
    } catch (reason) {
      setError(isAppError(reason) ? { field, message: `${reason.code}: ${reason.message}` } : { field, message: String(reason) });
    } finally {
      setBusy(null);
    }
  };

  if (!settings) return null;
  return (
    <div className="space-y-4">
      <h2 className="text-xl font-semibold leading-7">通用</h2>
      <Card description="按你的习惯设置输入条、快捷键与会话。">
        <div className="border-b border-border py-3">
          <div className="flex items-start justify-between gap-4">
            <div>
              <label htmlFor="barEnabled" className="text-sm font-medium">启用操作栏</label>
              <p id="barEnabled-description" className="mt-0.5 text-xs text-muted-foreground">关闭后两种唤起模式都不显示；按当前隐藏策略保留或结束会话。</p>
            </div>
            <Switch id="barEnabled" checked={settings.barEnabled} disabled={busy !== null} onCheckedChange={(checked) => void save("barEnabled", { barEnabled: checked })} />
          </div>
          {saved === "barEnabled" && <InlineStatus tone="success" className="mt-2">已保存</InlineStatus>}
        </div>
        <div className="border-b border-border py-3">
          <div className="flex items-start justify-between gap-4">
            <div>
              <label htmlFor="activation">唤起方式</label>
              <p id="activation-description" className="mt-0.5 text-xs text-muted-foreground">仅手动唤起 / 随 {manager} 自动显示（独立于快捷键）。</p>
            </div>
            <SegmentedControl
              id="activation"
              aria-label="唤起方式"
              value={settings.activation}
              disabled={busy !== null}
              options={[
                { value: "manual" as const, label: "仅手动" },
                { value: "followFinder" as const, label: `随 ${manager}` },
              ]}
              onChange={(value) => void save("activation", { activation: value })}
            />
          </div>
          {saved === "activation" && <InlineStatus tone="success" className="mt-2">已保存</InlineStatus>}
        </div>
        <div className="border-b border-border py-3">
          <div className="flex items-start justify-between gap-4">
            <div>
              <label htmlFor="hideBehavior">主动隐藏时</label>
              <p id="hideBehavior-description" className="mt-0.5 text-xs text-muted-foreground">保留全部会话 / 结束全部会话（{manager} 移动的临时隐藏不适用）。</p>
            </div>
            <SegmentedControl
              id="hideBehavior"
              aria-label="主动隐藏时"
              value={settings.hideBehavior}
              disabled={busy !== null}
              options={[
                { value: "keepAll" as const, label: "保留全部" },
                { value: "endAll" as const, label: "结束全部" },
              ]}
              onChange={(value) => void save("hideBehavior", { hideBehavior: value })}
            />
          </div>
          {saved === "hideBehavior" && <InlineStatus tone="success" className="mt-2">已保存</InlineStatus>}
        </div>
        <div className="py-3">
          <div className="flex items-start justify-between gap-4">
            <div>
              <label htmlFor="inlineSuggestionsEnabled">条内建议</label>
              <p id="inlineSuggestionsEnabled-description" className="mt-0.5 text-xs text-muted-foreground">只隐藏条内建议，不移除能力浏览入口。</p>
            </div>
            <Switch
              id="inlineSuggestionsEnabled"
              checked={settings.inlineSuggestionsEnabled}
              disabled={busy !== null}
              onCheckedChange={(checked) => void save("inlineSuggestionsEnabled", { inlineSuggestionsEnabled: checked })}
            />
          </div>
          {saved === "inlineSuggestionsEnabled" && <InlineStatus tone="success" className="mt-2">已保存</InlineStatus>}
        </div>
      </Card>
      {error && (
        <InlineStatus tone="error">
          {error.field}：{error.message}
        </InlineStatus>
      )}
      <HotkeyAndLoginCard
        settings={settings}
        busy={busy !== null}
        onError={(message) => setError({ field: "hotkey", message })}
      />
      <Card description="气泡常驻或 1–30 秒（0.1 步进）；条内建议 1–5 条，实际显示还受可用宽度限制。">
        <div className="border-b border-border py-3">
          <div className="flex items-start justify-between gap-4">
            <div>
              <label htmlFor="bubbleSeconds">结果气泡显示时长</label>
              <p id="bubbleSeconds-description" className="mt-0.5 text-xs text-muted-foreground">
                交互期间暂停倒计时；设为常驻则不自动消失。
              </p>
            </div>
            <div className="flex items-center gap-2">
              <label className="text-xs text-muted-foreground">
                <Input
                  type="checkbox"
                  className="mr-1"
                  checked={settings.bubbleSeconds == null}
                  disabled={busy !== null}
                  onChange={(event) => void save("bubbleSeconds", { bubbleSeconds: event.target.checked ? null : 4.8 })}
                />
                常驻
              </label>
              <Input
                id="bubbleSeconds"
                type="number"
                min={1}
                max={30}
                step={0.1}
                disabled={busy !== null || settings.bubbleSeconds == null}
                className="w-24 rounded-md border border-border bg-input px-2 py-1 font-mono text-sm"
                value={settings.bubbleSeconds ?? 4.8}
                onChange={(event) => {
                  const value = Number(event.target.value);
                  if (Number.isFinite(value)) void save("bubbleSeconds", { bubbleSeconds: Math.min(30, Math.max(1, value)) });
                }}
              />
              <span className="text-xs text-muted-foreground">秒</span>
            </div>
          </div>
          {saved === "bubbleSeconds" && <InlineStatus tone="success" className="mt-2">已保存</InlineStatus>}
        </div>
        <div className="py-3">
          <div className="flex items-start justify-between gap-4">
            <div>
              <label htmlFor="inlineSuggestionsLimit">条内建议数量</label>
              <p id="inlineSuggestionsLimit-description" className="mt-0.5 text-xs text-muted-foreground">1–5 条。</p>
            </div>
            <Input
              id="inlineSuggestionsLimit"
              type="number"
              min={1}
              max={5}
              step={1}
              disabled={busy !== null}
              className="w-24 rounded-md border border-border bg-input px-2 py-1 font-mono text-sm"
              value={settings.inlineSuggestionsLimit}
              onChange={(event) => {
                const value = Number(event.target.value);
                if (Number.isFinite(value)) void save("inlineSuggestionsLimit", { inlineSuggestionsLimit: Math.min(5, Math.max(1, Math.round(value))) });
              }}
            />
          </div>
          {saved === "inlineSuggestionsLimit" && <InlineStatus tone="success" className="mt-2">已保存</InlineStatus>}
        </div>
      </Card>
    </div>
  );
}

/** §10.3 快捷键录入：录入中 → 候选 → 真实注册 → 成功/失败（保留候选与旧绑定）；Esc 取消（"录入 Esc"为显式入口）。 */
function HotkeyAndLoginCard({
  settings,
  busy,
  onError,
}: {
  settings: NonNullable<ReturnType<typeof useHost>["bootstrap"]>["settings"];
  busy: boolean;
  onError: (message: string) => void;
}) {
  const host = useHost();
  const [recording, setRecording] = useState<"normal" | "esc" | null>(null);
  const [candidate, setCandidate] = useState<string | null>(null);
  const [committing, setCommitting] = useState(false);
  const [result, setResult] = useState<{ tone: "success" | "error"; text: string } | null>(null);

  useEffect(() => {
    if (!recording) return;
    const onKeyDown = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();
      if (event.key === "Escape" && recording === "normal") {
        setRecording(null);
        return;
      }
      const key = normalizeKey(event, recording === "esc");
      if (!key) return; // 纯修饰键继续等待
      const mods = [
        event.metaKey && "CommandOrControl",
        event.ctrlKey && "CommandOrCtrl",
        event.altKey && "Alt",
        event.shiftKey && "Shift",
      ].filter(Boolean) as string[];
      setCandidate([...new Set(mods), key].join("+"));
      setRecording(null);
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [recording]);

  const commit = async (accelerator: string) => {
    setCommitting(true);
    setResult(null);
    try {
      const outcome = await host.adapter.hotkeyCommit(newRequestId("settings-hotkey"), accelerator);
      if (outcome.registered && !outcome.message) {
        setResult({ tone: "success", text: `已注册并保存：${outcome.registered}` });
        setCandidate(null);
      } else {
        setResult({ tone: "error", text: outcome.message ?? "注册失败（候选保留，旧绑定仍有效）" });
      }
    } catch (reason) {
      setResult({ tone: "error", text: isAppError(reason) ? `${reason.code}: ${reason.message}` : String(reason) });
    } finally {
      setCommitting(false);
    }
  };

  const clear = async () => {
    setCommitting(true);
    setResult(null);
    try {
      await host.adapter.hotkeyClear(newRequestId("settings-hotkey-clear"));
      setCandidate(null);
      setResult({ tone: "success", text: "已清除绑定（未绑定是有效状态；与唤起方式互不影响）" });
    } catch (reason) {
      setResult({ tone: "error", text: isAppError(reason) ? `${reason.code}: ${reason.message}` : String(reason) });
    } finally {
      setCommitting(false);
    }
  };

  const current = settings.hotkey
    ? `${settings.hotkey.modifiers.map((m) => (m === "command" ? "Command" : m === "control" ? "Control" : m === "option" ? "Option" : "Shift")).join("+")}+${settings.hotkey.key}`
    : null;

  return (
    <Card
      title="快捷键与登录启动"
      description="单键或组合键均可；真实注册成功才保存，失败保留候选与原有效绑定。未绑定是有效状态，与唤起方式相互独立。"
    >
      <div className="border-b border-border py-3">
        <div className="flex items-start justify-between gap-4">
          <div>
            <label htmlFor="hotkey-display">全局快捷键</label>
            <p id="hotkey-display-description" className="mt-0.5 text-xs text-muted-foreground">
              当前：{current ?? "未绑定"}
            </p>
          </div>
          <div className="flex flex-wrap items-center justify-end gap-2">
            {recording ? (
              <InlineStatus tone="neutral" data-testid="hotkey-recording">
                {recording === "esc" ? "再次按下 Esc 作为快捷键；点击此处取消" : "按下要使用的按键；Esc 取消"}
              </InlineStatus>
            ) : (
              <Button variant="secondary" disabled={busy || committing} data-testid="hotkey-record" onClick={() => { setCandidate(null); setResult(null); setRecording("normal"); }}>
                录入快捷键
              </Button>
            )}
            <Button
              variant="secondary"
              disabled={busy || committing}
              aria-label="录入 Esc 单键"
              onClick={() => { setCandidate(null); setResult(null); setRecording("esc"); }}
            >
              录入 Esc
            </Button>
            {settings.hotkey && (
              <Button variant="secondary" disabled={busy || committing} data-testid="hotkey-clear" onClick={() => void clear()}>
                清除绑定
              </Button>
            )}
          </div>
        </div>
        {candidate && (
          <div className="mt-2 flex items-center gap-2" data-testid="hotkey-candidate">
            <span className="rounded-md border border-border px-2 py-0.5 font-mono text-sm">{candidate}</span>
            <Button variant="primary" disabled={committing} onClick={() => void commit(candidate)}>
              {committing ? "注册中…" : "注册并保存"}
            </Button>
            <Button variant="secondary" disabled={committing} onClick={() => setCandidate(null)}>
              放弃候选
            </Button>
          </div>
        )}
        {result && <InlineStatus tone={result.tone} className="mt-2">{result.text}</InlineStatus>}
      </div>
      <div className="py-3">
        <div className="flex items-start justify-between gap-4">
          <div>
            <label htmlFor="launchAtLogin">登录后驻留</label>
            <p id="launchAtLogin-description" className="mt-0.5 text-xs text-muted-foreground">
              原生启用失败会显示原因，不会伪称已开启。
            </p>
          </div>
          <Switch
            id="launchAtLogin"
            checked={settings.launchAtLogin}
            disabled={busy}
            onCheckedChange={() => {
              void (async () => {
                try {
                  await host.updateSettings({ launchAtLogin: !settings.launchAtLogin } as never, settings.revision, newRequestId("general-launch"));
                } catch (reason) {
                  onError(isAppError(reason) ? `${reason.code}: ${reason.message}` : String(reason));
                }
              })();
            }}
          />
        </div>
      </div>
    </Card>
  );
}

/** 物理键归一为 Tauri accelerator 键名；纯修饰键返回 null（继续录入）。 */
function normalizeKey(event: KeyboardEvent, allowEscape: boolean): string | null {
  const key = event.key;
  if (key === "Escape") return allowEscape ? "Escape" : null;
  if (["Meta", "Control", "Alt", "Shift"].includes(key)) return null;
  if (key === " ") return "Space";
  if (key === "Enter") return "Return";
  if (key === "Tab") return "Tab";
  if (key === "Backspace") return "Backspace";
  if (key.startsWith("Arrow")) return key.slice(5);
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(key)) return key;
  if (/^[a-z0-9]$/i.test(key)) return key.toUpperCase();
  return null;
}

export function SettingsShell({ page, onNavigate }: { page: string; onNavigate: (page: string) => void }) {
  const host = useHost();
  const settings = host.bootstrap?.settings;
  return (
    <AppShell
          pageLabel={({ general: "通用", appearance: "外观", models: "模型与 API", permissions: "权限与自检", files: "文件与工具", tasks: "任务与诊断", about: "关于与更新" } as Record<string, string>)[page] ?? page}
          title="Fleqi 设置"
          width={200}
          current={page}
          onSelect={onNavigate}
          groups={[
            {
              label: "设置",
              items: [
                { id: "general", label: "通用", icon: SlidersHorizontal },
                { id: "appearance", label: "外观", icon: Palette },
                { id: "models", label: "模型与 API", icon: Server },
                { id: "permissions", label: "权限与自检", icon: ShieldCheck },
                { id: "files", label: "文件与工具", icon: FolderCog },
                { id: "tasks", label: "任务与诊断", icon: Activity },
                { id: "about", label: "关于与更新", icon: Info },
              ],
            },
          ]}
          footer={host.adapter.kind === "preview" ? "浏览器预览 · 数据不计原生证据" : "桌面宿主"}
        >
          {page === "appearance" && <AppearancePage />}
          {page === "permissions" && <PermissionsPage />}
          {page === "about" && <AboutPage />}
          {settings && page === "general" && <GeneralPage />}
          {page === "models" && <ModelsPage />}
          {settings && page === "files" && <FilesPage />}
          {settings && page === "tasks" && <TasksPage />}
    </AppShell>
  );
}
