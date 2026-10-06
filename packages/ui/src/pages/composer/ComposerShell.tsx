import { Input } from "../../components/ui/input";
import { Textarea } from "../../components/ui/textarea";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ArrowUp, History, ListChecks, SquareTerminal, X } from "lucide-react";
import type { ProviderView, RunRecord, Session } from "@fleqi/contracts";
import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { InlineStatus } from "../../components/InlineStatus";
import { TrafficLights, WindowDragLayer } from "../../components/WindowChrome";
import { isAppError, newRequestId } from "../../adapters/host";
import { useHost } from "../../store/host";
import { usePresence } from "../../hooks/use-presence";
import type { PlanningView } from "./TaskPanel";

/** `!` 模式解析（FR-TERM-001）：首个非空白半角 `!`；全角/句中不触发。 */
export function parseMode(text: string): { mode: "ai" | "terminal"; body: string } {
  const trimmed = text.replace(/^\s+/, "");
  if (trimmed.startsWith("!")) {
    return { mode: "terminal", body: trimmed.slice(1) };
  }
  return { mode: "ai", body: text };
}

interface ComposerState {
  surface: "userHidden" | "visible" | "temporarilyHidden";
  sessionId: string | null;
  suppressed: boolean;
}

/** 提交/显示反馈可携带一个引导动作（ui-design.md：无模型/无热键/无会话给有效入口，不锁死输入）。 */
export type NoticeActionKind = "settings-models" | "settings-general" | "create-session" | "session-history";
export interface ComposerNotice {
  tone: "neutral" | "success" | "warning" | "error";
  text: string;
  action?: { label: string; kind: NoticeActionKind };
}

/** §7 结果气泡条目：完成结果与等待确认共用一个气泡位，多条完成按有界队列依次展示。 */
export interface BubbleEntry {
  kind: "approval" | "result";
  run: RunRecord;
  tone: "success" | "warning" | "error";
  text: string;
}

type ComposerFeedback = { kind: "bubble"; entry: BubbleEntry } | { kind: "queued"; line: string };

/** 完成结论（§7 简短结论）：无模型摘要时按真实输出末行与退出码如实归纳，不编造内容。 */
export function runConclusion(run: RunRecord): { tone: "success" | "warning" | "error"; text: string } {
  const lines = run.output
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
  const last = lines.at(-1) ?? "";
  const clip = (value: string, max = 120) => (value.length > max ? `${value.slice(0, max - 1)}…` : value);
  if (run.state === "partiallySucceeded") {
    return { tone: "warning", text: `部分成功：${last ? clip(last) : "部分步骤未通过"}` };
  }
  if (run.state === "failed") {
    const exit = run.exitStatus != null ? `（退出码 ${run.exitStatus}）` : "";
    return { tone: "error", text: `执行失败${exit}：${last ? clip(last) : "无输出"}` };
  }
  return { tone: "success", text: last ? clip(last) : "执行完成（无输出）" };
}

