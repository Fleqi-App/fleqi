import type {
  AppBootstrap,
  AppError,
  AppEvent,
  BuildInfo,
  ContextSnapshot,
  DiagnosticsSnapshot,
  Permission,
  PermissionOperation,
  PermissionRecord,
  PermissionSnapshot,
  QueuedLine,
  Settings,
  SettingsSnapshot,
  SettingsUpdateRequest,
  TerminalSnapshot,
} from "@fleqi/contracts";
import type { HostAdapter, SessionList, SurfaceView, TerminalStreamEvent, WindowRole, CatalogEntry } from "./types";

/**
 * 浏览器预览适配器：固定数据 + 内存状态机，持续标识为预览；不代表原生证据。
 * URL 参数：`?preview=fail` 所有调用失败；`?preview=degraded` 存储降级；`?preview=pick` 目录选择返回选中。
 */
export const PREVIEW_BUILD_INFO: BuildInfo = {
  productName: "Fleqi",
  version: "0.0.2",
  bundleIdentifier: "app.fleqi.desktop",
  stage: "BETA",
  targetOs: "browser-preview",
  targetArch: "browser-preview",
  buildProfile: "preview",
  minimumMacosVersion: "14.0",
};

export const PREVIEW_FAILURE: AppError = {
  code: "forbidden",
  message: "预览模式：宿主桥不可用（模拟）",
  retryable: true,
};

/** 需求 §2.1 默认值（与 Rust Settings::default 一致）。 */
export const PREVIEW_DEFAULT_SETTINGS: Settings = {
  barEnabled: true,
  activation: "manual",
  hotkey: null,
  hideBehavior: "keepAll",
  aiPolicy: "readOnlyAutoConfirmChanges",
  launchAtLogin: false,
  theme: "dark",
  bubbleSeconds: 4.8,
  inlineSuggestionsEnabled: true,
  inlineSuggestionsLimit: 3,
  transparency: true,
  motionMode: "system",
  providers: [],
  defaultModel: null,
  summaryModel: { kind: "default" },
  terminalFontSize: 13,
  outputLocation: { kind: "besideSource" },
  nameConflict: "uniqueName",
  conversionSourceHandling: "keep",
};

const EDITABLE_FIELDS = new Set([
  "theme",
  "transparency",
  "motionMode",
  "barEnabled",
  "activation",
  "hotkey",
  "hideBehavior",
  "launchAtLogin",
  "bubbleSeconds",
  "inlineSuggestionsEnabled",
  "inlineSuggestionsLimit",
  "aiPolicy",
  "defaultModel",
  "nameConflict",
  "conversionSourceHandling",
  "outputLocation",
  "terminalFontSize",
]);

function record(permission: Permission, status: PermissionRecord["status"]): PermissionRecord {
  return {
    permission,
    status,
    procedure: "passive",
    checkedAt: status === "unknown" ? null : "2026-09-17T12:00:00Z",
    error: null,
    recovery: status === "allowed" ? "none" : status === "needsConsent" ? "requestExplicitly" : status === "denied" ? "openSystemSettings" : "recheck",
    revoked: false,
  };
}

export interface PreviewOptions {
  fail?: boolean;
  degraded?: boolean;
  pick?: boolean;
  delayMs?: number;
}

