import { Input } from "../../components/ui/input";
import { Textarea } from "../../components/ui/textarea";
import { NativeSelect } from "../../components/ui/native-select";
import { useEffect, useState } from "react";
import type { CapabilityForm } from "@fleqi/contracts";
import { Button } from "../../components/Button";
import { Card } from "../../components/Card";
import { InlineStatus } from "../../components/InlineStatus";
import { isAppError, newRequestId, type CatalogEntry } from "../../adapters/host";
import { useHost } from "../../store/host";
import { fileManagerLabel } from "../../platform-copy";

export function CapabilityEditor({ entry, onClose }: { entry: CatalogEntry; onClose: () => void }) {
  const host = useHost();
  const manager = fileManagerLabel(host.bootstrap?.buildInfo.targetOs);
  const [form, setForm] = useState<CapabilityForm | null>(null);
  const [parameters, setParameters] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [refresh, setRefresh] = useState(0);
  useEffect(() => {
    let current = true;
    setForm(null);
    setError(null);
    void host.adapter.capabilityForm(entry.id).then((value) => {
      if (!current) return;
      setForm(value);
      setParameters(Object.fromEntries(value.fields.map((field) => [field.key, field.defaultValue])));
    }, (reason: unknown) => { if (current) setError(isAppError(reason) ? reason.message : String(reason)); });
    return () => { current = false; };
  }, [host.adapter, entry.id, refresh]);

  const submit = async () => {
    if (!form || busy) return;
    setBusy(true);
    setError(null);
    try {
      let session = sessionId;
      if (!session) {
        session = (await host.adapter.sessionCreate(newRequestId("capability-session"))).id;
        setSessionId(session);
      }
      const run = await host.adapter.capabilitySubmit(newRequestId("capability"), session, form.capabilityId, form.context.id, parameters);
      await host.adapter.openWindow("console", `runs?session=${encodeURIComponent(run.sessionId)}&run=${encodeURIComponent(run.id)}`);
    } catch (reason) {
      setError(isAppError(reason) ? reason.message : String(reason));
    } finally { setBusy(false); }
  };
  const selectionChanged = !!form && !!host.bootstrap?.context && host.bootstrap.context.id !== form.context.id;
  const enoughInputs = form && form.context.selectedItems.length >= form.minimumInputs;

  return <Card title={entry.title} description={entry.description} actions={<Button disabled={busy} onClick={onClose}>关闭</Button>}>
    {error && <p role="alert" className="mb-3 text-sm text-error">{error}</p>}
    {!form ? <>{!error && <InlineStatus tone="loading">读取当前上下文…</InlineStatus>}<Button onClick={() => setRefresh((value) => value + 1)}>重新读取</Button></> : <form data-capability-context={form.context.id} data-capability-directory={form.context.directoryRef?.displayPath} onSubmit={(event) => { event.preventDefault(); void submit(); }} className="space-y-4">
      <div className="rounded-lg border border-border p-3 text-sm">
        <p className="break-all">目录：{form.context.directoryRef?.displayPath ?? "未选择工作文件夹"}</p>
        <p className="mt-1 text-xs text-muted-foreground">本次将处理以下文件。执行前请核对文件名。</p>
        {form.context.selectedItems.length > 0 && <ul className="mt-2 max-h-36 overflow-auto text-xs">{form.context.selectedItems.map((item) => <li className="break-all py-1" key={item.id}>{item.displayPath}</li>)}</ul>}
        {selectionChanged && <p role="status" className="mt-2 text-warning">{manager} 选区已变化，请重新读取并核对后提交。</p>}
        {!enoughInputs && <p className="mt-2 text-warning">请在 {manager} 至少选择 {form.minimumInputs} 项，然后重新读取。</p>}
        <Button type="button" className="mt-2" disabled={busy} onClick={() => {
          void host.adapter.contextRefresh().then(() => setRefresh((value) => value + 1), (reason: unknown) => setError(isAppError(reason) ? reason.message : String(reason)));
        }}>重新读取 {manager} 选区</Button>
      </div>
      {form.fields.map((field) => <label key={field.key} className="block space-y-1.5 text-sm">
        <span>{field.label}{field.required && <span className="ml-1 text-muted-foreground">*</span>}</span>
        {field.kind === "select" ? <NativeSelect data-testid={`capability-parameter-${field.key}`} className="block w-full rounded-lg border border-border bg-input px-3 py-2" value={parameters[field.key] ?? ""} disabled={busy} onChange={(event) => setParameters((current) => ({ ...current, [field.key]: event.target.value }))}>
          {field.choices.map((choice) => <option key={choice} value={choice}>{({ reject: "遇到透明像素时停止", flatten: "合成到指定背景", keep: "保留原文件", trashAfterSuccess: "原文件移入回收站" } as Record<string, string>)[choice] ?? choice}</option>)}
        </NativeSelect> : field.kind === "textarea" ? <Textarea data-testid={`capability-parameter-${field.key}`} className="block min-h-24 w-full rounded-lg border border-border bg-input px-3 py-2" value={parameters[field.key] ?? ""} required={field.required} disabled={busy} onChange={(event) => setParameters((current) => ({ ...current, [field.key]: event.target.value }))} /> : <Input data-testid={`capability-parameter-${field.key}`} className="block w-full rounded-lg border border-border bg-input px-3 py-2" type={field.kind === "password" ? "password" : field.kind === "number" ? "number" : "text"} autoComplete={field.kind === "password" ? "off" : undefined} step="any" value={parameters[field.key] ?? ""} required={field.required} disabled={busy} onChange={(event) => setParameters((current) => ({ ...current, [field.key]: event.target.value }))} />}
      </label>)}
      <p className="text-xs text-muted-foreground">{form.changesFiles ? "将按这些参数生成操作计划；是否需要确认遵循当前执行策略。" : "将按所列参数提交；执行前请核对目录、选区和操作目标。"} 所需工具、模型或系统权限见能力说明；缺少条件时会返回明确原因。</p>
      <Button type="submit" variant="primary" disabled={busy || selectionChanged || !enoughInputs || !form.context.directoryRef}>{busy ? "提交中…" : form.changesFiles ? "生成操作计划" : "开始读取"}</Button>
    </form>}
  </Card>;
}