export function ComposerBar({ onOpenSessions, onOpenTerminal, sessionsOpen, panelOpen, selectionVersion, onFeedbackHeight, onSessionChange, onOpenTasks, onPlanningChange, onTaskMessage }: {
  onPlanningChange?: (value: PlanningView | null) => void;
  onTaskMessage?: (message: string) => void;
  onOpenTasks?: (runId?: string) => void;
  onOpenSessions: () => void;
  onOpenTerminal: () => void;
  sessionsOpen: boolean;
  panelOpen: boolean;
  selectionVersion: number;
  onFeedbackHeight: (height: number) => void;
  onSessionChange: (id: string | null) => void;
}) {
  const host = useHost();
  const [draft, setDraft] = useState("");
  const [submittedText, setSubmittedText] = useState<string | null>(null);
  useEffect(() => {
    if (!submittedText) return;
    const timer = setTimeout(() => setSubmittedText(null), 180);
    return () => clearTimeout(timer);
  }, [submittedText]);
  const [state, setState] = useState<ComposerState>({ surface: "userHidden", sessionId: null, suppressed: false });
  const [sessions, setSessions] = useState<Session[]>([]);
  const [notice, setNotice] = useState<ComposerNotice | null>(null);
  const [submitting, setSubmitting] = useState(false);
  /** 在途规划请求（run_plan_submit 的 requestId；用于取消）。 */
  const [planning, setPlanning] = useState<string | null>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const [providers, setProviders] = useState<ProviderView[] | null>(null);
  useEffect(() => {
    let alive = true;
    let version = 0;
    const refresh = () => {
      const request = ++version;
      void host.adapter.providerList().then((value) => { if (alive && request === version) setProviders(value); }, () => { if (alive) setProviders([]); });
    };
    refresh();
    const unsubscribe = host.adapter.subscribe((event) => { if (event.kind === "providersChanged") refresh(); });
    return () => { alive = false; unsubscribe(); };
  }, [host.adapter]);
  const reloadVersion = useRef(0);

  const [surfaceNotice, setSurfaceNotice] = useState<{ tone: "warning" | "error"; text: string; action?: { label: string; kind: NoticeActionKind } } | null>(null);

  /** §7 结果气泡：当前展示 + 有界队列（上限 3）+ 已展示去重 + 主条不可见时登记未读。 */
  const [bubble, setBubble] = useState<BubbleEntry | null>(null);
  const bubbleQueueRef = useRef<BubbleEntry[]>([]);
  const shownRunsRef = useRef<Set<string>>(new Set());
  const [unreadResults, setUnreadResults] = useState(0);
  const liveStateRef = useRef(state);
  useEffect(() => {
    liveStateRef.current = state;
  }, [state]);

  const pushBubble = useCallback((entry: BubbleEntry) => {
    setBubble((current) => {
      if (current) {
        bubbleQueueRef.current = [...bubbleQueueRef.current.slice(-2), entry];
        return current;
      }
      return entry;
    });
  }, []);

  const closeBubble = useCallback(() => {
    setBubble(() => bubbleQueueRef.current.shift() ?? null);
    inputRef.current?.focus();
  }, []);

  const reload = useCallback(async () => {
    const version = ++reloadVersion.current;
    try {
      const [list, surfaceResult] = await Promise.all([
        host.adapter.sessionList(),
        host.adapter.surfaceGet().then((value) => ({ ok: true as const, value }), (error: unknown) => ({ ok: false as const, error })),
      ]);
      if (version !== reloadVersion.current) return;
      setSessions(list.active);
      if (surfaceResult.ok) {
        setState({ surface: surfaceResult.value.visibility, sessionId: surfaceResult.value.visibleSessionId, suppressed: surfaceResult.value.autoShowSuppressed });
        const value = surfaceResult.value;
        setSurfaceNotice(value.visibility === "userHidden" && value.activation === "manual" && !host.bootstrap?.settings.hotkey
          ? { tone: "warning", text: "手动唤起需先设置快捷键", action: { label: "打开设置", kind: "settings-general" } }
          : null);
      } else if (isAppError(surfaceResult.error)) {
        // 显示状态警告与提交反馈分离：迟到的显示错误不覆盖用户动作结果。
        // manual 未注册热键被拒时给出设置入口（FR-ENTRY-003 引导）。
        setSurfaceNotice({ tone: "warning", text: surfaceResult.error.message, action: { label: "打开设置", kind: "settings-general" } });
      }
    } catch (reason) {
      if (version === reloadVersion.current) setSurfaceNotice({ tone: "error", text: isAppError(reason) ? reason.message : "读取会话失败" });
    }
  }, [host.adapter, host.bootstrap?.settings.hotkey]);

  useEffect(() => {
    void reload();
    return () => { reloadVersion.current += 1; };
  }, [reload, host.eventVersion, selectionVersion]);
  useEffect(() => { onSessionChange(state.sessionId); }, [state.sessionId, onSessionChange]);

  // §7：run 终态 → 拉真实记录 → 短结论气泡；主条不可见只登记未读，不强行唤起。
  useEffect(() => {
    let alive = true;
    const unsubscribe = host.adapter.subscribe((event) => {
      if (event.kind !== "runChanged") return;
      const runId = event.runId;
      void host.adapter.runGet(runId).then((run) => {
        if (!alive || shownRunsRef.current.has(run.id)) return;
        const terminal = run.state === "succeeded" || run.state === "partiallySucceeded" || run.state === "failed";
        if (!terminal) return;
        shownRunsRef.current.add(run.id);
        const current = liveStateRef.current;
        // 主条不可见（或无可见会话）时只登记未读，不强行唤起（§7）；可见时仅本会话的结果上气泡。
        if (current.surface !== "visible") {
          if (current.sessionId == null || run.sessionId === current.sessionId) {
            setUnreadResults((count) => count + 1);
          }
          return;
        }
        if (run.sessionId !== current.sessionId) return;
        pushBubble({ kind: "result", run, ...runConclusion(run) });
      }, () => {
        // run 记录可能已被清理：忽略该事件
      });
    });
    return () => {
      alive = false;
      unsubscribe();
    };
  }, [host.adapter, pushBubble]);

  const parsed = useMemo(() => parseMode(draft), [draft]);
  const contextDirectory = host.bootstrap?.context?.directoryRef?.displayPath ?? null;
  const activeSession = sessions.find((session) => session.id === state.sessionId);
  const displayDirectory = parsed.mode === "terminal" ? activeSession?.currentDirectory ?? contextDirectory : contextDirectory;
  const [queuedLine, setQueuedLine] = useState<string | null>(null);
  const bubbleSeconds = host.bootstrap ? host.bootstrap.settings.bubbleSeconds : 4.8;
  const feedback = useMemo<ComposerFeedback | null>(() => bubble ? { kind: "bubble", entry: bubble }
    : queuedLine ? { kind: "queued", line: queuedLine } : null, [bubble, queuedLine]);
  const feedbackPresence = usePresence(feedback);
  const shownFeedback = feedbackPresence.present;
  useEffect(() => { onFeedbackHeight(shownFeedback?.kind === "bubble" ? 160 : shownFeedback ? 64 : 0); }, [shownFeedback, onFeedbackHeight]);
  const modelSelection = host.bootstrap?.settings?.defaultModel;
  const modelLabel = (() => {
    if (!modelSelection) {
      if (providers === null) return "读取模型…";
      const endpoint = providers.find((view) => view.record.defaultGenerationModel) ?? providers[0];
      return endpoint?.record.defaultGenerationModel ?? endpoint?.record.models[0] ?? "未配置模型";
    }
    try { const value: unknown = JSON.parse(modelSelection); return Array.isArray(value) && typeof value[1] === "string" ? value[1] : modelSelection; }
    catch { return modelSelection; }
  })();

  const submit = async () => {
    if (submitting) return;
    const body = parsed.body.trim();
    if (parsed.mode === "ai") {
      if (!body) {
        setNotice({ tone: "neutral", text: "输入任务描述" });
        return;
      }
      if (!state.sessionId) {
        setNotice({ tone: "error", text: "没有可见会话；可新建会话或从会话选择器选择。", action: { label: "新建会话", kind: "create-session" } });
        return;
      }
      setSubmitting(true);
      setNotice(null);
      const planRequest = newRequestId("composer-plan");
      setPlanning(planRequest);
      onPlanningChange?.({ requestId: planRequest, prompt: body, cancelling: false });
      const submittedDraft = draft;
      setSubmittedText(draft);
      setDraft("");
      try {
        const outcome = await host.adapter.runPlanSubmit(
          planRequest,
          state.sessionId,
          host.bootstrap?.context?.id ?? "virtual",
          body,
        );
        if (outcome.kind === "execute") {
          onOpenTasks?.(outcome.run.id);
          setNotice(outcome.run.state === "awaitingApproval" ? { tone: "warning", text: "计划已准备好，请核对后确认" } : { tone: "success", text: "任务已提交；完成后在这里返回结果" });
        } else {
          onTaskMessage?.(outcome.text);
          setNotice({ tone: "neutral", text: "已返回摘要，未执行命令", action: { label: "查看完整回复", kind: "session-history" } });
        }
      } catch (error) {
        onTaskMessage?.(isAppError(error) ? error.message : String(error));
        setDraft((current) => current || submittedDraft);
        if (isAppError(error) && error.message === "已取消") {
          setNotice({ tone: "neutral", text: "已取消规划" });
        } else if (isAppError(error) && error.code === "unavailable") {
          // 无模型端点引导（ui-design.md §输入条）：保持可编辑，提交时给配置入口。
          setNotice({ tone: "warning", text: error.message, action: { label: "配置模型 API", kind: "settings-models" } });
        } else {
          setNotice({ tone: "error", text: isAppError(error) ? `${error.code}: ${error.message}` : String(error) });
        }
      } finally {
        setPlanning(null);
        onPlanningChange?.(null);
        setSubmitting(false);
      }
      return;
    }
    if (!body) {
      setNotice({ tone: "neutral", text: "输入终端命令" });
      return;
    }
    if (!state.sessionId) {
      setNotice({ tone: "error", text: "没有可见会话；可新建会话或从会话选择器选择。", action: { label: "新建会话", kind: "create-session" } });
      return;
    }
    setSubmitting(true);
    setNotice(null);
    try {
      const result = await host.adapter.terminalSubmitLine(
        newRequestId("composer"),
        state.sessionId,
        `!${body}`,
        host.bootstrap?.context?.revision ?? "1",
        (activeSession?.directorySync !== "synced" ? activeSession?.targetDirectory : activeSession?.currentDirectory) ?? contextDirectory ?? "",
      );
      if (result === "sent") {
        setNotice({ tone: "success", text: "已发送到终端" });
        setQueuedLine(null);
        setDraft("");
      } else {
        setQueuedLine(body);
        setNotice({ tone: "warning", text: "等待目录同步后自动发送（可取消）" });
      }
    } catch (error) {
      setNotice({ tone: "error", text: isAppError(error) ? `${error.code}: ${error.message}` : String(error) });
    } finally {
      setSubmitting(false);
    }
  };

  const cancelQueued = async () => {
    if (!state.sessionId) return;
    try {
      const withdrawn = await host.adapter.terminalCancelQueued(state.sessionId);
      setQueuedLine(null);
      if (withdrawn) setDraft(`!${withdrawn.text}`);
      setNotice({ tone: "neutral", text: "已取消排队；草稿已恢复" });
    } catch (error) {
      setNotice({ tone: "error", text: isAppError(error) ? error.message : String(error) });
    }
  };

  const hide = async () => {
    try {
      await host.adapter.surfaceHide();
      setNotice({ tone: "neutral", text: "已隐藏；会话按 hideBehavior 保留或结束。" });
      setState((s) => ({ ...s, surface: "userHidden", sessionId: null }));
    } catch (error) {
      setNotice({ tone: "error", text: isAppError(error) ? error.message : String(error) });
    }
  };

  const terminalMode = parsed.mode === "terminal";
  const activeCount = sessions.filter((s) => s.state === "active" || s.state === "ending").length;

  const createSession = async () => {
    try {
      const created = await host.adapter.sessionCreate(newRequestId("create"));
      const selected = await host.adapter.sessionSelect(newRequestId("select"), created.id);
      setState((s) => ({ ...s, sessionId: selected.id, surface: "visible" }));
      setSessions((list) => (list.some((item) => item.id === created.id) ? list : [created, ...list]));
      setNotice({ tone: "success", text: "已创建并切换到新会话" });
    } catch (error) {
      setNotice({ tone: "error", text: isAppError(error) ? error.message : String(error) });
    }
  };

  const renderAction = (action: { label: string; kind: NoticeActionKind }) => (
    <Button
      variant="secondary"
      className="shrink-0"
      data-testid="notice-action"
      data-action={action.kind}
      onClick={() => {
        if (action.kind === "create-session") void createSession();
        else if (action.kind === "session-history") void host.adapter.openWindow("console", `runs?session=${encodeURIComponent(state.sessionId ?? "")}`);
        else void host.adapter.openWindow("settings", action.kind === "settings-models" ? "models" : "general");
      }}
    >
      {action.label}
    </Button>
  );

  return (
    <main data-testid="composer" data-surface={state.surface} data-mode={parsed.mode} data-planning={!!planning} data-feedback-presence={feedbackPresence.phase} className="relative flex h-full flex-col rounded-2xl border border-border bg-background text-foreground">
      <WindowDragLayer />
      <div className="relative z-10 flex min-h-0 flex-1 flex-col">
        <div className="flex items-center gap-2 px-3 pt-2">
          <TrafficLights variant="close" closeLabel="隐藏输入条" onClose={() => void hide()} className="shrink-0" />
          <Button
            onClick={() => {
              setUnreadResults(0);
              onOpenSessions();
            }}
            aria-label="会话选择器"
            aria-expanded={sessionsOpen}
            title="会话与历史"
            className="relative shrink-0"
          >
            <History aria-hidden="true" className="size-4" />
            {activeCount > 0 && <span className="text-xs">{activeCount}</span>}
            {unreadResults > 0 && (
              <span data-testid="sessions-unread" className="absolute -right-1 -top-1 rounded-full bg-error px-1 text-[10px] font-medium leading-4 text-background">
                {unreadResults > 9 ? "9+" : unreadResults}
              </span>
            )}
          </Button>
          <Button onClick={() => onOpenTasks?.()} disabled={!state.sessionId} aria-label="打开任务浮层" title="任务"><ListChecks className="size-4" /></Button>
          <Badge tone={planning ? "error" : terminalMode ? "success" : "neutral"} data-testid="composer-mode">
            {planning ? "规划中" : terminalMode ? "终端" : "AI"}
          </Badge>
          <div className="relative min-w-0 flex-1">
            {submittedText && <span aria-hidden="true" className="submission-echo pointer-events-none absolute inset-0 truncate px-3 py-1.5 text-sm">{submittedText}</span>}
            <Textarea
              ref={inputRef}
              value={draft}
              rows={1}
              aria-label="任务输入"
              data-testid="composer-input"
              aria-describedby={planning ? "composer-planning-hint" : undefined}
              onChange={(event) => setDraft(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter" && !submitting && !event.nativeEvent.isComposing && event.nativeEvent.keyCode !== 229 && !event.shiftKey) {
                  event.preventDefault();
                  void submit();
                }
              }}
              placeholder={planning ? "正在规划，暂时无法发送；可先准备下一条信息" : terminalMode ? "" : "输入指令或问题；首个半角 ! 进入手动终端"}
              className={`h-8 min-h-8 w-full resize-none rounded-lg border bg-card px-3 ${host.bootstrap?.buildInfo.targetOs === "windows" ? "py-1" : "py-1.5"} text-sm leading-5 outline-none placeholder:text-muted-foreground ${
                terminalMode ? "border-success/50 text-terminal-mode-text" : "border-border"
              }`}
            />
          </div>
          <Button disabled={!state.sessionId} onClick={onOpenTerminal} aria-label="打开终端面板" title={state.sessionId ? "终端" : "请先创建或选择会话"}>
            <SquareTerminal aria-hidden="true" className="size-4" />
          </Button>
          <Button variant="primary" onClick={submit} disabled={submitting} aria-label="提交" className="shrink-0 rounded-full px-2.5" data-testid="composer-submit">
            <ArrowUp aria-hidden="true" className="size-4" />
          </Button>
        </div>
        <div className="flex min-h-6 items-center gap-2 px-3 pb-2 pt-1 text-xs text-muted-foreground" data-testid="composer-second-line">
          {planning ? <span id="composer-planning-hint" role="status" className="truncate text-error">正在规划 · 暂时无法发送，可先准备下一条信息</span> : <>
          <span className="truncate" data-testid="composer-directory" title={displayDirectory ?? undefined}>
            {displayDirectory ? `${displayDirectory}` : "请选择工作文件夹"}
            {activeSession?.targetDirectory && activeSession.directorySync !== "synced" && ` · 将切换到 ${activeSession.targetDirectory}`}
          </span>
          </>}
          {!terminalMode && !!host.bootstrap?.context?.selectedItems.length && <span className="shrink-0" data-testid="composer-selection" title={host.bootstrap.context.selectedItems.map((item) => item.displayPath).join("\n")}>已选 {host.bootstrap.context.selectedItems.length} 项 · {host.bootstrap.context.selectedItems[0]?.displayPath.split("/").pop()}</span>}
          {terminalMode && <span className="shrink-0 text-terminal-mode-text">手动终端 · 直接执行</span>}
          <span className="ml-auto flex min-w-0 max-w-[60%] items-center gap-2">
            {!terminalMode && !notice && !surfaceNotice && <span className="truncate" data-testid="composer-model">{modelLabel}</span>}
            {notice && (
              <span title={notice.text} className="composer-notice flex min-w-0 items-center gap-1" data-testid="composer-notice">
                <InlineStatus tone={notice.tone}>{notice.text}</InlineStatus>
                {notice.action && renderAction(notice.action)}
              </span>
            )}
            {!notice && surfaceNotice && (
              <span className="flex shrink-0 items-center gap-1" data-testid="composer-surface-notice">
                <InlineStatus tone={surfaceNotice.tone}>{surfaceNotice.text}</InlineStatus>
                {surfaceNotice.action && renderAction(surfaceNotice.action)}
              </span>
            )}
          </span>
        </div>
        {shownFeedback?.kind === "queued" && !panelOpen && (
          <div inert={feedbackPresence.phase === "closing"} className="composer-feedback absolute inset-x-3 bottom-full mb-2 flex items-center gap-2 rounded-xl border border-border bg-card px-3 py-2 text-xs shadow-lg" data-testid="queued-line">
            <span className="truncate font-mono">{shownFeedback.line}</span>
            <Button className="ml-auto shrink-0" onClick={() => void cancelQueued()} aria-label="取消排队命令">
              取消排队
            </Button>
          </div>
        )}
      </div>
      {shownFeedback?.kind === "bubble" && !panelOpen && (
        <ResultBubble
          key={shownFeedback.entry.run.id}
          entry={shownFeedback.entry}
          closing={feedbackPresence.phase === "closing"}
          seconds={bubbleSeconds}
          onClose={closeBubble}
          onDetail={() => {
            closeBubble();
            onOpenTasks?.(shownFeedback.entry.run.id);
          }}
          onRespond={(prompt) => {
            setDraft(`请就「${prompt.slice(0, 40)}」继续说明：`);
            closeBubble();
            inputRef.current?.focus();
          }}
        />
      )}
    </main>
  );
}

