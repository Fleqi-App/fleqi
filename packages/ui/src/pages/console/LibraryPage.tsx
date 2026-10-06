import { Input } from "../../components/ui/input";
import { Textarea } from "../../components/ui/textarea";
import { NativeSelect } from "../../components/ui/native-select";
import { useCallback, useEffect, useState } from "react";
import type { Favorite, Rule, RuleScope } from "@fleqi/contracts";
import { Plus, Star, Trash2 } from "lucide-react";

import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { Card } from "../../components/Card";
import { EmptyState, InlineStatus } from "../../components/InlineStatus";
import type { CatalogEntry } from "../../adapters/host";
import { fileManagerLabel } from "../../platform-copy";
import { isAppError, newRequestId } from "../../adapters/host";
import { useHost } from "../../store/host";
import { categoryLabel } from "./catalog";
import { CapabilityEditor } from "./CapabilityEditor";

/** 能力库（UI-CAPABILITY-LIBRARY）：六类能力目录 + 规则 + 收藏（ui-design.md §9.4/§8）。 */
export function LibraryPage() {
  const host = useHost();
  const [catalog, setCatalog] = useState<CatalogEntry[] | null>(null);
  const [rules, setRules] = useState<Rule[] | null>(null);
  const [favorites, setFavorites] = useState<Favorite[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [ruleDraft, setRuleDraft] = useState({ name: "", content: "" });
  const [favoriteDraft, setFavoriteDraft] = useState({ name: "", content: "" });
  const [ruleScope, setRuleScope] = useState<RuleScope["kind"]>("global");
  const [rulePath, setRulePath] = useState("");
  const [rulePhrase, setRulePhrase] = useState("");
  const [editingRule, setEditingRule] = useState<string | null>(null);
  const [favoriteKind, setFavoriteKind] = useState<"ai" | "manual">("ai");
  const [reuse, setReuse] = useState<Favorite | null>(null);
  const [reuseBusy, setReuseBusy] = useState(false);
  const [reuseMessage, setReuseMessage] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState(() => new URLSearchParams(window.location.hash.split("?")[1]).get("category") ?? "all");
  const [selected, setSelected] = useState<CatalogEntry | null>(null);

  const reload = useCallback(async () => {
    try {
      const [catalogEntries, ruleList, favoriteList] = await Promise.all([
        host.adapter.catalogQuery(),
        host.adapter.rulesList(),
        host.adapter.favoritesList(),
      ]);
      setCatalog(catalogEntries);
      const requested = new URLSearchParams(window.location.hash.split("?")[1]).get("capability");
      if (requested) setSelected(catalogEntries.find((entry) => entry.id === requested) ?? null);
      setRules(ruleList);
      setFavorites(favoriteList);
      setError(null);
    } catch (reason) {
      setError(isAppError(reason) ? `${reason.code}: ${reason.message}` : String(reason));
    }
  }, [host.adapter]);

  useEffect(() => {
    void reload();
  }, [reload, host.eventVersion]);

  const addRule = async () => {
    if (!ruleDraft.name.trim() || !ruleDraft.content.trim()) return;
    try {
      const scope: RuleScope = ruleScope === "directory" ? { kind: ruleScope, path: rulePath } : ruleScope === "phrase" ? { kind: ruleScope, phrase: rulePhrase } : ruleScope === "directoryAndPhrase" ? { kind: ruleScope, path: rulePath, phrase: rulePhrase } : { kind: "global" };
      if (editingRule) await host.adapter.rulesUpdate(editingRule, ruleDraft);
      else await host.adapter.rulesCreate(ruleDraft.name, ruleDraft.content, scope);
      setEditingRule(null);
      setRuleDraft({ name: "", content: "" });
      await reload();
    } catch (reason) {
      setError(isAppError(reason) ? `${reason.code}: ${reason.message}` : String(reason));
    }
  };

  const addFavorite = async () => {
    if (!favoriteDraft.name.trim() || !favoriteDraft.content.trim()) return;
    try {
      await host.adapter.favoritesCreate(favoriteDraft.name, favoriteDraft.content, favoriteKind);
      setFavoriteDraft({ name: "", content: "" });
      await reload();
    } catch (reason) {
      setError(isAppError(reason) ? `${reason.code}: ${reason.message}` : String(reason));
    }
  };

  const categories = [...new Set((catalog ?? []).map((entry) => entry.category))];
  const visibleEntries = (catalog ?? []).filter((entry) => (category === "all" || entry.category === category) && `${entry.id} ${entry.title} ${entry.description} ${entry.inputs}`.toLowerCase().includes(query.trim().toLowerCase()));

  return (
    <div className="space-y-4">
      <h2 className="text-xl font-semibold leading-7">能力库</h2>
      {error && <InlineStatus tone="error">{error}</InlineStatus>}
      {selected && <CapabilityEditor key={selected.id} entry={selected} onClose={() => setSelected(null)} />}

      <Card title="能力目录" description="先核对输入与依赖；条目列出所需条件，不代表本机已具备全部工具。">
        <div className="mb-4 flex flex-wrap gap-2">
          <Input aria-label="搜索能力" placeholder="搜索名称、描述或能力 ID" value={query} onChange={(event) => setQuery(event.target.value)} className="min-w-0 flex-1 rounded-lg border border-border bg-input px-3 py-2 text-sm" />
          <NativeSelect aria-label="能力分类" value={category} onChange={(event) => setCategory(event.target.value)} className="rounded-lg border border-border bg-input px-3 py-2 text-sm">
            <option value="all">全部分类</option>
            {categories.map((id) => <option key={id} value={id}>{categoryLabel(id)}</option>)}
          </NativeSelect>
        </div>
        {catalog === null ? (
          <InlineStatus tone="neutral">正在读取目录…</InlineStatus>
        ) : (
          <div className="space-y-3">
            {visibleEntries.length === 0 && <EmptyState title="没有匹配的能力" description="尝试其他关键词或切换分类。" />}
            {categories.filter((id) => visibleEntries.some((entry) => entry.category === id)).map((category) => (
              <div key={category}>
                <h3 className="mb-2 text-sm font-semibold">{categoryLabel(category)}</h3>
                <ul className="grid gap-1 sm:grid-cols-2">
                  {visibleEntries
                    .filter((entry) => entry.category === category)
                    .map((entry) => (
                      <li key={entry.id} className="rounded-md border border-border px-2 py-1.5 text-sm" data-testid="catalog-entry" data-capability-id={entry.id}>
                        <div className="flex items-center justify-between gap-2">
                          <button type="button" disabled={entry.availability === "unsupported"} className="min-h-7 text-left font-medium text-navigation-icon hover:underline disabled:opacity-50 disabled:no-underline" onClick={() => setSelected(entry)}>{entry.title}</button>
                          {entry.dependencies.length > 0 && <Badge tone="warning">{entry.dependencies.join("、")}</Badge>}
                        </div>
                        <p className="mt-0.5 text-xs text-muted-foreground">{entry.description}</p>
                        <p className="mt-2 text-xs text-muted-foreground">输入：{entry.inputs}</p>
                        {entry.unavailableReason && <p className="mt-1 text-xs text-muted-foreground">{entry.unavailableReason}</p>}
                      </li>
                    ))}
                </ul>
              </div>
            ))}
          </div>
        )}
      </Card>

      <Card title="规则" description="四种作用域（全局/目录/短语/目录且短语）；每次任务重新求值，不形成永久批准。">
        <div className="mb-3 grid gap-2 sm:grid-cols-[1fr_2fr_auto]">
          <Input
            className="rounded-md border border-border bg-input px-2 py-1.5 text-sm"
            placeholder="规则名称"
            value={ruleDraft.name}
            onChange={(event) => setRuleDraft((draft) => ({ ...draft, name: event.target.value }))}
          />
          <Input
            className="rounded-md border border-border bg-input px-2 py-1.5 text-sm"
            placeholder="规则内容（注入任务提示）"
            value={ruleDraft.content}
            onChange={(event) => setRuleDraft((draft) => ({ ...draft, content: event.target.value }))}
          />
          <Button variant="secondary" onClick={() => void addRule()} disabled={!ruleDraft.name.trim() || !ruleDraft.content.trim()}>
            <Plus aria-hidden="true" />
            {editingRule ? "保存修改" : "添加规则"}
          </Button>
        </div>
        {!editingRule && <div className="mb-3 flex flex-wrap gap-2"><NativeSelect aria-label="规则作用范围" value={ruleScope} onChange={(event) => setRuleScope(event.target.value as RuleScope["kind"])} className="rounded-md border border-border bg-input p-2 text-sm"><option value="global">全局</option><option value="directory">目录</option><option value="phrase">短语</option><option value="directoryAndPhrase">目录与短语</option></NativeSelect>{(ruleScope === "directory" || ruleScope === "directoryAndPhrase") && <Input aria-label="规则目录" placeholder="目录绝对路径" value={rulePath} onChange={(event) => setRulePath(event.target.value)} className="rounded-md border border-border bg-input p-2 text-sm" />}{(ruleScope === "phrase" || ruleScope === "directoryAndPhrase") && <Input aria-label="匹配短语" placeholder="请求包含的短语" value={rulePhrase} onChange={(event) => setRulePhrase(event.target.value)} className="rounded-md border border-border bg-input p-2 text-sm" />}</div>}
        {editingRule && <Button onClick={() => { setEditingRule(null); setRuleDraft({ name: "", content: "" }); }}>取消修改</Button>}
        {rules === null ? (
          <InlineStatus tone="neutral">正在读取…</InlineStatus>
        ) : rules.length === 0 ? (
          <EmptyState title="暂无规则" description="规则在计划形成阶段生效并记录命中；不能越过当前请求或执行策略。" />
        ) : (
          <ul className="space-y-1">
            {rules.map((rule) => (
              <li key={rule.id} className="flex items-center justify-between gap-2 rounded-md border border-border px-2 py-1.5 text-sm">
                <div className="min-w-0">
                  <span className="font-medium">{rule.name}</span>
                  <span className="ml-2 truncate text-xs text-muted-foreground">{rule.content}</span>
                </div>
                <div className="flex shrink-0 items-center gap-2">
                  <Badge tone={rule.enabled ? "success" : "neutral"}>{rule.enabled ? "启用" : "停用"}</Badge>
                  <Badge tone="neutral">{{ global: "全局", directory: "目录", phrase: "短语", directoryAndPhrase: "目录与短语" }[rule.scope.kind]}</Badge>
                  <Button variant="secondary" onClick={() => { setEditingRule(rule.id); setRuleDraft({ name: rule.name, content: rule.content }); }}>编辑</Button>
                  <Button variant="secondary" onClick={() => { void host.adapter.rulesUpdate(rule.id, { enabled: !rule.enabled }).then(reload).catch((reason: unknown) => setError(isAppError(reason) ? reason.message : String(reason))); }}>{rule.enabled ? "停用" : "启用"}</Button>
                  <Button
                    variant="secondary"
                    aria-label={`删除规则 ${rule.name}`}
                    onClick={() => {
                      void (async () => {
                        try {
                          await host.adapter.rulesDelete(rule.id);
                          await reload();
                        } catch (reason) {
                          setError(isAppError(reason) ? `${reason.code}: ${reason.message}` : String(reason));
                        }
                      })();
                    }}
                  >
                    <Trash2 aria-hidden="true" />
                  </Button>
                </div>
              </li>
            ))}
          </ul>
        )}
      </Card>

      <Card title="收藏" description="自然语言任务与命令均可收藏；复用时重新绑定当前上下文并按当前 AI 策略判定。">
        <div className="mb-3 grid gap-2 sm:grid-cols-[1fr_2fr_auto]">
          <Input
            className="rounded-md border border-border bg-input px-2 py-1.5 text-sm"
            placeholder="收藏名称"
            value={favoriteDraft.name}
            onChange={(event) => setFavoriteDraft((draft) => ({ ...draft, name: event.target.value }))}
          />
          <Input
            className="rounded-md border border-border bg-input px-2 py-1.5 font-mono text-sm"
            placeholder="命令或提示"
            value={favoriteDraft.content}
            onChange={(event) => setFavoriteDraft((draft) => ({ ...draft, content: event.target.value }))}
          />
          <Button variant="secondary" onClick={() => void addFavorite()} disabled={!favoriteDraft.name.trim() || !favoriteDraft.content.trim()}>
            <Star aria-hidden="true" />
            收藏
          </Button>
        </div>
        <label className="mb-3 flex items-center gap-2 text-sm">收藏类型<NativeSelect aria-label="收藏类型" value={favoriteKind} onChange={(event) => setFavoriteKind(event.target.value as "ai" | "manual")} className="rounded-md border border-border bg-input p-2"><option value="ai">自然语言任务</option><option value="manual">手动终端命令</option></NativeSelect></label>
        {reuse && <div className="mb-3 space-y-2 rounded-lg border border-border p-3"><p className="text-sm">复用「{reuse.name}」· {reuse.kind === "manual" ? "手动命令将直接发送终端" : "按当前上下文与 AI 策略生成计划"}</p><Textarea aria-label="复用内容" value={reuse.content} onChange={(event) => setReuse({ ...reuse, content: event.target.value })} className="min-h-24 w-full rounded-md border border-border bg-input p-2 text-sm" /><p className="break-all text-xs text-muted-foreground">提交时绑定最新 {fileManagerLabel(host.bootstrap?.buildInfo.targetOs)} 工作目录；原收藏保持不变。</p>{reuseMessage && <p className="whitespace-pre-wrap text-sm">{reuseMessage}</p>}<Button disabled={reuseBusy || !reuse.content.trim()} onClick={() => { void (async () => {
          setReuseBusy(true); setError(null); setReuseMessage(null);
          try {
            const context = await host.adapter.contextRefresh();
            if (!context.directoryRef) throw new Error(`请先在 ${fileManagerLabel(host.bootstrap?.buildInfo.targetOs)} 打开工作文件夹`);
            const session = await host.adapter.sessionCreate(newRequestId("favorite-session"));
            if (reuse.kind === "manual") {
              await host.adapter.sessionSelect(newRequestId("favorite-select"), session.id);
              await host.adapter.terminalOpen(session.id);
              const submitted = await host.adapter.terminalSubmitLine(newRequestId("favorite-manual"), session.id, `!${reuse.content.replace(/^\s*!/, "")}`, context.revision, context.directoryRef.displayPath);
              setReuseMessage(submitted === "sent" ? "已发送到终端，可在输入条打开终端面板查看。" : "已等待安全提示符；可在输入条取消排队命令。");
            } else {
              const outcome = await host.adapter.runPlanSubmit(newRequestId("favorite-ai"), session.id, context.id, reuse.content);
              if (outcome.kind === "execute") await host.adapter.openWindow("console", `runs?session=${encodeURIComponent(session.id)}&run=${encodeURIComponent(outcome.run.id)}`);
              else setReuseMessage(outcome.text);
            }
          } catch (reason) { setError(isAppError(reason) ? reason.message : String(reason)); }
          finally { setReuseBusy(false); }
        })(); }}>{reuseBusy ? "提交中…" : reuse.kind === "manual" ? "发送到终端" : "生成任务"}</Button><Button disabled={reuseBusy} onClick={() => setReuse(null)}>关闭</Button></div>}
        {favorites === null ? (
          <InlineStatus tone="neutral">正在读取…</InlineStatus>
        ) : favorites.length === 0 ? (
          <EmptyState title="暂无收藏" description="重启后收藏保留；手动命令收藏继续走手动终端路径。" />
        ) : (
          <ul className="space-y-1">
            {favorites.map((favorite) => (
              <li key={favorite.id} className="flex items-center justify-between gap-2 rounded-md border border-border px-2 py-1.5 text-sm">
                <div className="min-w-0">
                  <span className="font-medium">{favorite.name}</span>
                  <span className="ml-2 truncate font-mono text-xs text-muted-foreground">{favorite.content}</span>
                </div>
                <div className="flex shrink-0 items-center gap-2">
                  <Button variant="secondary" onClick={() => { setReuse(favorite); setReuseMessage(null); }}>复用</Button>
                  <Badge tone={favorite.kind === "ai" ? "warning" : "neutral"}>{favorite.kind === "ai" ? "AI" : "手动"}</Badge>
                  <Button
                    variant="secondary"
                    aria-label={`删除收藏 ${favorite.name}`}
                    onClick={() => {
                      void (async () => {
                        try {
                          await host.adapter.favoritesDelete(favorite.id);
                          await reload();
                        } catch (reason) {
                          setError(isAppError(reason) ? `${reason.code}: ${reason.message}` : String(reason));
                        }
                      })();
                    }}
                  >
                    <Trash2 aria-hidden="true" />
                  </Button>
                </div>
              </li>
            ))}
          </ul>
        )}
      </Card>
    </div>
  );
}
