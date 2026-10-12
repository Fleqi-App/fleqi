import { useEffect, useRef, useState } from "react";
import type { ProviderRecord, ProviderSaveRequest } from "@fleqi/contracts";
import { Eye, EyeOff, Plus, RefreshCw, Trash2 } from "lucide-react";
import { isAppError, newRequestId, type HostAdapter } from "../../adapters/host";
import { Button } from "../../components/Button";
import { InlineStatus } from "../../components/InlineStatus";
import { Input } from "../../components/ui/input";
import { NativeSelect } from "../../components/ui/native-select";

type ProbeState = { state: "idle" } | { state: "checking"; kind: "models" | "model" } | { state: "ok" | "failed"; message: string };

/** 草稿独立于宿主刷新；凭据只写不读，异步检查只更新发起时的草稿。 */
export function ProviderEditor({ initial, existing, credentialConfigured, onSave, onCancel, onDirtyChange, onSavingChange, onProbe }: {
  initial: ProviderRecord | null;
  existing: boolean;
  credentialConfigured: boolean;
  onSave: (request: ProviderSaveRequest) => Promise<void>;
  onCancel: () => void;
  onDirtyChange: (dirty: boolean) => void;
  onSavingChange: (saving: boolean) => void;
  onProbe: HostAdapter["providerProbe"];
}) {
  const [id] = useState(initial?.id ?? newRequestId("provider"));
  const [displayName, setDisplayName] = useState(initial?.displayName ?? "");
  const [baseUrl, setBaseUrl] = useState(initial?.baseUrl ?? "");
  const [apiKey, setApiKey] = useState("");
  const [showKey, setShowKey] = useState(false);
  const [clearKey, setClearKey] = useState(false);
  const [models, setModels] = useState<string[]>(initial?.models ?? []);
  const [modelInput, setModelInput] = useState("");
  const [search, setSearch] = useState("");
  const [defaultModel, setDefaultModel] = useState(initial?.defaultGenerationModel ?? "");
  const [summaryModel, setSummaryModel] = useState(initial?.summaryModel ?? "");
  const [timeout, setTimeout] = useState(String(initial?.timeoutMs ?? 30000));
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [probe, setProbe] = useState<ProbeState>({ state: "idle" });
  const attempt = useRef(0);
  useEffect(() => () => { attempt.current += 1; }, []);

  const changed = () => {
    onDirtyChange(true);
    attempt.current += 1;
    setProbe({ state: "idle" });
    setError(null);
  };
  const validate = () => {
    if (!displayName.trim()) return "请填写显示名称";
    try {
      const url = new URL(baseUrl.trim());
      const loopback = url.hostname === "localhost" || url.hostname === "[::1]" || /^127\.(\d{1,3}\.){2}\d{1,3}$/.test(url.hostname);
      if (!(url.protocol === "https:" || (url.protocol === "http:" && loopback)) || url.username || url.password || url.search || url.hash) return "地址须使用 HTTPS 或本机回环，且不能包含凭据、查询参数或片段";
      if (/\/(chat\/completions|responses|messages|models)\/?$/.test(url.pathname)) return "请填写 API 基础地址（如 /v1），不要包含 /chat/completions 或 /models";
    } catch { return "请填写有效的端点地址"; }
    if (!Number.isInteger(Number(timeout)) || Number(timeout) < 1000 || Number(timeout) > 300000) return "超时必须在 1000–300000 ms 之间";
    if (existing && credentialConfigured && initial?.baseUrl.replace(/\/$/, "") !== baseUrl.trim().replace(/\/$/, "") && !apiKey.trim() && !clearKey) return "地址已修改，请重新输入密钥或明确清除已有密钥";
    return null;
  };
  const addModel = () => {
    const model = modelInput.trim();
    if (!model) return;
    changed();
    setModels((previous) => [...new Set([...previous, model])]);
    setModelInput("");
    setSearch("");
  };
  const removeModel = (model: string) => {
    changed();
    setModels((previous) => previous.filter((candidate) => candidate !== model));
    if (defaultModel === model) setDefaultModel("");
    if (summaryModel === model) setSummaryModel("");
  };
  const probeNow = async (kind: "models" | "model") => {
    const invalid = validate();
    if (invalid) { setError(invalid); return; }
    if (kind === "model" && !defaultModel) { setError("请先选择默认生成模型后测试"); return; }
    const current = ++attempt.current;
    const started = performance.now();
    setError(null);
    setProbe({ state: "checking", kind });
    try {
      const result = await onProbe(baseUrl.trim(), clearKey ? "" : apiKey.trim() || null, Number(timeout), {
        ...(existing ? { providerId: id } : {}), ...(kind === "model" ? { model: defaultModel } : {}),
      });
      if (current !== attempt.current) return;
      if (!result.ok) { setProbe({ state: "failed", message: result.error ?? "连接失败" }); return; }
      if (kind === "models") {
        const available = result.models.map((model) => model.trim()).filter(Boolean);
        setModels((previous) => [...new Set([...previous, ...available])]);
        if (available.length) onDirtyChange(true);
        setProbe({ state: "ok", message: available.length ? `获取成功；${available.length} 个模型，保留原有配置` : "服务未列出模型；可以手动添加模型 ID 后测试" });
      } else {
        setProbe({ state: "ok", message: `模型 ${defaultModel} 调用成功 · ${Math.round(performance.now() - started)} ms` });
      }
    } catch (reason) {
      if (current === attempt.current) setProbe({ state: "failed", message: isAppError(reason) ? reason.message : String(reason) });
    }
  };
  const save = async () => {
    if (modelInput.trim()) { setError("还有未添加的模型 ID，请先添加或清空"); return; }
    const invalid = validate();
    if (invalid) { setError(invalid); return; }
    if (!models.length) { setError("请获取模型或手动添加至少一个模型 ID"); return; }
    attempt.current += 1;
    setProbe({ state: "idle" });
    setSaving(true);
    onSavingChange(true);
    setError(null);
    try {
      await onSave({ id, displayName: displayName.trim(), baseUrl: baseUrl.trim().replace(/\/$/, ""), models,
        defaultGenerationModel: defaultModel || null, summaryModel: summaryModel || null, timeoutMs: Number(timeout), apiKey: clearKey ? "" : apiKey.trim() || null });
    } catch (reason) {
      setError(isAppError(reason) ? `${reason.code}: ${reason.message}` : String(reason));
    } finally { setSaving(false); onSavingChange(false); }
  };

  return (
    <div className="space-y-5 [&_[data-slot=native-select-wrapper]]:w-full">
      <div className="rounded-lg border bg-muted/30 px-3 py-2 text-xs text-muted-foreground">
        OpenAI 兼容 · Chat Completions{existing && credentialConfigured ? " · 已保存凭据，留空保留；不会回填密钥" : " · 本地服务允许不填写密钥"}
      </div>
      {error && <InlineStatus tone="error">{error}</InlineStatus>}
      <fieldset disabled={saving} className="space-y-5 disabled:opacity-60">
        <div className="grid gap-4 sm:grid-cols-2">
          <label className="space-y-1.5 text-sm"><span className="block font-medium">显示名称</span><Input value={displayName} onChange={(event) => { changed(); setDisplayName(event.target.value); }} placeholder="例如 工作 API / 本地 Ollama" /></label>
          <label className="space-y-1.5 text-sm"><span className="block font-medium">超时（1000–300000 ms）</span><Input type="number" min={1000} max={300000} value={timeout} onChange={(event) => { changed(); setTimeout(event.target.value); }} /></label>
          <label className="space-y-1.5 text-sm sm:col-span-2"><span className="block font-medium">端点地址（HTTPS 或本机回环）</span><Input aria-label="端点地址（HTTPS 或本机回环）" className="font-mono" value={baseUrl} onChange={(event) => { changed(); setBaseUrl(event.target.value); }} placeholder="https://api.example.com/v1" /><span className="block text-xs text-muted-foreground">填写基础地址；Fleqi 自动追加 /models 和 /chat/completions。</span></label>
          <div className="space-y-1.5 sm:col-span-2">
            <label className="block text-sm font-medium" htmlFor={`key-${id}`}>API 密钥（可选；本地服务可留空）</label>
            <div className="flex gap-2"><Input id={`key-${id}`} className="font-mono" type={showKey ? "text" : "password"} value={apiKey} disabled={clearKey} onChange={(event) => { changed(); setApiKey(event.target.value); }} placeholder={credentialConfigured ? "已保存；输入新密钥可替换" : "输入 API Key"} autoComplete="off" spellCheck={false} /><Button aria-label={showKey ? "隐藏新密钥" : "显示新密钥"} onClick={() => setShowKey(!showKey)}>{showKey ? <EyeOff /> : <Eye />}</Button></div>
            {credentialConfigured && <label className="flex items-center gap-2 text-xs text-muted-foreground"><input type="checkbox" checked={clearKey} onChange={(event) => { changed(); setClearKey(event.target.checked); setApiKey(""); }} />保存时清除已有密钥</label>}
          </div>
        </div>
        <section aria-label="配置模型" className="space-y-3 rounded-xl border p-4">
          <div className="flex flex-wrap items-center justify-between gap-2"><h3 className="font-medium">模型列表 <span className="text-xs font-normal text-muted-foreground">{models.length} 个</span></h3><Button onClick={() => void probeNow("models")} disabled={probe.state === "checking"}><RefreshCw className={probe.state === "checking" ? "animate-spin" : ""} />获取模型</Button></div>
          <p className="text-xs text-muted-foreground">不支持列出模型的服务，可以直接添加模型 ID。获取列表不会删除手动模型或改变用途。</p>
          <div className="flex gap-2"><Input aria-label="手动模型 ID" value={modelInput} onChange={(event) => { onDirtyChange(true); setModelInput(event.target.value); }} onKeyDown={(event) => { if (event.key === "Enter" && !event.nativeEvent.isComposing) { event.preventDefault(); addModel(); } }} placeholder="输入模型 ID" /><Button onClick={addModel} disabled={!modelInput.trim()}><Plus />添加模型</Button></div>
          <Input aria-label="搜索模型" placeholder="搜索已配置模型…" value={search} onChange={(event) => setSearch(event.target.value)} />
          <ul className="max-h-40 space-y-1 overflow-y-auto" aria-label="已配置模型">
            {models.filter((model) => model.toLowerCase().includes(search.toLowerCase())).map((model) => <li key={model} data-testid={`model-row-${model}`} className="flex items-center gap-2 rounded-md bg-muted/40 px-2 py-1"><span className="min-w-0 flex-1 break-all font-mono text-xs">{model}</span>{model === defaultModel && <span className="text-xs text-muted-foreground">生成</span>}{model === summaryModel && <span className="text-xs text-muted-foreground">摘要</span>}<Button aria-label={`移除模型 ${model}`} onClick={() => removeModel(model)}><Trash2 /></Button></li>)}
          </ul>
          {models.length === 0 && <p className="text-sm text-muted-foreground">尚未添加模型</p>}
          {models.length > 0 && !models.some((model) => model.toLowerCase().includes(search.toLowerCase())) && <p className="text-sm text-muted-foreground">没有匹配的模型</p>}
          <div className="grid gap-3 sm:grid-cols-2">
            <label className="space-y-1.5 text-sm"><span className="block font-medium">默认生成模型</span><NativeSelect aria-label="默认生成模型" className="w-full" value={defaultModel} onChange={(event) => { changed(); setDefaultModel(event.target.value); }}><option value="">使用列表中第一个模型</option>{models.map((model) => <option key={model} value={model}>{model}</option>)}</NativeSelect></label>
            <label className="space-y-1.5 text-sm"><span className="block font-medium">摘要模型（可选）</span><NativeSelect aria-label="摘要模型（可选）" className="w-full" value={summaryModel} onChange={(event) => { changed(); setSummaryModel(event.target.value); }}><option value="">跟随默认模型</option>{models.map((model) => <option key={model} value={model}>{model}</option>)}</NativeSelect></label>
          </div>
        </section>
        <div className="space-y-2"><div className="flex flex-wrap gap-2"><Button onClick={() => void probeNow("model")} disabled={probe.state === "checking" || !defaultModel}>测试模型</Button>{probe.state === "checking" && <Button onClick={() => { attempt.current += 1; setProbe({ state: "idle" }); }}>取消检查</Button>}</div><p className="text-xs text-muted-foreground">测试会发送一条简短对话，可能产生少量费用。取消停止等待，请求可能已发送。</p></div>
      </fieldset>
      {probe.state === "checking" && <InlineStatus tone="neutral">{probe.kind === "models" ? "正在获取模型…" : "正在测试模型…"}</InlineStatus>}
      {(probe.state === "ok" || probe.state === "failed") && <InlineStatus tone={probe.state === "ok" ? "success" : "error"}>{probe.message}</InlineStatus>}
      <div className="flex justify-end gap-2 border-t pt-4"><Button onClick={onCancel} disabled={saving}>取消编辑</Button><Button variant="primary" onClick={() => void save()} disabled={saving}>{saving ? "保存中…" : "保存端点"}</Button></div>
    </div>
  );
}