/** §7 结果气泡：锚定主条左侧上方带尖角；第一行结论+详情+关闭，第二行复制/回应；悬停/焦点暂停计时。 */
function ResultBubble({ entry, seconds, closing, onClose, onDetail, onRespond }: {
  entry: BubbleEntry;
  seconds: number | null;
  closing: boolean;
  onClose: () => void;
  onDetail: () => void;
  onRespond: (prompt: string) => void;
}) {
  const [paused, setPaused] = useState(false);
  const [copyState, setCopyState] = useState<"idle" | "copied" | "failed">("idle");

  useEffect(() => {
    if (seconds == null || paused || entry.tone !== "success") return;
    const timer = setTimeout(onClose, seconds * 1000);
    return () => clearTimeout(timer);
  }, [seconds, paused, entry, onClose]);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(entry.run.output || entry.run.prompt);
      setCopyState("copied");
    } catch {
      setCopyState("failed");
    }
  };

  return (
    <div
      data-testid="result-bubble"
      data-bubble-kind={entry.kind}
      role="status"
      inert={closing}
      onMouseEnter={() => setPaused(true)}
      onMouseLeave={() => setPaused(false)}
      onFocusCapture={() => setPaused(true)}
      onBlurCapture={() => setPaused(false)}
      className="composer-feedback absolute bottom-full left-2 z-20 mb-2 w-[calc(100%-1rem)] max-w-[420px] rounded-xl border border-border bg-card p-2 shadow-lg"
    >
      <span aria-hidden="true" className="absolute left-6 top-full -mt-1.5 size-2.5 rotate-45 border-b border-r border-border bg-card" />
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          <InlineStatus tone={entry.tone}>
            <span className="line-clamp-2 whitespace-pre-wrap">{entry.text}</span>
          </InlineStatus>
        </div>
        <Button variant="secondary" className="shrink-0" data-testid="bubble-detail" onClick={onDetail}>
          详情
        </Button>
        <Button className="shrink-0" aria-label="关闭结果" data-testid="bubble-close" onClick={onClose}>
          <X aria-hidden="true" className="size-3.5" />
        </Button>
      </div>
      <div className="mt-1 flex items-center gap-2 pl-6 text-xs text-muted-foreground">
        <button type="button" className="hover:text-foreground" data-testid="bubble-copy" onClick={() => void copy()}>
          {copyState === "copied" ? "已复制" : copyState === "failed" ? "复制失败" : "复制结果"}
        </button>
        <span aria-hidden="true">·</span>
        <button type="button" className="hover:text-foreground" data-testid="bubble-respond" onClick={() => onRespond(entry.run.prompt)}>
          回应
        </button>
        <span className="ml-auto">{seconds == null ? "常驻" : `${Math.round(seconds)} 秒`}</span>
      </div>
    </div>
  );
}

