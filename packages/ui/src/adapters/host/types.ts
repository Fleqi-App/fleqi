import type {
  AppBootstrap,
  AppError,
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
  ProviderSaveRequest,
  ProviderView,
  QueuedLine,
  Rule,
  RuleScope,
  RunRecord,
  Session,
  SettingsSnapshot,
  SettingsUpdateRequest,
  TerminalSnapshot,
  ToolEntry,
  ToolPreparation,
} from "@fleqi/contracts";

/**
 * 宿主适配器：UI 唯一的 IPC 出口（architecture.md §2、§12.2）。
 * - desktop：经 Tauri IPC 调用真实命令并订阅低频事件；
 * - preview：浏览器预览用固定数据，不接真实进程、网络或文件写入。
 */
export type HostKind = "desktop" | "preview";
export type WindowRole = "console" | "settings";

export interface SurfaceView {
  visibility: "visible" | "temporarilyHidden" | "userHidden";
  visibleSessionId: string | null;
  autoShowSuppressed: boolean;
  barEnabled: boolean;
  activation: string;
}

export interface SessionList {
  active: import("@fleqi/contracts").Session[];
  history: import("@fleqi/contracts").Session[];
}

export interface TerminalStreamEvent {
  kind: "output" | "promptReady" | "exited";
  bytes?: number[];
  cursor?: number;
  cwd?: string;
}

