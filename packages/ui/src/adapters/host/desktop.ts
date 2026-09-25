import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type {
  AppBootstrap,
  AppEvent,
  BuildInfo,
  ContextSnapshot,
  DiagnosticsSnapshot,
  DirectoryPickResult,
  Favorite,
  InstallProgress,
  Permission,
  PermissionOperation,
  PermissionSnapshot,
  PlanOutcome,
  ProviderView,
  QueuedLine,
  Rule,
  RunRecord,
  Session,
  SettingsSnapshot,
  SettingsUpdateRequest,
  TerminalSnapshot,
  ToolEntry,
} from "@fleqi/contracts";
import {
  toAppError,
  type CatalogEntry,
  type HostAdapter,
  type SessionList,
  type SurfaceView,
  type TerminalStreamEvent,
  type WindowRole,
} from "./types";

/** 宿主事件名（fleqi_application::dto::AppEvent::name）：Tauri 事件名不允许 `.`，合同的 `settings.changed` 传输为 `settings:changed`。 */
export const EVENT_NAMES = [
  "settings:changed",
  "permissions:changed",
  "platform:changed",
  "context:changed",
  "host:changed",
  "session:changed",
  "surface:changed",
  "terminal:changed",
  "run:changed",
  "rules:changed",
  "favorites:changed",
  "tools:changed",
  "providers:changed",
] as const;

export function hasTauriBridge(target: Window = window): boolean {
  return "__TAURI_INTERNALS__" in target;
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw toAppError(error, `宿主命令 ${command} 失败`);
  }
}