export interface QueuedDraft {
  requestId: string;
  text: string;
  reason: string;
}

/** 会话选择器气泡（UI-SESSION-SELECTOR）：活跃/历史分组、置顶、删除在图钉左。 */
export function SessionSelector({ onSelect, onClose }: { onSelect: (session: Session) => void; onClose: () => void }) {
  const host = useHost();
  const [sessions, setSessions] = useState<Session[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [query, setQuery] = useState("");
  const [historySession, setHistorySession] = useState<Session | null>(null);
  const [historyRuns, setHistoryRuns] = useState<RunRecord[] | null>(null);

  useEffect(() => {
    setHistoryRuns(null);
    if (!historySession) return;
    let current = true;
    void host.adapter.runList(historySession.id).then((runs) => {
      if (current) setHistoryRuns(runs);
    }, (reason: unknown) => { if (current) setError(isAppError(reason) ? reason.message : String(reason)); });
    return () => { current = false; };
  }, [historySession, host.adapter]);

  useEffect(() => {
    host.adapter
      .sessionList()
      .then((list) => setSessions([...list.active, ...list.history]))
      .catch((reason: unknown) => setError(isAppError(reason) ? reason.message : String(reason)));
  }, [host.adapter, host.eventVersion]);

  const filtered = sessions.filter((session) => `${session.title} ${session.currentDirectory ?? ""}`.toLowerCase().includes(query.trim().toLowerCase()));
  const pinned = filtered.filter((s) => s.pinned);
  const active = filtered.filter((s) => !s.pinned && (s.state === "active" || s.state === "ending"));
  const history = filtered.filter((s) => !s.pinned && s.state !== "active" && s.state !== "ending");

  const end = async (session: Session) => {
    try {
      await host.adapter.sessionEnd(newRequestId("end"), session.id);
      const list = await host.adapter.sessionList();
      setSessions([...list.active, ...list.history]);
    } catch (reason) {
      setError(isAppError(reason) ? reason.message : String(reason));
    }
  };

  const pin = async (session: Session, next: boolean) => {
    try {
      await host.adapter.sessionPin(session.id, next, session.revision);
      const list = await host.adapter.sessionList();
      setSessions([...list.active, ...list.history]);
    } catch (reason) {
      setError(isAppError(reason) ? reason.message : String(reason));
    }
  };

  const continueSession = async (session: Session) => {
    try {
      const created = await host.adapter.sessionContinue(newRequestId("continue"), session.id);
      onSelect(created);
    } catch (reason) {
      setError(isAppError(reason) ? reason.message : String(reason));
    }
  };

  const create = async () => {
    setCreating(true);
    try {
      const created = await host.adapter.sessionCreate(newRequestId("create"));
      onSelect(created);
    } catch (reason) {
      setError(isAppError(reason) ? reason.message : String(reason));
    } finally {
      setCreating(false);
    }
  };

  const remove = async (session: Session) => {
    try {
      await host.adapter.sessionDelete(newRequestId("delete"), session.id, session.revision);
      const list = await host.adapter.sessionList();
      setSessions([...list.active, ...list.history]);
    } catch (reason) {
      setError(isAppError(reason) ? reason.message : String(reason));
    }
  };

  const renderRow = (session: Session) => (
    <li key={session.id} data-testid="session-row" data-session-id={session.id} data-state={session.state} className="flex items-center gap-2 rounded-lg px-2 py-1.5 hover:bg-accent-surface">
      <button type="button" className="min-w-0 flex-1 text-left" onClick={() => session.state === "active" || session.state === "ending" ? onSelect(session) : setHistorySession(session)}>
        <p className="truncate text-sm">{session.title}</p>
        <p className="truncate text-xs text-muted-foreground">{session.currentDirectory ?? "无目录"}</p>
      </button>
      <span className="shrink-0 text-[11px] text-muted-foreground">{session.state === "active" ? "活跃" : session.state === "ended" ? "已结束" : session.state}</span>
      <Button aria-label={`结束会话 ${session.title}`} onClick={() => void end(session)} disabled={session.state !== "active"}>
        结束
      </Button>
      {session.state !== "active" && session.state !== "ending" && (
        <>
          <Button
            variant="secondary"
            data-testid="session-continue"
            aria-label={`从历史继续 ${session.title}`}
            onClick={() => void continueSession(session)}
          >
            继续
          </Button>
          <Button aria-label={`删除会话 ${session.title}`} onClick={() => void remove(session)}>
            删除
          </Button>
        </>
      )}
      <Button aria-label={session.pinned ? `取消置顶 ${session.title}` : `置顶 ${session.title}`} onClick={() => void pin(session, !session.pinned)}>
        {session.pinned ? "取消置顶" : "置顶"}
      </Button>
    </li>
  );

  return (
    <section aria-label="会话选择器" data-testid="session-selector" className="composer-panel absolute left-0 bottom-[78px] top-0 z-20 w-[min(460px,100%)] overflow-auto rounded-[14px] border border-border bg-card p-2 shadow-lg">
      <header className="mb-1 flex items-center justify-between px-1">
        <h2 className="text-sm font-semibold">会话</h2>
        <div className="flex items-center gap-1">
          <Button variant="secondary" data-testid="session-create" onClick={() => void create()} disabled={creating}>
            新建会话
          </Button>
          <Button onClick={onClose} aria-label="关闭会话选择器">
            关闭
          </Button>
        </div>
      </header>
      {error && <InlineStatus tone="error">{error}</InlineStatus>}
      {historySession ? <div className="space-y-3 p-2">
        <Button onClick={() => setHistorySession(null)}>返回会话列表</Button>
        <h3 className="text-sm font-medium">{historySession.title} · 只读历史</h3>
        <p className="break-all text-xs text-muted-foreground">{historySession.currentDirectory ?? "未记录目录"}</p>
        {historyRuns === null ? <InlineStatus tone="loading">读取历史任务…</InlineStatus> : historyRuns.length === 0 ? <p className="text-xs text-muted-foreground">此会话没有 AI 任务记录。</p> : historyRuns.map((run) => <div key={run.id} className="rounded-lg border border-border p-2"><p className="text-sm">{run.prompt}</p><pre className="mt-2 max-h-40 overflow-auto whitespace-pre-wrap break-all text-xs">{run.output || "无输出"}</pre></div>)}
        <Button onClick={() => void continueSession(historySession)}>继续为新会话</Button>
      </div> : <>
      <Input autoFocus aria-label="搜索会话" placeholder="搜索会话或目录" value={query} onChange={(event) => setQuery(event.target.value)} className="mb-2 w-full rounded-lg border border-border bg-input px-3 py-1.5 text-sm" />
      {pinned.length > 0 && (
        <>
          <p className="px-1 text-[11px] uppercase text-muted-foreground">置顶</p>
          <ul>{pinned.map(renderRow)}</ul>
        </>
      )}
      <p className="px-1 text-[11px] uppercase text-muted-foreground">活跃</p>
      <ul>{active.length ? active.map(renderRow) : <li className="px-2 py-1 text-xs text-muted-foreground">暂无活跃会话</li>}</ul>
      {history.length > 0 && (
        <>
          <p className="px-1 text-[11px] uppercase text-muted-foreground">历史</p>
          <ul>{history.map(renderRow)}</ul>
        </>
      )}
      <p className="px-2 py-1 text-[11px] text-muted-foreground">删除仅移除会话记录，不删除用户文件。历史会话可以继续为新会话。</p>
      </>}
    </section>
  );
}
