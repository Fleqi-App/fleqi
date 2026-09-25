import { Input } from "../../components/ui/input";
import { NativeSelect } from "../../components/ui/native-select";
import { useCallback, useEffect, useState } from "react";
import type { ProviderRecord, ProviderView } from "@fleqi/contracts";
import { Cloud, HardDrive, KeyRound, Pencil, Plus, RefreshCw, Trash2 } from "lucide-react";

import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { Card, DescriptionList } from "../../components/Card";
import { EmptyState, InlineStatus } from "../../components/InlineStatus";
import { isAppError, newRequestId } from "../../adapters/host";
import { useHost } from "../../store/host";

import { Button as ProviderButton } from "../../components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "../../components/ui/dialog";

// UI form starters only: no provider is saved, called or given a key until the user submits.
const PROVIDER_PRESETS: Array<Pick<ProviderRecord, "id" | "displayName" | "baseUrl"> & { local?: boolean; description: string }> = [
  { id: "openai", displayName: "OpenAI", baseUrl: "https://api.openai.com/v1", description: "OpenAI API" },
  { id: "deepseek", displayName: "DeepSeek", baseUrl: "https://api.deepseek.com/v1", description: "DeepSeek API" },
  { id: "qwen", displayName: "通义千问", baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1", description: "阿里云百炼 · 北京" },
  { id: "openrouter", displayName: "OpenRouter", baseUrl: "https://openrouter.ai/api/v1", description: "多个模型，一个连接" },
  { id: "ollama", displayName: "Ollama", baseUrl: "http://127.0.0.1:11434/v1", local: true, description: "本机模型" },
  { id: "lmstudio", displayName: "LM Studio", baseUrl: "http://127.0.0.1:1234/v1", local: true, description: "本地模型服务" },
];

type ProbeState = { state: "idle" } | { state: "checking" } | { state: "ok"; models: string[] } | { state: "failed"; message: string };

/** 模型与 API（ui-design.md §9.5）：端点列表 + 编辑器；密钥只写不读（保存后显示"已保存凭据"）。 */
export function ModelsPage() {
  const host = useHost();
  const [providers, setProviders] = useState<ProviderView[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [editing, setEditing] = useState<ProviderRecord | null>(null);
  const [editorOpen, setEditorOpen] = useState(false);
  const connect = (preset?: typeof PROVIDER_PRESETS[number]) => {
    setEditing(preset ? { id: newRequestId(preset.id), displayName: preset.displayName, baseUrl: preset.baseUrl, models: [], defaultGenerationModel: null, summaryModel: null, timeoutMs: 30000 } : null);
    setEditorOpen(true);
  };

  const reload = useCallback(async () => {
    try {
      setProviders(await host.adapter.providerList());
      setError(null);
    } catch (reason) {
      setError(isAppError(reason) ? `${reason.code}: ${reason.message}` : String(reason));
    }
  }, [host.adapter]);

  useEffect(() => {
    void reload();
  }, [reload, host.eventVersion]);

  const save = async (request: import("@fleqi/contracts").ProviderSaveRequest, successText: (view: ProviderView) => string) => {
    const saved = await host.adapter.providerSave(request);
    setNotice(successText(saved));
    await reload();
  };

  const remove = async (providerId: string) => {
    try {
      await host.adapter.providerDelete(providerId);
      setNotice(`端点 ${providerId} 已删除（密钥一并清理）`);
      if (editing?.id === providerId) setEditing(null);
      await reload();
    } catch (reason) {
      setError(isAppError(reason) ? `${reason.code}: ${reason.message}` : String(reason));
    }
  };

  const clearSecret = async (view: ProviderView) => {
    try {
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
    } catch (reason) {
      setError(isAppError(reason) ? `${reason.code}: ${reason.message}` : String(reason));
    }
  };

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between gap-3"><h2 className="text-xl font-semibold leading-7">模型与 API</h2><Button onClick={() => connect()}><Plus className="size-4" />自定义连接</Button></div>
      <p className="text-sm text-muted-foreground">
        选择供应商，填写你的 API Key，然后获取可用模型。也可以连接本机运行的模型；密钥保存在系统钥匙串。
      </p>
      {error && <InlineStatus tone="error">{error}</InlineStatus>}
      {notice && <InlineStatus tone="success">{notice}</InlineStatus>}
      <section aria-label="模型供应商" className="grid grid-cols-2 gap-3 min-[1000px]:grid-cols-3">
        {PROVIDER_PRESETS.map((preset) => <ProviderButton key={preset.id} variant="outline" onClick={() => connect(preset)} className="h-auto justify-start gap-3 whitespace-normal rounded-xl bg-card p-4 text-left shadow-none" aria-label={`连接 ${preset.displayName}`}>
          <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-muted">{preset.local ? <HardDrive className="size-4" /> : <Cloud className="size-4" />}</span>
          <span className="min-w-0"><span className="block text-sm font-medium">{preset.displayName}</span><span className="mt-1 block text-[11px] font-normal text-muted-foreground">{preset.description}</span></span>
        </ProviderButton>)}
      </section>
      <DefaultModelCard providers={providers} />
      {providers === null ? (
        <InlineStatus tone="neutral">正在读取端点…</InlineStatus>
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
              onEdit={() => { setEditing(view.record); setEditorOpen(true); }}
              onRemoved={() => void remove(view.record.id)}
              onClearSecret={() => void clearSecret(view)}
            />
          ))}
        </div>
      )}
      <Dialog open={editorOpen} onOpenChange={setEditorOpen}><DialogContent className="max-h-[85vh] overflow-auto sm:max-w-xl"><DialogHeader><DialogTitle>模型连接</DialogTitle><DialogDescription>确认服务地址，获取模型后保存。</DialogDescription></DialogHeader><ProviderEditor
        key={editing?.id ?? "new"}
        initial={editing}
        existing={(providers ?? []).some((view) => view.record.id === editing?.id)}
        onCancelEdit={() => { setEditing(null); setEditorOpen(false); }}
        onSave={async (request) => {
          await save(request, (saved) => `端点 ${saved.record.displayName} 已保存${saved.credentialConfigured ? "；凭据已保存" : ""}`);
          setEditing(null);
          setEditorOpen(false);
        }}
        onError={setError}
        onProbe={async (baseUrl, apiKey) => host.adapter.providerProbe(baseUrl, apiKey)}
      /></DialogContent></Dialog>
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

  const save = async (value: string) => {
    setBusy(true);
    setError(null);
    setSaved(false);
    try {
      await host.updateSettings({ defaultModel: value || null }, settings.revision, newRequestId("models-default"));
      setSaved(true);
      setTimeout(() => setSaved(false), 1800);
    } catch (reason) {
      setError(isAppError(reason) ? `${reason.code}: ${reason.message}` : String(reason));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card title="默认模型" description="规划输入条任务时优先使用该模型；未选择时用端点记录里配置的默认生成模型。">
      {providers === null ? (
        <InlineStatus tone="neutral">正在读取可用模型…</InlineStatus>
      ) : options.length === 0 ? (
        <p className="text-sm text-muted-foreground">尚无可用模型：先添加端点并完成连接检查后在此选择。</p>
      ) : (
        <div className="flex items-center gap-3">
          <NativeSelect
            aria-label="默认模型"
            data-testid="default-model-select"
            className="w-full max-w-sm appearance-none rounded-md border border-border bg-input px-2 py-1.5 text-sm"
            value={selectedValue}
            disabled={busy}
            onChange={(event) => void save(event.target.value)}
          >
            <option value="">跟随端点默认</option>
            {options.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </NativeSelect>
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
          { label: "模型", value: record.models.join("、") || "（未列出）" },
          { label: "默认生成模型", value: record.defaultGenerationModel ?? "（未选择）", mono: true },
          { label: "摘要模型", value: record.summaryModel ?? "跟随默认模型", mono: true },
          { label: "超时", value: `${record.timeoutMs} ms`, mono: true },
        ]}
      />
    </Card>
  );
}

function ProviderEditor({
  initial,
  existing,
  onCancelEdit,
  onSave,
  onError,
  onProbe,
}: {
  initial: ProviderRecord | null;
  existing: boolean;
  onCancelEdit?: () => void;
  onSave: (request: import("@fleqi/contracts").ProviderSaveRequest) => Promise<void>;
  onError: (message: string | null) => void;
  onProbe: (baseUrl: string, apiKey: string | null) => Promise<{ ok: boolean; models: string[]; error: string | null }>;
}) {
  const [id, setId] = useState(initial?.id ?? "");
  const [displayName, setDisplayName] = useState(initial?.displayName ?? "");
  const [baseUrl, setBaseUrl] = useState(initial?.baseUrl ?? "");
  const [apiKey, setApiKey] = useState("");
  const [models, setModels] = useState<string[]>(initial?.models ?? []);
  const [defaultModel, setDefaultModel] = useState(initial?.defaultGenerationModel ?? "");
  const [summaryModel, setSummaryModel] = useState(initial?.summaryModel ?? "");
  const [timeoutMs, setTimeoutMs] = useState(initial?.timeoutMs ?? 30000);
  const [saving, setSaving] = useState(false);
  const [probe, setProbe] = useState<ProbeState>({ state: "idle" });

  const probeNow = async () => {
    if (!baseUrl.trim()) {
      setProbe({ state: "failed", message: "请先填写端点地址" });
      return;
    }
    setProbe({ state: "checking" });
    try {
      const result = await onProbe(baseUrl.trim(), apiKey.trim() ? apiKey : null);
      setProbe(
        result.ok
          ? { state: "ok", models: result.models }
          : { state: "failed", message: result.error ?? "连接失败" },
      );
      if (result.ok && result.models.length > 0) {
        setModels(result.models);
        if (!result.models.includes(defaultModel)) setDefaultModel(result.models[0] ?? "");
      }
    } catch (reason) {
      setProbe({ state: "failed", message: isAppError(reason) ? reason.message : String(reason) });
    }
  };

  const reset = () => {
    setId("");
    setDisplayName("");
    setBaseUrl("");
    setApiKey("");
    setModels([]);
    setDefaultModel("");
    setSummaryModel("");
    setTimeoutMs(30000);
    setProbe({ state: "idle" });
  };

  const save = async () => {
    setSaving(true);
    onError(null);
    try {
      await onSave({
        id: id.trim() || `provider-${Date.now().toString(36)}`,
        displayName: displayName.trim() || "未命名端点",
        baseUrl: baseUrl.trim(),
        models,
        defaultGenerationModel: defaultModel || null,
        summaryModel: summaryModel || null,
        timeoutMs,
        apiKey: apiKey ? apiKey : null,
      });
      reset();
    } catch (reason) {
      onError(isAppError(reason) ? `${reason.code}: ${reason.message}` : String(reason));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Card
      className="border-0 bg-transparent py-0 shadow-none [&>[data-slot=card-header]]:px-0 [&>[data-slot=card-content]]:px-0"
      title={initial?.displayName ?? "自定义连接"}
      description={existing ? "密钥留空会保留已保存的凭据。" : "服务地址可修改；连接检查会读取可用模型。"}
    >
      {existing && (
        <p className="mb-3 text-xs text-muted-foreground">
          正在编辑此连接。修改后保存即可生效。
        </p>
      )}
      <div className="grid gap-3 sm:grid-cols-2">
        <label className="text-sm">
          <span className="mb-1 block font-medium">显示名称</span>
          <Input
            className="w-full rounded-md border border-border bg-input px-2 py-1.5 text-sm"
            value={displayName}
            onChange={(event) => setDisplayName(event.target.value)}
            placeholder="例如 本地 Ollama"
          />
        </label>
        <label className="text-sm">
          <span className="mb-1 block font-medium">端点地址（HTTPS 或本机回环）</span>
          <Input
            className="w-full rounded-md border border-border bg-input px-2 py-1.5 font-mono text-sm"
            value={baseUrl}
            onChange={(event) => setBaseUrl(event.target.value)}
            placeholder="https://api.example.com/v1"
          />
        </label>
        <label className="text-sm">
          <span className="mb-1 block font-medium">API 密钥（可选；本地服务可留空）</span>
          <Input
            type="password"
            className="w-full rounded-md border border-border bg-input px-2 py-1.5 font-mono text-sm"
            value={apiKey}
            onChange={(event) => setApiKey(event.target.value)}
            placeholder="只写不读；留空表示保持现状"
            autoComplete="off"
          />
        </label>
        <label className="text-sm">
          <span className="mb-1 block font-medium">超时（1000–300000 ms）</span>
          <Input
            type="number"
            min={1000}
            max={300000}
            step={500}
            className="w-full rounded-md border border-border bg-input px-2 py-1.5 font-mono text-sm"
            value={timeoutMs}
            onChange={(event) => setTimeoutMs(Number(event.target.value) || 30000)}
          />
        </label>
      </div>
      <div className="mt-3 flex items-center gap-2">
        <Button variant="secondary" onClick={() => void probeNow()} disabled={probe.state === "checking"}>
          <RefreshCw aria-hidden="true" />
          {probe.state === "checking" ? "检查中…" : "连接检查"}
        </Button>
        {probe.state === "ok" && <InlineStatus tone="success">连接成功；{probe.models.length} 个模型</InlineStatus>}
        {probe.state === "failed" && <InlineStatus tone="error">{probe.message}</InlineStatus>}
      </div>
      {models.length > 0 && (
        <div className="mt-3 grid gap-3 sm:grid-cols-2">
          <label className="text-sm">
            <span className="mb-1 block font-medium">默认生成模型</span>
            <NativeSelect
              className="w-full appearance-none rounded-md border border-border bg-input px-2 py-1.5 text-sm"
              value={defaultModel}
              onChange={(event) => setDefaultModel(event.target.value)}
            >
              {models.map((model) => (
                <option key={model} value={model}>
                  {model}
                </option>
              ))}
            </NativeSelect>
          </label>
          <label className="text-sm">
            <span className="mb-1 block font-medium">摘要模型（可选）</span>
            <NativeSelect
              className="w-full appearance-none rounded-md border border-border bg-input px-2 py-1.5 text-sm"
              value={summaryModel}
              onChange={(event) => setSummaryModel(event.target.value)}
            >
              <option value="">跟随默认模型</option>
              {models.map((model) => (
                <option key={model} value={model}>
                  {model}
                </option>
              ))}
            </NativeSelect>
          </label>
        </div>
      )}
      <div className="mt-4 flex items-center gap-2">
        <Button variant="primary" onClick={() => void save()} disabled={saving || !baseUrl.trim()}>
          <Plus aria-hidden="true" />
          {saving ? "保存中…" : "保存端点"}
        </Button>
        {onCancelEdit && (
          <Button onClick={onCancelEdit} aria-label="取消编辑端点">
            取消编辑
          </Button>
        )}
      </div>
    </Card>
  );
}