export function createDesktopAdapter(): HostAdapter {
  // 原生宿主：窗口无边框、透明，红绿灯/圆角由 CSS 按 data-chrome 启用；
  // 浏览器预览不带该标记，保持常规不透明背景。
  document.documentElement.dataset.chrome = "native";
  return {
    kind: "desktop",
    getBuildInfo: () => call<BuildInfo>("app_build_info"),
    bootstrap: () => call<AppBootstrap>("app_bootstrap"),
    diagnostics: () => call<DiagnosticsSnapshot>("diagnostics_get"),
    appUpdateStatus: () => call("app_update_status"),
    appUpdateCheck: () => call("app_update_check"),
    appUpdateInstall: () => call("app_update_install"),
    openWindow: (role: WindowRole, page?: string) => call<void>("app_open_window", { role, page: page ?? null }),
    quit: () => call<void>("app_quit"),
    windowControl: async (action) => {
      const current = getCurrentWindow();
      if (action === "close") await current.close();
      else if (action === "minimize") await current.minimize();
      else await current.toggleMaximize();
    },
    updateSettings: (request: SettingsUpdateRequest) => call<SettingsSnapshot>("settings_update", { request }),
    permissionsGet: () => call<PermissionSnapshot>("permissions_get"),
    permissionsCheck: () => call<PermissionSnapshot>("permissions_check"),
    permissionsRequest: (requestId: string, permission: Permission) =>
      call<PermissionOperation>("permissions_request", { requestId, permission }),
    permissionsOpenSettings: (permission: Permission) => call<void>("permissions_open_settings", { permission }),
    contextGet: (contextId?: string) => call<ContextSnapshot>("context_get", { contextId: contextId ?? null }),
    contextRefresh: () => call<ContextSnapshot>("context_refresh"),
    contextPickDirectory: (requestId: string) => call<DirectoryPickResult>("context_pick_directory", { requestId }),
    surfaceGet: () => call<SurfaceView>("surface_get"),
    surfaceLayout: (extraHeight) => call<void>("surface_layout", { extraHeight }),
    surfaceShow: () => call<SurfaceView>("surface_show"),
    surfaceHide: () => call<SurfaceView>("surface_hide"),
    sessionEntries: (sessionId) => call<import("@fleqi/contracts").ConversationEntry[]>("session_entries", { sessionId, limit: 100 }),
    sessionList: () => call<SessionList>("session_list", { offset: 0, limit: 50 }),
    sessionCreate: (requestId) => call<Session>("session_create", { requestId }),
    sessionSelect: (requestId, sessionId) => call<Session>("session_select", { requestId, sessionId }),
    sessionEnd: (requestId, sessionId) => call<Session>("session_end", { requestId, sessionId }),
    sessionEndAll: (requestId) => call<number>("session_end_all", { requestId }),
    sessionPin: (sessionId, pinned, expectedRevision) => call<Session>("session_pin", { sessionId, pinned, expectedRevision }),
    sessionDelete: (requestId, sessionId, expectedRevision) => call<void>("session_delete", { requestId, sessionId, expectedRevision }),
    sessionContinue: (requestId, historySessionId) => call<Session>("session_continue", { requestId, historySessionId }),
    terminalOpen: (sessionId, cols, rows) =>
      call<{ terminalId: string; currentDirectory: string }>("terminal_open", {
        sessionId,
        cols: cols ?? 100,
        rows: rows ?? 30,
      }),
    terminalSnapshot: (sessionId) => call<TerminalSnapshot>("terminal_snapshot", { sessionId }),
    terminalInput: (sessionId, lease, input) => call<void>("terminal_input", { sessionId, lease, input: Array.from(input) }),
    terminalReleaseLease: (sessionId, lease) => call<void>("terminal_release_lease", { sessionId, lease }),
    terminalAcquireLease: (sessionId, owner) => call<string>("terminal_acquire_lease", { sessionId, owner }),
    terminalResize: (sessionId, cols, rows) => call<void>("terminal_resize", { sessionId, cols, rows }),
    terminalSubmitLine: (requestId, sessionId, line, contextRevision, targetDisplay) =>
      call<"sent" | "queued">("terminal_submit_line", {
        requestId,
        sessionId,
        line,
        contextRevision,
        targetDisplay,
      }),
    terminalCancelQueued: (sessionId) => call<QueuedLine | null>("terminal_cancel_queued", { sessionId }),
    terminalWithdrawnLine: (sessionId) => call<QueuedLine | null>("terminal_withdrawn_line", { sessionId }),
    async terminalSubscribe(sessionId, cursor, onEvent) {
      const { Channel } = await import("@tauri-apps/api/core");
      const channel = new Channel<TerminalStreamEvent>();
      channel.onmessage = onEvent;
      const subscription = await call<number>("terminal_subscribe", { sessionId, cursor, onEvent: channel });
      return () => call<void>("terminal_unsubscribe", { sessionId, subscription });
    },
    terminalAck: (sessionId, cursor) => call<void>("terminal_ack", { sessionId, cursor }),
    hotkeyCommit: (requestId, accelerator) =>
      call<{ registered: string | null; message: string | null }>("hotkey_commit", { requestId, accelerator }),
    hotkeyClear: (requestId) => call<{ registered: string | null; message: string | null }>("hotkey_clear", { requestId }),
    // ---- M3：Run / 模型 / 工具 / 规则收藏历史 ----
    runList: (sessionId) => call<RunRecord[]>("run_list", { sessionId }),
    runPlanGet: (runId) => call<import("@fleqi/contracts").ExecutionPlan>("run_plan_get", { runId }),
    runGet: (runId) => call<RunRecord>("run_get", { runId }),
    runApprove: (requestId, runId, planRevision) => call<RunRecord>("run_approve", { requestId, runId, planRevision }),
    runCancel: (runId) => call<RunRecord>("run_cancel", { runId }),
    runRetry: (requestId, runId) => call<RunRecord>("run_retry", { requestId, runId }),
    runPlanSubmit: (requestId, sessionId, contextId, prompt) =>
      call<PlanOutcome>("run_plan_submit", { requestId, sessionId, contextId, prompt }),
    runPlanCancel: (requestId) => call<void>("run_plan_cancel", { requestId }),
    providerList: () => call<ProviderView[]>("provider_list"),
    providerSave: (request) => call<ProviderView>("provider_save", { request }),
    providerDelete: (providerId) => call<void>("provider_delete", { providerId }),
    providerProbe: (baseUrl, apiKey, timeoutMs) =>
      call<{ ok: boolean; models: string[]; error: string | null }>("provider_probe", {
        baseUrl,
        apiKey,
        timeoutMs: timeoutMs ?? null,
      }),
    toolsList: () => call<ToolEntry[]>("tools_list"),
    toolsPrepare: () => call("tools_prepare"),
    toolsPrepareStatus: () => call("tools_prepare_status"),
    toolsPrepareCancel: () => call("tools_prepare_cancel"),
    toolsInstall: (requestId, toolId) => call<ToolEntry>("tools_install", { requestId, toolId }),
    toolsInstallStatus: (requestId) => call<InstallProgress | null>("tools_install_status", { requestId }),
    toolsInstallCancel: (requestId) => call<boolean>("tools_install_cancel", { requestId }),
    toolsRemove: (toolId) => call<void>("tools_remove", { toolId }),
    capabilityForm: (capabilityId, contextId) => call<import("@fleqi/contracts").CapabilityForm>("capability_form", { capabilityId, contextId: contextId ?? null }),
    capabilitySubmit: (requestId, sessionId, capabilityId, contextId, parameters) => call<RunRecord>("capability_submit", { requestId, sessionId, capabilityId, contextId, parameters }),
    catalogQuery: () => call<CatalogEntry[]>("catalog_query"),
    rulesCreate: (name, content, scope) => call<Rule>("rules_create", { name, content, scope: scope ?? null }),
    rulesList: () => call<Rule[]>("rules_list"),
    rulesUpdate: (ruleId, patch) => call<Rule>("rules_update", { ruleId, ...patch }),
    rulesDelete: (ruleId) => call<void>("rules_delete", { ruleId }),
    favoritesCreate: (name, content, kind) => call<Favorite>("favorites_create", { name, content, kind }),
    favoritesList: () => call<Favorite[]>("favorites_list"),
    favoritesUpdate: (favoriteId, patch) => call<Favorite>("favorites_update", { favoriteId, ...patch }),
    favoritesDelete: (favoriteId) => call<void>("favorites_delete", { favoriteId }),
    historyList: () => call<string[]>("history_list"),
    historyAppend: (entry) => call<void>("history_append", { entry }),
    historyClear: () => call<void>("history_clear"),
    subscribe(handler) {
      const unlisteners: Array<() => void> = [];
      let cancelled = false;
      for (const name of EVENT_NAMES) {
        listen<AppEvent>(name, (event) => handler(event.payload)).then(
          (unlisten) => {
            if (cancelled) unlisten();
            else unlisteners.push(unlisten);
          },
          (error: unknown) => {
            // 订阅失败必须可见：否则窗口会静默停留在旧版本。
            console.error(`订阅宿主事件 ${name} 失败`, error);
          },
        );
      }
      return () => {
        cancelled = true;
        for (const unlisten of unlisteners) unlisten();
      };
    },
  };
}