export function createPreviewAdapter(options: PreviewOptions = {}): HostAdapter {
  const { fail = false, degraded = false, pick = false, delayMs = 120 } = options;
  let revision = 0;
  let settings: Settings = { ...PREVIEW_DEFAULT_SETTINGS };
  let permissionRevision = 0;
  let permissions: PermissionRecord[] = [record("finderAutomation", "unknown"), record("accessibility", "unknown")];
  let contextCounter = 0;
  let latestContext: ContextSnapshot | null = null;
  const receipts = new Map<string, { fingerprint: string; snapshot: SettingsSnapshot }>();
  const handlers = new Set<(event: AppEvent) => void>();

  const emit = (event: AppEvent) => {
    for (const handler of handlers) handler(event);
  };
  const wait = async () => {
    if (delayMs > 0) await new Promise((resolve) => setTimeout(resolve, delayMs));
    if (fail) throw PREVIEW_FAILURE;
  };
  const settingsSnapshot = (): SettingsSnapshot => ({ revision: String(revision), persisted: !degraded, ...settings });
  const permissionSnapshot = (): PermissionSnapshot => ({ revision: String(permissionRevision), records: permissions.map((r) => ({ ...r })) });
  const makeContext = (source: "finder" | "picker", directory: string | null): ContextSnapshot => {
    contextCounter += 1;
    const snapshot: ContextSnapshot = {
      id: `ctx-${contextCounter}`,
      revision: String(contextCounter),
      source,
      sourceWindowId: source === "finder" ? 7 : null,
      directoryRef: directory ? { id: `path-${contextCounter}`, displayPath: directory, kind: "directory" } : null,
      selectedItems: directory
        ? [
            { id: `path-${contextCounter}-a`, displayPath: `${directory}/预览文件 A.txt`, kind: "file" },
            { id: `path-${contextCounter}-b`, displayPath: `${directory}/预览子目录`, kind: "directory" },
          ]
        : [],
      viewKind: "physical",
      capturedAt: new Date().toISOString(),
      availability: directory ? { kind: "available" } : { kind: "permissionRequired" },
      selectionComplete: true,
    };
    latestContext = snapshot;
    emit({ kind: "contextChanged", contextId: snapshot.id, revision: snapshot.revision });
    return snapshot;
  };

  const bootstrap = (): AppBootstrap => ({
    buildInfo: { ...PREVIEW_BUILD_INFO },
    hostState: degraded ? "degraded" : "ready",
    windowRole: window.location.hash.startsWith("#/settings") ? "settings" : "console",
    storage: {
      state: degraded ? "degraded" : "ready",
      dataDirDisplay: "~/Library/Application Support/app.fleqi.desktop（预览）",
      schemaVersion: degraded ? 0 : 1,
      message: degraded ? "预览：模拟数据库损坏（database disk image is malformed）" : null,
      lastBackupDisplay: null,
    },
    settings: settingsSnapshot(),
    permissions: permissionSnapshot(),
    platform: {
      revision: String(permissionRevision),
      items: [
        { id: "finderContext", state: "temporarilyUnavailable", reason: "尚未检查", recovery: "运行权限自检" },
        { id: "accessibilityGeometry", state: "temporarilyUnavailable", reason: "尚未检查", recovery: "运行权限自检" },
        { id: "directoryPicker", state: "supported", reason: null, recovery: null },
        { id: "credentialStore", state: "supported", reason: null, recovery: null },
      ],
    },
    context: latestContext,
  });

  const surfaceState = {
    visible: false,
    sessionId: null as string | null,
    hotkey: null as string | null,
    sessions: [] as import("@fleqi/contracts").Session[],
    suppressed: false,
  };
  const terminals = new Map<string, { lines: string[]; cursor: number; queued: import("@fleqi/contracts").QueuedLine | null }>();
  // M3 预览状态：内存数据，重启即失；语义与真实宿主一致（掩码/拒绝/状态机）。
  const m3 = {
    providers: [] as import("@fleqi/contracts").ProviderView[],
    runs: [] as import("@fleqi/contracts").RunRecord[],
    planCounter: 1,
    rules: [] as import("@fleqi/contracts").Rule[],
    favorites: [] as import("@fleqi/contracts").Favorite[],
    history: ["printf 预览历史示例"] as string[],
    tools: [
      {
        manifest: {
          id: "git",
          version: "system",
          platform: "macos",
          arch: "any",
          executable: "git",
          source: { kind: "system" },
          license: null,
          capabilities: ["cap.git.*"],
          detectionArgs: ["--version"],
        },
        status: { kind: "available", version: "git version 2.50（预览）", owner: "system", path: "/usr/bin/git" },
        installed: null,
      },
      {
        manifest: {
          id: "tesseract",
          version: "5.5.0",
          platform: "macos",
          arch: "aarch64",
          executable: "bin/tesseract",
          source: { kind: "managed", url: "https://example.com/tesseract.zip", sha256: "0".repeat(64) },
          license: "Apache-2.0",
          capabilities: ["cap.ocr.text"],
          detectionArgs: ["--version"],
        },
        status: { kind: "notInstalled" },
        installed: null,
      },
    ] as import("@fleqi/contracts").ToolEntry[],
    catalog: [
      "文件|file|读取目录结构|返回文件与子目录清单|路径|无",
      "文件|file|复制文件|复制到目标并保留原件|源、目标|无",
      "压缩|zip|打包 ZIP|把多个文件压缩为归档|文件列表|无",
      "压缩|zip|解压 ZIP|安全解压到独立目录|归档路径|无",
      "图片|image|格式转换|PNG/JPEG/WebP 六方向转换|源、格式|无",
      "图片|image|缩放|按比例缩小（不放大）|源、宽度|无",
      "媒体|media|音频转换|经 ffmpeg 转换音频格式|源、格式|ffmpeg",
      "PDF|pdf|提取 PDF 页|按页码提取到新文件|PDF 文件|无",
      "文本|text|提取 DOCX 文本|读取文档正文（不执行宏）|文档路径|无",
      "系统|system|系统信息|CPU/内存/显示器/电池事实|无|无",
    ]
      .map((row, index) => ({ parts: row.split("|"), index }))
      .map(({ parts, index }) => {
        const [, category, title, description, inputs, dependencies] = parts;
        return {
          id: `cap.preview-${index}`,
          category,
          title,
          description,
          inputs,
          dependencies: dependencies === "无" ? [] : [dependencies],
        };
      }) as CatalogEntry[],
  };
  const surfaceView = (): SurfaceView => ({
    visibility: surfaceState.visible ? "visible" : "userHidden",
    visibleSessionId: surfaceState.sessionId,
    autoShowSuppressed: surfaceState.suppressed,
    barEnabled: settings.barEnabled,
    activation: settings.activation,
  });
  return {
    kind: "preview",
    async appUpdateStatus() { return { phase: "idle", version: null, notes: null, downloadedBytes: 0, totalBytes: null, error: null }; },
    async appUpdateCheck() { throw new Error("请在桌面应用中检查更新"); },
    async appUpdateInstall() { throw new Error("请在桌面应用中安装更新"); },
    async getBuildInfo() {
      await wait();
      return { ...PREVIEW_BUILD_INFO };
    },
    async bootstrap() {
      await wait();
      return bootstrap();
    },
    async diagnostics(): Promise<DiagnosticsSnapshot> {
      await wait();
      const b = bootstrap();
      return {
        buildInfo: b.buildInfo,
        hostState: b.hostState,
        generation: "1",
        storage: b.storage,
        logDirDisplay: "~/Library/Application Support/app.fleqi.desktop/logs（预览）",
        appliedMigrations: degraded ? [] : ["1:m1_settings_and_receipts"],
        permissions: b.permissions,
        platform: b.platform,
        credentialStore: { available: true, namespace: "app.fleqi.desktop（预览）", message: null },
      };
    },
    async openWindow(role: WindowRole, page?: string) {
      await wait();
      window.location.hash = page ? `#/${role}/${page}` : `#/${role}`;
    },
    async quit() {
      await wait();
    },
    async windowControl() {
      // 浏览器预览没有宿主窗口：窗口条在预览下不渲染，此处保持空操作。
      await wait();
    },
    // ---- M2 预览实现（内存状态机；关键分支与真实宿主语义一致） ----
    async surfaceGet() {
      await wait();
      return surfaceView();
    },
    async surfaceLayout() {},
    async surfaceShow(): Promise<SurfaceView> {
      await wait();
      if (settings.activation === "manual" && !surfaceState.hotkey) {
        throw { code: "unavailable", message: "manual 模式需要先注册有效快捷键", retryable: true } satisfies AppError;
      }
      surfaceState.visible = true;
      surfaceState.suppressed = false;
      if (!surfaceState.sessionId) {
        const id = `session-preview-${surfaceState.sessions.length + 1}`;
        surfaceState.sessions.unshift({
          id,
          parentSessionId: null,
          title: "示例 文件夹",
          state: "active",
          initialDirectory: "/Users/preview/Documents/示例 文件夹",
          currentDirectory: "/Users/preview/Documents/示例 文件夹",
          targetDirectory: null,
          directorySync: "synced",
          terminalId: null,
          pinned: false,
          createdAt: new Date().toISOString(),
          lastUsedAt: new Date().toISOString(),
          endedAt: null,
          revision: "1",
        });
        surfaceState.sessionId = id;
      }
      return surfaceView();
    },
    async surfaceHide(): Promise<SurfaceView> {
      await wait();
      surfaceState.visible = false;
      surfaceState.suppressed = true;
      surfaceState.sessionId = null;
      return surfaceView();
    },
    async sessionEntries() { return []; },
    async sessionList(): Promise<SessionList> {
      await wait();
      return {
        active: surfaceState.sessions.filter((s) => s.state === "active"),
        history: surfaceState.sessions.filter((s) => s.state !== "active"),
      };
    },
    async sessionCreate(_requestId: string): Promise<import("@fleqi/contracts").Session> {
      await wait();
      const now = new Date().toISOString();
      let next = surfaceState.sessions.length + 1;
      while (surfaceState.sessions.some((s) => s.id === `session-preview-${next}`)) next += 1;
      const created: import("@fleqi/contracts").Session = {
        id: `session-preview-${next}`,
        parentSessionId: null,
        title: "示例 文件夹",
        state: "active",
        initialDirectory: "/Users/preview/Documents/示例 文件夹",
        currentDirectory: "/Users/preview/Documents/示例 文件夹",
        targetDirectory: null,
        directorySync: "synced",
        terminalId: null,
        pinned: false,
        createdAt: now,
        lastUsedAt: now,
        endedAt: null,
        revision: "1",
      };
      surfaceState.sessions.unshift(created);
      return created;
    },
    async sessionSelect(_requestId: string, sessionId: string): Promise<import("@fleqi/contracts").Session> {
      await wait();
      const session = surfaceState.sessions.find((s) => s.id === sessionId);
      if (!session) throw { code: "not_found", message: `会话 ${sessionId} 不存在`, retryable: false } satisfies AppError;
      surfaceState.sessionId = sessionId;
      surfaceState.visible = true;
      return session;
    },
    async sessionEnd(_requestId: string, sessionId: string): Promise<import("@fleqi/contracts").Session> {
      await wait();
      const session = surfaceState.sessions.find((s) => s.id === sessionId);
      if (!session) throw { code: "not_found", message: `会话 ${sessionId} 不存在`, retryable: false } satisfies AppError;
      session.state = "ended";
      if (surfaceState.sessionId === sessionId) surfaceState.sessionId = null;
      return session;
    },
    async sessionEndAll(_requestId: string): Promise<number> {
      await wait();
      const count = surfaceState.sessions.filter((s) => s.state === "active").length;
      surfaceState.sessions.forEach((s) => {
        s.state = "ended";
      });
      surfaceState.sessionId = null;
      return count;
    },
    async sessionPin(sessionId: string, pinned: boolean, _expectedRevision: string): Promise<import("@fleqi/contracts").Session> {
      await wait();
      const session = surfaceState.sessions.find((s) => s.id === sessionId);
      if (!session) throw { code: "not_found", message: `会话 ${sessionId} 不存在`, retryable: false } satisfies AppError;
      session.pinned = pinned;
      return session;
    },
    async sessionDelete(_requestId: string, sessionId: string, _expectedRevision: string): Promise<void> {
      await wait();
      const index = surfaceState.sessions.findIndex((s) => s.id === sessionId);
      const session = surfaceState.sessions[index];
      if (!session) throw { code: "not_found", message: `会话 ${sessionId} 不存在`, retryable: false } satisfies AppError;
      if (session.state === "active") {
        throw { code: "conflict", message: "会话仍在活动，请先结束", retryable: false } satisfies AppError;
      }
      surfaceState.sessions.splice(index, 1);
      if (surfaceState.sessionId === sessionId) surfaceState.sessionId = null;
    },
    async sessionContinue(_requestId: string, historySessionId: string): Promise<import("@fleqi/contracts").Session> {
      await wait();
      const source = surfaceState.sessions.find((s) => s.id === historySessionId);
      if (!source) throw { code: "not_found", message: `会话 ${historySessionId} 不存在`, retryable: false } satisfies AppError;
      if (source.state === "active") {
        throw { code: "conflict", message: "只能从已结束的会话继续", retryable: false } satisfies AppError;
      }
      const now = new Date().toISOString();
      let next = surfaceState.sessions.length + 1;
      while (surfaceState.sessions.some((s) => s.id === `session-preview-${next}`)) next += 1;
      const created: import("@fleqi/contracts").Session = {
        ...source,
        id: `session-preview-${next}`,
        parentSessionId: source.id,
        state: "active",
        terminalId: null,
        pinned: false,
        createdAt: now,
        lastUsedAt: now,
        endedAt: null,
        revision: "1",
      };
      surfaceState.sessions.push(created);
      surfaceState.sessionId = created.id;
      surfaceState.visible = true;
      return created;
    },
    async terminalOpen(sessionId: string): Promise<{ terminalId: string; currentDirectory: string }> {
      await wait();
      const session = surfaceState.sessions.find((s) => s.id === sessionId);
      const dir = session?.currentDirectory ?? "/Users/preview/Documents/示例 文件夹";
      const terminalId = `term-preview-${sessionId}`;
      if (!terminals.has(sessionId)) {
        terminals.set(sessionId, { lines: [`zsh 提示符 (${dir}) $ `], cursor: 0, queued: null });
      }
      return { terminalId, currentDirectory: dir };
    },
    async terminalSnapshot(sessionId: string): Promise<TerminalSnapshot> {
      await wait();
      const terminal = terminals.get(sessionId);
      return {
        terminalId: `term-preview-${sessionId}`,
        sessionId,
        state: "running",
        shell: "/bin/zsh（预览）",
        size: { cols: 100, rows: 30 },
        shellReadiness: "ready",
        foregroundProcess: null,
        currentDirectory: terminal?.lines[0]?.slice(0, 40) ?? "",
        pendingDirectory: null,
        directorySync: "synced",
        screen: (terminal?.lines ?? []).join("\n"),
        streamCursor: String(terminal?.cursor ?? 0),
        exitStatus: null,
        truncated: false,
      };
    },
    async terminalInput(_sessionId: string, _lease: string | null, input: Uint8Array): Promise<void> {
      await wait();
      const terminal = terminals.values().next().value;
      if (!terminal) throw { code: "not_found", message: "终端未启动", retryable: false } satisfies AppError;
      const text = new TextDecoder().decode(input);
      terminal.lines.push(text);
      terminal.cursor += input.length;
      if (text.trim() === "echo fleqi-m2-pty-ok") {
        terminal.lines.push("fleqi-m2-pty-ok\r\n");
        terminal.cursor += text.length;
      }
    },
    async terminalReleaseLease() {},
    async terminalAcquireLease(_sessionId: string, _owner: string): Promise<string> {
      await wait();
      return "lease-preview";
    },
    async terminalResize(_sessionId: string, _cols: number, _rows: number): Promise<void> {
      await wait();
    },
    async terminalSubmitLine(
      _requestId: string,
      _sessionId: string,
      _line: string,
      _contextRevision: string,
      _targetDisplay: string,
    ): Promise<"sent" | "queued"> {
      await wait();
      return "sent";
    },
    async terminalCancelQueued(_sessionId: string): Promise<QueuedLine | null> {
      await wait();
      return null;
    },
    async terminalWithdrawnLine(_sessionId: string): Promise<QueuedLine | null> {
      await wait();
      return null;
    },
    async terminalSubscribe(_sessionId: string, _cursor: number, _onEvent: (event: TerminalStreamEvent) => void): Promise<() => Promise<void>> {
      await wait();
      return async () => {};
    },
    async terminalAck(_sessionId: string, _cursor: number): Promise<void> {
      await wait();
    },
    async hotkeyCommit(_requestId: string, accelerator: string): Promise<{ registered: string | null; message: string | null }> {
      await wait();
      surfaceState.hotkey = accelerator;
      // 与真实宿主一致：注册成功的候选持久化进 settings.hotkey。
      const parts = accelerator.split("+").filter(Boolean);
      const key = parts.pop() ?? "";
      const modifiers: import("@fleqi/contracts").HotkeyModifier[] = parts.map((m) =>
        m === "CommandOrControl" || m === "Cmd"
          ? "command"
          : m === "Shift"
            ? "shift"
            : m === "Alt" || m === "Option"
              ? "option"
              : "control",
      );
      settings = { ...settings, hotkey: { key, modifiers } };
      emit({ kind: "settingsChanged", revision: settingsSnapshot().revision });
      return { registered: accelerator, message: null };
    },
    async hotkeyClear(_requestId: string): Promise<{ registered: string | null; message: string | null }> {
      await wait();
      surfaceState.hotkey = null;
      settings = { ...settings, hotkey: null };
      emit({ kind: "settingsChanged", revision: settingsSnapshot().revision });
      return { registered: null, message: null };
    },
    // ---- M3 预览实现（内存状态；语义与真实宿主一致，数据为预览示例） ----
    async runList(sessionId: string): Promise<import("@fleqi/contracts").RunRecord[]> {
      await wait();
      return m3.runs.filter((run) => run.sessionId === sessionId);
    },
    async runGet(runId: string): Promise<import("@fleqi/contracts").RunRecord> {
      await wait();
      const run = m3.runs.find((r) => r.id === runId);
      if (!run) throw { code: "not_found", message: `Run ${runId} 不存在`, retryable: false } satisfies AppError;
      return { ...run };
    },
    async runApprove(_requestId: string, runId: string, planRevision: string): Promise<import("@fleqi/contracts").RunRecord> {
      await wait();
      const run = m3.runs.find((r) => r.id === runId);
      if (!run) throw { code: "not_found", message: `Run ${runId} 不存在`, retryable: false } satisfies AppError;
      if (run.state !== "awaitingApproval") {
        throw { code: "conflict", message: `Run 状态 ${run.state} 不接受确认`, retryable: false } satisfies AppError;
      }
      if (run.planRevision !== planRevision) {
        throw { code: "conflict", message: "计划版本已过期", retryable: false } satisfies AppError;
      }
      run.state = "succeeded";
      run.exitStatus = 0;
      run.output = "预览执行输出\n";
      return { ...run };
    },
    async runCancel(runId: string): Promise<import("@fleqi/contracts").RunRecord> {
      await wait();
      const run = m3.runs.find((r) => r.id === runId);
      if (!run) throw { code: "not_found", message: `Run ${runId} 不存在`, retryable: false } satisfies AppError;
      run.state = "cancelled";
      return { ...run };
    },
    async runPlanGet(runId: string) {
      const run = m3.runs.find((item) => item.id === runId);
      if (!run) throw PREVIEW_FAILURE;
      return { id: runId, revision: run.planRevision, capabilityId: null, contextId: run.contextId, steps: [], requiredTools: [], effects: [], previewCompleteness: "unknown" as const, sourceFingerprint: "preview" };
    },
    async runRetry(_requestId: string, runId: string): Promise<import("@fleqi/contracts").RunRecord> {
      await wait();
      const run = m3.runs.find((r) => r.id === runId);
      if (!run) throw { code: "not_found", message: `Run ${runId} 不存在`, retryable: false } satisfies AppError;
      const retried: import("@fleqi/contracts").RunRecord = {
        ...run,
        id: `run-preview-${m3.runs.length + 1}`,
        parentRunId: run.id,
        state: "queued",
        exitStatus: null,
        output: "",
      };
      m3.runs.push(retried);
      return { ...retried };
    },
    async runPlanSubmit(
      _requestId: string,
      sessionId: string,
      _contextId: string,
      prompt: string,
    ): Promise<import("@fleqi/contracts").PlanOutcome> {
      await wait();
      if (m3.providers.length === 0) {
        throw {
          code: "unavailable",
          message: "尚未配置模型端点：请先在设置的模型页添加 OpenAI 兼容端点并选择默认模型",
          retryable: false,
        } satisfies AppError;
      }
      const run: import("@fleqi/contracts").RunRecord = {
        id: `run-preview-${m3.runs.length + 1}`,
        sessionId,
        parentRunId: null, plan: null, stepResults: [], directoryDisplay: "", requestId: null, requestFingerprint: "", approvalRequestId: null,
        origin: "ai",
        prompt,
        contextId: _contextId,
        planRevision: String(m3.planCounter++),
        state: "awaitingApproval",
        policy: settings.aiPolicy,
        output: "",
        exitStatus: null,
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      };
      m3.runs.push(run);
      return { kind: "execute", run: { ...run } };
    },
    async runPlanCancel(_requestId: string): Promise<void> {
      await wait();
    },
    async providerList(): Promise<import("@fleqi/contracts").ProviderView[]> {
      await wait();
      return m3.providers.map((view) => ({ ...view, record: { ...view.record } }));
    },
    async providerSave(request: import("@fleqi/contracts").ProviderSaveRequest): Promise<import("@fleqi/contracts").ProviderView> {
      await wait();
      if (!request.id.trim()) {
        throw { code: "validation", message: "端点 ID 不能为空", retryable: false } satisfies AppError;
      }
      const view: import("@fleqi/contracts").ProviderView = {
        record: {
          id: request.id,
          displayName: request.displayName,
          baseUrl: request.baseUrl,
          models: request.models,
          defaultGenerationModel: request.defaultGenerationModel,
          summaryModel: request.summaryModel,
          timeoutMs: request.timeoutMs,
        },
        // 与后端 provider_save 语义一致：null 保持现状；空串清除密钥。
        credentialConfigured:
          request.apiKey === null
            ? m3.providers.find((p) => p.record.id === request.id)?.credentialConfigured ?? false
            : request.apiKey.length > 0,
      };
      m3.providers = [view, ...m3.providers.filter((p) => p.record.id !== request.id)];
      return { ...view, record: { ...view.record } };
    },
    async providerDelete(providerId: string): Promise<void> {
      await wait();
      m3.providers = m3.providers.filter((p) => p.record.id !== providerId);
    },
    async providerProbe(baseUrl: string): Promise<{ ok: boolean; models: string[]; error: string | null }> {
      await wait();
      if (!baseUrl.includes("127.0.0.1")) {
        return { ok: true, models: ["preview-mini", "preview-pro"], error: null };
      }
      return { ok: false, models: [], error: "预览：本地端点未运行" };
    },
    async toolsPrepare() { return { running: false, cancelled: false, completed: 0, total: 0, currentTool: null, progress: null, errors: ["浏览器预览不会安装系统工具"] }; },
    async toolsPrepareStatus() { return { running: false, cancelled: false, completed: 0, total: 0, currentTool: null, progress: null, errors: [] }; },
    async toolsPrepareCancel() {},
    async toolsList(): Promise<import("@fleqi/contracts").ToolEntry[]> {
      await wait();
      return m3.tools.map((entry) => ({ ...entry, manifest: { ...entry.manifest }, installed: entry.installed ? { ...entry.installed } : null }));
    },
    async toolsInstall(_requestId: string, toolId: string): Promise<import("@fleqi/contracts").ToolEntry> {
      await wait();
      const entry = m3.tools.find((t) => t.manifest.id === toolId);
      if (!entry) throw { code: "not_found", message: `工具 ${toolId} 不在目录中`, retryable: false } satisfies AppError;
      if (entry.manifest.source.kind === "system") {
        throw { code: "conflict", message: `系统工具 ${toolId} 由系统安装，Fleqi 仅登记状态`, retryable: false } satisfies AppError;
      }
      entry.status = { kind: "available", version: entry.manifest.version, owner: "fleqi", path: `/预览/tools/${toolId}/${entry.manifest.executable}` };
      entry.installed = { manifest: entry.manifest, installedAt: new Date().toISOString(), installDir: `/预览/tools/${toolId}` };
      return { ...entry, manifest: { ...entry.manifest }, installed: entry.installed ? { ...entry.installed } : null };
    },
    async toolsInstallStatus(_requestId: string): Promise<import("@fleqi/contracts").InstallProgress | null> {
      await wait();
      return null;
    },
    async toolsInstallCancel(_requestId: string): Promise<boolean> {
      await wait();
      return false;
    },
    async toolsRemove(toolId: string): Promise<void> {
      await wait();
      const entry = m3.tools.find((t) => t.manifest.id === toolId);
      if (!entry?.installed) throw { code: "not_found", message: `工具 ${toolId} 没有受管安装记录`, retryable: false } satisfies AppError;
      entry.status = { kind: "notInstalled" };
      entry.installed = null;
    },
    async capabilityForm() { throw { code: "unavailable", message: "浏览器预览不读取真实选区；请在桌面 App 中使用本地能力。", retryable: false }; },
    async capabilitySubmit() { throw { code: "unavailable", message: "浏览器预览不执行文件操作", retryable: false }; },
    async catalogQuery(): Promise<CatalogEntry[]> {
      await wait();
      return m3.catalog;
    },
    async rulesCreate(name: string, content: string, scope?: import("@fleqi/contracts").RuleScope): Promise<import("@fleqi/contracts").Rule> {
      await wait();
      const rule: import("@fleqi/contracts").Rule = {
        id: `rule-preview-${m3.rules.length + 1}`,
        name,
        content,
        enabled: true,
        scope: scope ?? { kind: "global" },
        revision: "1",
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      };
      m3.rules.push(rule);
      return { ...rule };
    },
    async rulesList(): Promise<import("@fleqi/contracts").Rule[]> {
      await wait();
      return m3.rules.map((rule) => ({ ...rule }));
    },
    async rulesUpdate(ruleId: string, patch: { name?: string; enabled?: boolean; content?: string }): Promise<import("@fleqi/contracts").Rule> {
      await wait();
      const rule = m3.rules.find((r) => r.id === ruleId);
      if (!rule) throw { code: "not_found", message: `规则 ${ruleId} 不存在`, retryable: false } satisfies AppError;
      Object.assign(rule, patch);
      return { ...rule };
    },
    async rulesDelete(ruleId: string): Promise<void> {
      await wait();
      m3.rules = m3.rules.filter((r) => r.id !== ruleId);
    },
    async favoritesCreate(name: string, content: string, kind: string): Promise<import("@fleqi/contracts").Favorite> {
      await wait();
      const favorite: import("@fleqi/contracts").Favorite = {
        id: `fav-preview-${m3.favorites.length + 1}`,
        name,
        kind,
        content,
        tags: [],
        createdAt: new Date().toISOString(),
        lastUsedAt: null,
      };
      m3.favorites.push(favorite);
      return { ...favorite };
    },
    async favoritesList(): Promise<import("@fleqi/contracts").Favorite[]> {
      await wait();
      return m3.favorites.map((favorite) => ({ ...favorite }));
    },
    async favoritesUpdate(favoriteId: string, patch: { name?: string; tags?: string[] }): Promise<import("@fleqi/contracts").Favorite> {
      await wait();
      const favorite = m3.favorites.find((f) => f.id === favoriteId);
      if (!favorite) throw { code: "not_found", message: `收藏 ${favoriteId} 不存在`, retryable: false } satisfies AppError;
      Object.assign(favorite, patch);
      return { ...favorite };
    },
    async favoritesDelete(favoriteId: string): Promise<void> {
      await wait();
      m3.favorites = m3.favorites.filter((f) => f.id !== favoriteId);
    },
    async historyList(): Promise<string[]> {
      await wait();
      return [...m3.history];
    },
    async historyAppend(entry: string): Promise<void> {
      await wait();
      m3.history = [entry, ...m3.history.filter((existing) => existing !== entry)].slice(0, 200);
    },
    async historyClear(): Promise<void> {
      await wait();
      m3.history = [];
    },
    async updateSettings(request: SettingsUpdateRequest) {
      await wait();
      const fingerprint = JSON.stringify([request.expectedRevision, request.patch]);
      const existing = receipts.get(request.requestId);
      if (existing) {
        if (existing.fingerprint === fingerprint) return existing.snapshot;
        throw { code: "conflict", message: "同一 requestId 携带了不同载荷", retryable: false, currentRevision: String(revision) } satisfies AppError;
      }
      if (degraded) throw { code: "storage", message: "存储不可用：设置仍是未持久化的临时默认值", retryable: true } satisfies AppError;
      if (request.expectedRevision !== String(revision)) {
        throw {
          code: "conflict",
          message: `期望版本 ${request.expectedRevision} 已过期，当前 ${revision}`,
          retryable: false,
          currentRevision: String(revision),
        } satisfies AppError;
      }
      const blocked = Object.keys(request.patch).filter((field) => !EDITABLE_FIELDS.has(field));
      if (blocked.length > 0) {
        throw {
          code: "validation",
          message: `${blocked.join("、")} 由后续阶段交付`,
          retryable: false,
          fieldErrors: blocked.map((field) => ({ field, code: "notAvailable", message: "该设置由后续阶段交付，当前阶段不可修改" })),
        } satisfies AppError;
      }
      settings = { ...settings, ...request.patch } as Settings;
      revision += 1;
      const snapshot = settingsSnapshot();
      receipts.set(request.requestId, { fingerprint, snapshot });
      emit({ kind: "settingsChanged", revision: snapshot.revision });
      return snapshot;
    },
    async permissionsGet() {
      await wait();
      return permissionSnapshot();
    },
    async permissionsCheck() {
      await wait();
      permissions = [record("finderAutomation", "needsConsent"), record("accessibility", "needsConsent")];
      permissionRevision += 1;
      emit({ kind: "permissionsChanged", revision: String(permissionRevision) });
      return permissionSnapshot();
    },
    async permissionsRequest(requestId: string, permission: Permission) {
      await wait();
      const operation: PermissionOperation = { operationId: `preview-op-${requestId}`, permission, state: "inProgress", startedAt: new Date().toISOString() };
      setTimeout(() => {
        permissions = permissions.map((r) => (r.permission === permission ? { ...record(permission, "allowed"), procedure: "explicit" } : r));
        permissionRevision += 1;
        emit({ kind: "permissionsChanged", revision: String(permissionRevision) });
      }, delayMs);
      return operation;
    },
    async permissionsOpenSettings() {
      await wait();
    },
    async contextGet(contextId?: string) {
      await wait();
      if (contextId && latestContext?.id !== contextId) {
        throw { code: "not_found", message: `上下文 ${contextId} 已不在保留范围`, retryable: false } satisfies AppError;
      }
      return latestContext ?? makeContext("finder", "/Users/preview/Documents/示例 文件夹");
    },
    async contextRefresh() {
      await wait();
      const allowed = permissions.some((r) => r.permission === "finderAutomation" && r.status === "allowed");
      return makeContext("finder", allowed ? "/Users/preview/Documents/示例 文件夹" : null);
    },
    async contextPickDirectory() {
      await wait();
      if (!pick) return { kind: "cancelled" };
      return { kind: "selected", snapshot: makeContext("picker", "/Users/preview/Projects/选择的目录") };
    },
    subscribe(handler) {
      handlers.add(handler);
      return () => {
        handlers.delete(handler);
      };
    },
  };
}