export interface HostAdapter {
  appUpdateStatus(): Promise<import("@fleqi/contracts").AppUpdateStatus>;
  appUpdateCheck(): Promise<import("@fleqi/contracts").AppUpdateStatus>;
  appUpdateInstall(): Promise<void>;
  readonly kind: HostKind;
  getBuildInfo(): Promise<BuildInfo>;
  bootstrap(): Promise<AppBootstrap>;
  diagnostics(): Promise<DiagnosticsSnapshot>;
  /** 打开窗口；`page` 在白名单内时直接落到对应页面（引导跳转）。 */
  openWindow(role: WindowRole, page?: string): Promise<void>;
  quit(): Promise<void>;
  /** 自绘红绿灯的窗口动作（ui-design.md §2.1）：仅宿主窗口有效，预览为空操作。 */
  windowControl(action: "close" | "minimize" | "toggleMaximize"): Promise<void>;
  updateSettings(request: SettingsUpdateRequest): Promise<SettingsSnapshot>;
  permissionsGet(): Promise<PermissionSnapshot>;
  permissionsCheck(): Promise<PermissionSnapshot>;
  permissionsRequest(requestId: string, permission: Permission): Promise<PermissionOperation>;
  permissionsOpenSettings(permission: Permission): Promise<void>;
  contextGet(contextId?: string): Promise<ContextSnapshot>;
  contextRefresh(): Promise<ContextSnapshot>;
  contextPickDirectory(requestId: string): Promise<DirectoryPickResult>;
  // ---- M2：输入条 / 会话 / 终端 ----
  surfaceShow(): Promise<SurfaceView>;
  /** 只读快照；页面挂载/事件重拉不能隐式唤起或创建会话。 */
  surfaceGet(): Promise<SurfaceView>;
  /** 为锚定面板分配窗口内空间；宿主限制尺寸并保持主条位置。 */
  surfaceLayout(extraHeight: number): Promise<void>;
  surfaceHide(): Promise<SurfaceView>;
  sessionList(): Promise<SessionList>;
  sessionEntries(sessionId: string): Promise<import("@fleqi/contracts").ConversationEntry[]>;
  /** 显式创建新会话（不改变可见性；随后用 sessionSelect 选择）。 */
  sessionCreate(requestId: string): Promise<Session>;
  sessionSelect(requestId: string, sessionId: string): Promise<Session>;
  sessionEnd(requestId: string, sessionId: string): Promise<Session>;
  sessionEndAll(requestId: string): Promise<number>;
  sessionPin(sessionId: string, pinned: boolean, expectedRevision: string): Promise<Session>;
  sessionDelete(requestId: string, sessionId: string, expectedRevision: string): Promise<void>;
  /** 从历史（已结束）会话继续：创建关联新会话并继承当前上下文。 */
  sessionContinue(requestId: string, historySessionId: string): Promise<Session>;
  terminalOpen(sessionId: string, cols?: number, rows?: number): Promise<{ terminalId: string; currentDirectory: string }>;
  terminalSnapshot(sessionId: string): Promise<TerminalSnapshot>;
  terminalInput(sessionId: string, lease: string | null, input: Uint8Array): Promise<void>;
  terminalAcquireLease(sessionId: string, owner: string): Promise<string>;
  terminalReleaseLease(sessionId: string, lease: string): Promise<void>;
  terminalResize(sessionId: string, cols: number, rows: number): Promise<void>;
  terminalSubmitLine(
    requestId: string,
    sessionId: string,
    line: string,
    contextRevision: string,
    targetDisplay: string,
  ): Promise<"sent" | "queued">;
  terminalCancelQueued(sessionId: string): Promise<QueuedLine | null>;
  terminalWithdrawnLine(sessionId: string): Promise<QueuedLine | null>;
  /** 订阅终端输出流；onEvent 收到线格式事件。返回显式取消订阅函数。 */
  terminalSubscribe(sessionId: string, cursor: number, onEvent: (event: TerminalStreamEvent) => void): Promise<() => Promise<void>>;
  /** 消费位点回执：宿主记录 UI 已消费的流位置（流控/诊断）。 */
  terminalAck(sessionId: string, cursor: number): Promise<void>;
  hotkeyCommit(requestId: string, accelerator: string): Promise<{ registered: string | null; message: string | null }>;
  hotkeyClear(requestId: string): Promise<{ registered: string | null; message: string | null }>;
  /** 订阅宿主事件；返回取消订阅函数。 */
  subscribe(handler: (event: AppEvent) => void): () => void;
  // ---- M3：Run / 模型 / 工具 / 规则收藏历史 ----
  runList(sessionId: string): Promise<RunRecord[]>;
  runGet(runId: string): Promise<RunRecord>;
  runPlanGet(runId: string): Promise<import("@fleqi/contracts").ExecutionPlan>;
  runApprove(requestId: string, runId: string, planRevision: string): Promise<RunRecord>;
  runCancel(runId: string): Promise<RunRecord>;
  runRetry(requestId: string, runId: string): Promise<RunRecord>;
  runPlanSubmit(requestId: string, sessionId: string, contextId: string, prompt: string): Promise<PlanOutcome>;
  /** 取消在途规划（模型块间生效）；无在途请求报 not_found。 */
  runPlanCancel(requestId: string): Promise<void>;
  providerList(): Promise<ProviderView[]>;
  providerSave(request: ProviderSaveRequest): Promise<ProviderView>;
  providerDelete(providerId: string): Promise<void>;
  providerProbe(baseUrl: string, apiKey: string | null, timeoutMs?: number): Promise<{ ok: boolean; models: string[]; error: string | null }>;
  toolsList(): Promise<ToolEntry[]>;
  toolsPrepare(): Promise<ToolPreparation>;
  toolsPrepareStatus(): Promise<ToolPreparation>;
  toolsPrepareCancel(): Promise<void>;
  toolsInstall(requestId: string, toolId: string): Promise<ToolEntry>;
  /** 轮询在途安装进度；未知 requestId 返回 null。 */
  toolsInstallStatus(requestId: string): Promise<InstallProgress | null>;
  /** 取消在途安装（下载分块间生效）；无在途任务返回 false。 */
  toolsInstallCancel(requestId: string): Promise<boolean>;
  toolsRemove(toolId: string): Promise<void>;
  catalogQuery(): Promise<CatalogEntry[]>;
  capabilityForm(capabilityId: string, contextId?: string): Promise<import("@fleqi/contracts").CapabilityForm>;
  capabilitySubmit(requestId: string, sessionId: string, capabilityId: string, contextId: string, parameters: Record<string, string>): Promise<RunRecord>;
  rulesCreate(name: string, content: string, scope?: RuleScope): Promise<Rule>;
  rulesList(): Promise<Rule[]>;
  rulesUpdate(ruleId: string, patch: { name?: string; enabled?: boolean; content?: string }): Promise<Rule>;
  rulesDelete(ruleId: string): Promise<void>;
  favoritesCreate(name: string, content: string, kind: string): Promise<Favorite>;
  favoritesList(): Promise<Favorite[]>;
  favoritesUpdate(favoriteId: string, patch: { name?: string; tags?: string[] }): Promise<Favorite>;
  favoritesDelete(favoriteId: string): Promise<void>;
  historyList(): Promise<string[]>;
  historyAppend(entry: string): Promise<void>;
  historyClear(): Promise<void>;
}

/** catalog_query 条目（宿主 catalog.rs 的展示 DTO；参数化能力数据）。 */
export interface CatalogEntry {
  availability?: import("@fleqi/contracts").CapabilityState;
  unavailableReason?: string | null;
  id: string;
  category: string;
  title: string;
  description: string;
  inputs: string;
  dependencies: string[];
}

export function isAppError(value: unknown): value is AppError {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as AppError).code === "string" &&
    typeof (value as AppError).message === "string" &&
    typeof (value as AppError).retryable === "boolean"
  );
}

/** 把未知错误（IPC 拒绝、桥缺失、异常）归一为 AppError；不伪造成功。 */
export function toAppError(value: unknown, fallbackMessage: string): AppError {
  if (isAppError(value)) return value;
  const detail = value instanceof Error ? value.message : typeof value === "string" ? value : "";
  return {
    code: "forbidden",
    message: detail ? `${fallbackMessage}：${detail}` : fallbackMessage,
    retryable: true,
  };
}

/** 客户端生成的 requestId：时间 + UUID，跨窗口不冲突（非加密用途）。 */
export function newRequestId(prefix: string): string {
  return `${prefix}-${Date.now().toString(36)}-${crypto.randomUUID()}`;
}
