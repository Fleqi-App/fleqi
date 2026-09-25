import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type {
  AppBootstrap,
  AppError,
  AppEvent,
  ContextSnapshot,
  DirectoryPickResult,
  Permission,
  PermissionSnapshot,
  SettingsPatch,
  SettingsSnapshot,
} from "@fleqi/contracts";
import { newRequestId, toAppError, type HostAdapter, type WindowRole } from "../adapters/host";
import { applyAppearance } from "../theme";

/**
 * 宿主快照存储：UI 只消费宿主返回的快照与结果；事件到达后主动重拉，
 * 过期回执不覆盖新状态（按 attempt 编号）。
 */
export type HostPhase = "loading" | "ready" | "failed";

export interface HostStore {
  adapter: HostAdapter;
  phase: HostPhase;
  attempt: number;
  bootstrap: AppBootstrap | null;
  error: AppError | null;
  /** 最近一次收到的宿主事件（可观测性；测试用）。 */
  lastEvent: string | null;
  eventVersion: number;
  reload: () => void;
  updateSettings: (patch: SettingsPatch, expectedRevision: string, requestId?: string) => Promise<SettingsSnapshot>;
  checkPermissions: () => Promise<PermissionSnapshot>;
  requestPermission: (permission: Permission) => Promise<void>;
  openSystemSettings: (permission: Permission) => Promise<void>;
  refreshContext: () => Promise<ContextSnapshot>;
  pickDirectory: () => Promise<DirectoryPickResult>;
  openWindow: (role: WindowRole) => Promise<void>;
  quit: () => Promise<void>;
}

const HostContext = createContext<HostStore | null>(null);

export function HostProvider({ adapter, children }: { adapter: HostAdapter; children: ReactNode }) {
  const [phase, setPhase] = useState<HostPhase>("loading");
  const [attempt, setAttempt] = useState(0);
  const [bootstrap, setBootstrap] = useState<AppBootstrap | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [lastEvent, setLastEvent] = useState<string | null>(null);
  const attemptRef = useRef(0);
  const [eventVersion, setEventVersion] = useState(0);

  const load = useCallback(
    (showLoading: boolean) => {
      const current = attemptRef.current + 1;
      attemptRef.current = current;
      if (showLoading) {
        setPhase("loading");
        setAttempt(current);
      }
      adapter.bootstrap().then(
        (snapshot) => {
          if (attemptRef.current !== current) return;
          setBootstrap(snapshot);
          setError(null);
          setPhase("ready");
          setAttempt(current);
        },
        (reason: unknown) => {
          if (attemptRef.current !== current) return;
          setError(toAppError(reason, "读取宿主状态失败"));
          setPhase("failed");
          setAttempt(current);
        },
      );
    },
    [adapter],
  );

  useEffect(() => {
    load(true);
    let timer: ReturnType<typeof setTimeout> | null = null;
    const unsubscribe = adapter.subscribe((event: AppEvent) => {
      // 版本变化后合并重拉一次完整快照，不展示加载态覆盖已有内容。
      setLastEvent(event.kind);
      setEventVersion((version) => version + 1);
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => load(false), 60);
    });
    return () => {
      attemptRef.current += 1;
      if (timer) clearTimeout(timer);
      unsubscribe();
    };
  }, [adapter, load]);

  useEffect(() => {
    const apply = () => {
      if (bootstrap) {
        document.documentElement.dataset.platform = bootstrap.buildInfo.targetOs;
        applyAppearance({
          theme: bootstrap.settings.theme,
          transparency: bootstrap.settings.transparency,
          motionMode: bootstrap.settings.motionMode,
        });
      }
    };
    apply();
    const preference = window.matchMedia?.("(prefers-color-scheme: dark)");
    preference?.addEventListener?.("change", apply);
    return () => preference?.removeEventListener?.("change", apply);
  }, [bootstrap]);

  const store = useMemo<HostStore>(
    () => ({
      adapter,
      phase,
      attempt,
      bootstrap,
      error,
      lastEvent,
      eventVersion,
      reload: () => load(true),
      async updateSettings(patch, expectedRevision, requestId = newRequestId("settings")) {
        const snapshot = await adapter.updateSettings({ requestId, expectedRevision, patch });
        setBootstrap((current) => (current ? { ...current, settings: snapshot } : current));
        return snapshot;
      },
      async checkPermissions() {
        const snapshot = await adapter.permissionsCheck();
        setBootstrap((current) => (current ? { ...current, permissions: snapshot } : current));
        return snapshot;
      },
      async requestPermission(permission) {
        await adapter.permissionsRequest(newRequestId("perm"), permission);
      },
      openSystemSettings: (permission) => adapter.permissionsOpenSettings(permission),
      async refreshContext() {
        const snapshot = await adapter.contextRefresh();
        setBootstrap((current) => (current ? { ...current, context: snapshot } : current));
        return snapshot;
      },
      async pickDirectory() {
        const result = await adapter.contextPickDirectory(newRequestId("pick"));
        if (result.kind === "selected") {
          const snapshot = result.snapshot;
          setBootstrap((current) => (current ? { ...current, context: snapshot } : current));
        }
        return result;
      },
      openWindow: (role) => adapter.openWindow(role),
      quit: () => adapter.quit(),
    }),
    [adapter, phase, attempt, bootstrap, error, lastEvent, eventVersion, load],
  );

  return <HostContext.Provider value={store}>{children}</HostContext.Provider>;
}

export function useHost(): HostStore {
  const store = useContext(HostContext);
  if (!store) throw new Error("useHost 必须在 HostProvider 内使用");
  return store;
}
