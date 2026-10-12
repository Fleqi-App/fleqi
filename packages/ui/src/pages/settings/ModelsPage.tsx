import { NativeSelect } from "../../components/ui/native-select";
import { useCallback, useEffect, useRef, useState } from "react";
import type { ProviderRecord, ProviderView, SettingsPatch } from "@fleqi/contracts";
import { Cloud, HardDrive, KeyRound, Pencil, Plus, Trash2 } from "lucide-react";

import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { Card, DescriptionList } from "../../components/Card";
import { EmptyState, InlineStatus } from "../../components/InlineStatus";
import { isAppError, newRequestId } from "../../adapters/host";
import { useHost } from "../../store/host";

import { Button as ProviderButton } from "../../components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "../../components/ui/dialog";
import { ProviderEditor } from "./ProviderEditor";

// UI form starters only: no provider is saved, called or given a key until the user submits.
const PROVIDER_PRESETS: Array<Pick<ProviderRecord, "id" | "displayName" | "baseUrl"> & { local?: boolean; description: string }> = [
  { id: "openai", displayName: "OpenAI", baseUrl: "https://api.openai.com/v1", description: "OpenAI API" },
  { id: "deepseek", displayName: "DeepSeek", baseUrl: "https://api.deepseek.com/v1", description: "DeepSeek API" },
  { id: "qwen", displayName: "通义千问", baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1", description: "阿里云百炼 · 北京" },
  { id: "openrouter", displayName: "OpenRouter", baseUrl: "https://openrouter.ai/api/v1", description: "多个模型，一个连接" },
  { id: "ollama", displayName: "Ollama", baseUrl: "http://127.0.0.1:11434/v1", local: true, description: "本机模型" },
  { id: "lmstudio", displayName: "LM Studio", baseUrl: "http://127.0.0.1:1234/v1", local: true, description: "本地模型服务" },
];

/** 模型与 API（ui-design.md §9.5）：端点列表 + 编辑器；密钥只写不读（保存后显示"已保存凭据"）。 */
export function ModelsPage() {
  const host = useHost();
  const [providers, setProviders] = useState<ProviderView[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [editing, setEditing] = useState<ProviderRecord | null>(null);
  const [editorOpen, setEditorOpen] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [editorBusy, setEditorBusy] = useState(false);
  const [discard, setDiscard] = useState(false);
  const [confirm, setConfirm] = useState<{ view: ProviderView; action: "delete" | "clear" } | null>(null);
  const [actionBusy, setActionBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const listAttempt = useRef(0);
  const connect = (preset?: typeof PROVIDER_PRESETS[number]) => {
    setDirty(false); setDiscard(false);
    setEditing(preset ? { id: newRequestId(preset.id), displayName: preset.displayName, baseUrl: preset.baseUrl, models: [], defaultGenerationModel: null, summaryModel: null, timeoutMs: 30000 } : null);
    setEditorOpen(true);
  };

  const reload = useCallback(async () => {
    const current = ++listAttempt.current;
    try {
      const providers = await host.adapter.providerList();
      if (current !== listAttempt.current) return;
      setProviders(providers);
      setError(null);
    } catch (reason) {
      if (current === listAttempt.current) setError(isAppError(reason) ? `${reason.code}: ${reason.message}` : String(reason));
    }
  }, [host.adapter]);

  useEffect(() => {
    void reload();
    return () => { listAttempt.current += 1; };
  }, [reload, host.eventVersion]);

  const save = async (request: import("@fleqi/contracts").ProviderSaveRequest, successText: (view: ProviderView) => string) => {
    const saved = await host.adapter.providerSave(request);
    setNotice(successText(saved));
    await reload();
  };

  const closeEditor = () => {
    if (editorBusy) return;
    if (dirty) { setDiscard(true); return; }
    setEditorOpen(false); setEditing(null);
  };

  const remove = async (providerId: string) => {
    await host.adapter.providerDelete(providerId);
    setNotice(`端点 ${providerId} 已删除（密钥一并清理）`);
    if (editing?.id === providerId) setEditing(null);
    await reload();
  };

  const clearSecret = async (view: ProviderView) => {
    await save(
        {
          id: view.record.id,
          displayName: view.record.displayName,
          baseUrl: view.record.baseUrl,
          models: view.record.models,
          defaultGenerationModel: view.record.defaultGenerationModel,
          summaryModel: view.record.summaryModel,
          timeoutMs: view.record.timeoutMs,
          apiKey: "",
        },
        () => `端点 ${view.record.displayName} 的密钥已清除`,
    );
  };
  const confirmAction = async () => {
    if (!confirm) return;
    setActionBusy(true); setActionError(null);
    try {
      if (confirm.action === "delete") await remove(confirm.view.record.id);
      else await clearSecret(confirm.view);
      setConfirm(null);
    } catch (reason) { setActionError(isAppError(reason) ? reason.message : String(reason)); }
    finally { setActionBusy(false); }
  };

  return (
    <div className="space-y-4 [&_[data-slot=native-select-wrapper]]:w-full">
      <div className="flex items-center justify-between gap-3"><h2 className="text-xl font-semibold leading-7">模型与 API</h2><Button onClick={() => connect()}><Plus className="size-4" />自定义连接</Button></div>
      <p className="text-sm text-muted-foreground">
        管理云端 API 和本地模型连接；同一供应商可以配置多个连接。密钥保存在系统凭据服务，请求直接发送到你配置的地址。
      </p>
      {error && <div className="flex flex-wrap items-center gap-2"><InlineStatus tone="error">{error}</InlineStatus><Button onClick={() => void reload()}>重试读取</Button></div>}
      {notice && <InlineStatus tone="success">{notice}</InlineStatus>}
      <section aria-label="模型供应商" className="grid gap-2 min-[760px]:grid-cols-2 min-[1000px]:grid-cols-3">
        {PROVIDER_PRESETS.map((preset) => <ProviderButton key={preset.id} variant="outline" onClick={() => connect(preset)} className="h-auto justify-start gap-3 whitespace-normal rounded-xl bg-card p-4 text-left shadow-none" aria-label={`连接 ${preset.displayName}`}>
          <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-muted">{preset.local ? <HardDrive className="size-4" /> : <Cloud className="size-4" />}</span>
          <span className="min-w-0"><span className="block text-sm font-medium">{preset.displayName}</span><span className="mt-1 block text-[11px] font-normal text-muted-foreground">{preset.description}</span></span>
        </ProviderButton>)}
      </section>
      <DefaultModelCard providers={providers} />
      <h3 className="text-sm font-semibold">已配置连接 <span className="font-normal text-muted-foreground">{providers?.length ?? 0}</span></h3>
      {providers === null ? (
        <InlineStatus tone="neutral">{error ? "端点列表未能加载" : "正在读取端点…"}</InlineStatus>
      ) : providers.length === 0 ? (
        <EmptyState
          title="尚未配置端点"
          description="添加第一个 OpenAI 兼容端点后即可在输入条使用自然语言任务；手动终端不需要模型。"
        />
      ) : (
        <div className="space-y-3">
          {providers.map((view) => (
            <ProviderCard
              key={view.record.id}
              view={view}
              editing={editing?.id === view.record.id}
              onEdit={() => { setDirty(false); setDiscard(false); setEditing(view.record); setEditorOpen(true); }}
              onRemoved={() => { setActionError(null); setConfirm({ view, action: "delete" }); }}
              onClearSecret={() => { setActionError(null); setConfirm({ view, action: "clear" }); }}
            />
          ))}
        </div>
      )}
      <Dialog open={editorOpen} onOpenChange={(open) => { if (!open) closeEditor(); }}><DialogContent showCloseButton={!editorBusy} className="max-h-[calc(100vh-2rem)] grid-rows-[auto_minmax(0,1fr)] overflow-hidden sm:max-w-2xl" onInteractOutside={(event) => event.preventDefault()} onEscapeKeyDown={(event) => { if (editorBusy || event.isComposing) event.preventDefault(); }}><DialogHeader><DialogTitle>模型连接</DialogTitle><DialogDescription>配置地址与凭据，获取或手动添加模型；保存后生效。</DialogDescription></DialogHeader>
        <div className="min-h-0 overflow-y-auto"><ProviderEditor
        key={editing?.id ?? "new"}
        initial={editing}
        existing={(providers ?? []).some((view) => view.record.id === editing?.id)}
        credentialConfigured={(providers ?? []).find((view) => view.record.id === editing?.id)?.credentialConfigured ?? false}
        onCancel={closeEditor}
        onDirtyChange={setDirty}
        onSavingChange={setEditorBusy}
        onSave={async (request) => {
          await save(request, (saved) => `端点 ${saved.record.displayName} 已保存${saved.credentialConfigured ? "；凭据已保存" : ""}`);
          setEditing(null);
          setEditorOpen(false);
        }}
        onProbe={(...args) => host.adapter.providerProbe(...args)}
      /></div></DialogContent></Dialog>
      <Dialog open={discard} onOpenChange={setDiscard}><DialogContent><DialogHeader><DialogTitle>放弃修改？</DialogTitle><DialogDescription>模型连接有未保存的修改。关闭会丢弃这份草稿。</DialogDescription></DialogHeader><div className="flex justify-end gap-2"><Button onClick={() => setDiscard(false)}>继续编辑</Button><Button onClick={() => { setEditorOpen(false); setEditing(null); setDiscard(false); }}>放弃修改</Button></div></DialogContent></Dialog>
      <Dialog open={confirm !== null} onOpenChange={(open) => { if (!open && !actionBusy) setConfirm(null); }}><DialogContent showCloseButton={!actionBusy} onInteractOutside={(event) => event.preventDefault()} onEscapeKeyDown={(event) => { if (actionBusy) event.preventDefault(); }}><DialogHeader><DialogTitle>{confirm?.action === "delete" ? "删除连接" : "清除密钥"}</DialogTitle><DialogDescription>{confirm?.action === "delete" ? `删除 ${confirm.view.record.displayName} 及其凭据；使用该连接的默认模型将不可用，需要重新选择。` : `清除 ${confirm?.view.record.displayName} 的凭据，模型配置保留；需要认证的服务将无法调用。`}</DialogDescription></DialogHeader>{actionError && <InlineStatus tone="error">{actionError}</InlineStatus>}<div className="flex justify-end gap-2"><Button disabled={actionBusy} onClick={() => setConfirm(null)}>取消</Button><Button disabled={actionBusy} onClick={() => void confirmAction()}>{actionBusy ? "处理中…" : confirm?.action === "delete" ? "确认删除" : "确认清除"}</Button></div></DialogContent></Dialog>
    </div>
  );
}

/** 顶层默认模型（settings.defaultModel）：规划时优先于端点记录的默认生成模型。 */
function DefaultModelCard({ providers }: { providers: ProviderView[] | null }) {
  const host = useHost();
  const settings = host.bootstrap?.settings;
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  if (!settings) return null;
  const options = (providers ?? []).flatMap((view) => view.record.models.map((model) => ({ value: JSON.stringify([view.record.id, model]), model, label: `${view.record.displayName} / ${model}` })));
  const legacyMatches = options.filter((option) => option.model === settings.defaultModel);
  const selectedValue = legacyMatches.length === 1 ? legacyMatches[0]!.value : settings.defaultModel ?? "";
  const summary = settings.summaryModel.kind === "model" ? settings.summaryModel.model : "";
  const summaryMatches = options.filter((option) => option.model === summary);
  const summaryValue = summaryMatches.length === 1 ? summaryMatches[0]!.value : summary;
  const missing = (value: string) => !!value && !options.some((option) => option.value === value);

  const save = async (patch: SettingsPatch) => {
    setBusy(true);
    setError(null);
    setSaved(false);
    try {
      await host.updateSettings(patch, settings.revision, newRequestId("models-default"));
      setSaved(true);
      setTimeout(() => setSaved(false), 1800);
    } catch (reason) {
      setError(isAppError(reason) ? `${reason.code}: ${reason.message}` : String(reason));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card title="模型用途" description="指定规划任务和结果摘要使用的模型；相同模型 ID 会按连接区分。">
      {providers === null ? (
        <InlineStatus tone="neutral">正在读取可用模型…</InlineStatus>
      ) : (
        <div className="space-y-3">
          <div className="grid gap-3 sm:grid-cols-2"><label className="space-y-1.5 text-sm"><span className="block font-medium">默认模型</span>
          <NativeSelect
            aria-label="默认模型"
            data-testid="default-model-select"
            className="w-full max-w-sm appearance-none rounded-md border border-border bg-input px-2 py-1.5 text-sm"
            value={selectedValue}
            disabled={busy || !settings.persisted}
            onChange={(event) => void save({ defaultModel: event.target.value || null })}
          >
            <option value="">跟随端点默认</option>
            {missing(selectedValue) && <option value={selectedValue}>当前模型已不可用，请重新选择</option>}
            {options.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </NativeSelect></label><label className="space-y-1.5 text-sm"><span className="block font-medium">结果摘要模型</span><NativeSelect aria-label="结果摘要模型" className="w-full" value={summaryValue} disabled={busy || !settings.persisted} onChange={(event) => void save({ summaryModel: event.target.value ? { kind: "model", model: event.target.value } : { kind: "default" } })}><option value="">跟随默认模型</option>{missing(summaryValue) && <option value={summaryValue}>当前摘要模型已不可用，请重新选择</option>}{options.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}</NativeSelect></label></div>
          {(missing(selectedValue) || missing(summaryValue)) && <InlineStatus tone="error">选定模型已被删除或不再可用，请重新选择。</InlineStatus>}
          {options.length === 0 && <p className="text-xs text-muted-foreground">先添加连接及模型；手动终端无需配置模型。</p>}
          {saved && <InlineStatus tone="success">已保存</InlineStatus>}
          {error && <InlineStatus tone="error">{error}</InlineStatus>}
        </div>
      )}
    </Card>
  );
}

function ProviderCard({ view, editing, onEdit, onRemoved, onClearSecret }: {
  view: ProviderView;
  editing: boolean;
  onEdit: () => void;
  onRemoved: () => void;
  onClearSecret: () => void;
}) {
  const record = view.record;
  return (
    <Card
      className="[&_[data-slot=card-header]]:flex [&_[data-slot=card-header]]:flex-wrap [&_[data-slot=card-action]]:flex-wrap [&_[data-slot=card-description]]:break-all [&_[data-slot=card-title]]:break-all"
      title={record.displayName}
      description={record.baseUrl}
      actions={
        <>
          {view.credentialConfigured ? <Badge tone="success">已保存凭据</Badge> : <Badge tone="neutral">无凭据</Badge>}
          <Button
            variant={editing ? "primary" : "secondary"}
            aria-label={`编辑端点 ${record.displayName}`}
            data-testid={`provider-edit-${record.id}`}
            onClick={onEdit}
          >
            <Pencil aria-hidden="true" />
            {editing ? "编辑中" : "编辑"}
          </Button>
          {view.credentialConfigured && (
            <Button variant="secondary" aria-label={`清除密钥 ${record.displayName}`} data-testid={`provider-clear-secret-${record.id}`} onClick={onClearSecret}>
              <KeyRound aria-hidden="true" />
            </Button>
          )}
          <Button variant="secondary" aria-label={`删除端点 ${record.displayName}`} onClick={onRemoved}>
            <Trash2 aria-hidden="true" />
          </Button>
        </>
      }
    >
      <DescriptionList
        items={[
          { label: "接口", value: "OpenAI 兼容 · Chat Completions" },
          { label: "模型", value: `${record.models.length} 个 · ${record.models.slice(0, 4).join("、")}${record.models.length > 4 ? "…" : ""}` },
          { label: "默认生成模型", value: record.defaultGenerationModel ?? "（未选择）", mono: true },
          { label: "摘要模型", value: record.summaryModel ?? "跟随默认模型", mono: true },
          { label: "超时", value: `${record.timeoutMs} ms`, mono: true },
        ]}
      />
    </Card>
  );
}
