import { useCallback, useEffect, useState } from "react";
import { AlertTriangle, Loader2 } from "lucide-react";
import { Button } from "./components/Button";
import { ComposerBar, SessionSelector } from "./pages/composer/ComposerShell";
import { TaskPanel, type PlanningView } from "./pages/composer/TaskPanel";
import { TerminalPanel } from "./pages/composer/TerminalPanel";
import { ConsoleShell } from "./pages/console/ConsoleShell";
import { SettingsShell } from "./pages/settings/SettingsShell";
import { parseRoute, routeHash, type Route } from "./router";
import { HostProvider, useHost } from "./store/host";
import type { HostAdapter } from "./adapters/host";
import { isAppError, newRequestId } from "./adapters/host";
import { usePresence } from "./hooks/use-presence";

function useRoute(): [Route, (page: string) => void] {
  const [route, setRoute] = useState<Route>(() => parseRoute(window.location.hash));
  useEffect(() => {
    const onChange = () => setRoute(parseRoute(window.location.hash));
    window.addEventListener("hashchange", onChange);
    return () => window.removeEventListener("hashchange", onChange);
  }, []);
  const navigate = (page: string) => {
    window.location.hash = routeHash(route.window, page);
  };
  return [route, navigate];
}

/** 加载/失败态不展示旧值为本次成功；失败可重试。 */
function HostGate() {
  const host = useHost();
  const [route, navigate] = useRoute();
  if (host.phase === "loading") {
    return (
      <div role="status" aria-live="polite" data-phase="loading" data-window-root="shell" className="flex h-full items-center justify-center gap-2 bg-background text-sm text-muted-foreground">
        <Loader2 data-slot="spinner" aria-hidden="true" className="size-4" />
        正在读取宿主状态…（第 {host.attempt} 次）
      </div>
    );
  }
  if (host.phase === "failed" || !host.bootstrap) {
    return (
      <div role="alert" data-phase="failed" data-window-root="shell" className="flex h-full flex-col items-center justify-center gap-3 bg-background p-6 text-foreground">
        <AlertTriangle aria-hidden="true" className="size-6 text-error" />
        <p className="font-medium">无法读取宿主状态</p>
        <p className="text-sm text-muted-foreground">
          <code className="font-mono">{host.error?.code}</code> · {host.error?.message}
        </p>
        <Button variant="primary" onClick={host.reload}>
          重试
        </Button>
      </div>
    );
  }
  return (
    <div data-phase="ready" data-window-role={route.window} data-window-root={route.window} data-attempt={host.attempt} data-last-event={host.lastEvent ?? ""} className="h-full">
      {route.window === "settings" ? (
        <SettingsShell page={route.page} onNavigate={navigate} />
      ) : route.window === "composer" ? (
        <ComposerWindow />
      ) : (
        <ConsoleShell page={route.page} onNavigate={navigate} />
      )}
    </div>
  );
}

type ComposerPanel = { kind: "sessions" } | { kind: "tasks"; runId?: string } | { kind: "terminal"; sessionId: string };

function ComposerWindow() {
  const host = useHost();
  const [panel, setPanel] = useState<ComposerPanel | null>(null);
  const [planning, setPlanning] = useState<PlanningView | null>(null);
  const [taskMessage, setTaskMessage] = useState<string | null>(null);
  const { present, phase } = usePresence(panel);
  const [visibleSession, setVisibleSession] = useState<string | null>(null);
  const [feedbackHeight, setFeedbackHeight] = useState(0);
  const [selectionVersion, setSelectionVersion] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const panelOpen = present !== null;
  const extraHeight = panelOpen ? 360 : feedbackHeight;

  useEffect(() => {
    let current = true;
    void host.adapter.surfaceLayout(extraHeight).catch((reason: unknown) => {
      if (current) setError(isAppError(reason) ? reason.message : String(reason));
    });
    return () => { current = false; };
  }, [host.adapter, extraHeight]);

  const onSessionChange = useCallback((id: string | null) => {
    setVisibleSession(id);
    setPanel((current) => {
      if (current?.kind === "terminal") return id ? { kind: "terminal", sessionId: id } : null;
      return current?.kind === "tasks" ? null : current;
    });
  }, []);
  const onPlanningChange = useCallback((value: PlanningView | null) => {
    setPlanning(value);
    if (value) { setTaskMessage(null); setPanel({ kind: "tasks" }); }
  }, []);
  const closePanels = () => {
    setPanel(null);
    document.querySelector<HTMLTextAreaElement>("[data-testid='composer-input']")?.focus();
  };

  return (
    <div className="composer-window relative flex h-full flex-col justify-end" onKeyDown={(event) => {
      if (event.key !== "Escape" || event.nativeEvent.isComposing) return;
      if ((event.target as HTMLElement).closest(".xterm")) return;
      if (panelOpen) { event.preventDefault(); closePanels(); }
    }}>
      {error && <p role="alert" className="absolute inset-x-3 top-2 z-30 rounded-lg bg-card p-2 text-xs text-error">{error}</p>}
      <div className="contents" data-panel-presence={phase} inert={phase === "closing"}>
        {present?.kind === "tasks" && visibleSession && <TaskPanel key={visibleSession} sessionId={visibleSession} initialRunId={present.runId} planning={planning} message={taskMessage} onCancelPlanning={() => {
          if (!planning) return;
          setPlanning({ ...planning, cancelling: true });
          void host.adapter.runPlanCancel(planning.requestId).catch((reason: unknown) => setTaskMessage(isAppError(reason) ? reason.message : String(reason)));
        }} onClose={closePanels} />}
        {present?.kind === "terminal" && <TerminalPanel key={present.sessionId} sessionId={present.sessionId} onClose={closePanels} />}
        {present?.kind === "sessions" && (
          <SessionSelector
            onClose={closePanels}
            onSelect={async (session) => {
              try {
                if (session.state === "active" || session.state === "ending") {
                  await host.adapter.sessionSelect(newRequestId("select"), session.id);
                  setVisibleSession(session.id);
                  setSelectionVersion((version) => version + 1);
                } else {
                  await host.adapter.openWindow("console", "runs");
                }
                closePanels();
                setError(null);
              } catch (reason) {
                setError(isAppError(reason) ? reason.message : String(reason));
              }
            }}
          />
        )}
      </div>
      <div className="relative h-[72px] shrink-0">
        <ComposerBar
          sessionsOpen={panel?.kind === "sessions"}
          panelOpen={panelOpen}
          selectionVersion={selectionVersion}
          onFeedbackHeight={setFeedbackHeight}
          onSessionChange={onSessionChange}
          onPlanningChange={onPlanningChange}
          onTaskMessage={(message) => { setTaskMessage(message); setPanel({ kind: "tasks" }); }}
          onOpenTasks={(runId) => { setTaskMessage(null); setPanel({ kind: "tasks", runId }); }}
          onOpenSessions={() => panel?.kind === "sessions" ? closePanels() : setPanel({ kind: "sessions" })}
          onOpenTerminal={() => { if (visibleSession) setPanel({ kind: "terminal", sessionId: visibleSession }); }}
        />
      </div>
    </div>
  );
}

export function App({ adapter }: { adapter: HostAdapter }) {
  return (
    <HostProvider adapter={adapter}>
      <HostGate />
    </HostProvider>
  );
}
