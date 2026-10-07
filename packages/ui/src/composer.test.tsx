import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { AppError, AppEvent, QueuedLine, RunRecord } from "@fleqi/contracts";
import { App } from "./App";
import { createPreviewAdapter } from "./adapters/host";
import { parseMode, runConclusion } from "./pages/composer/ComposerShell";

function go(hash: string) {
  window.location.hash = hash;
}

describe("`!` 模式解析（FR-TERM-001）", () => {
  it("首个非空白半角 ! 触发终端模式；全角/句中不触发", () => {
    expect(parseMode("!ls -la")).toEqual({ mode: "terminal", body: "ls -la" });
    expect(parseMode("  !echo")).toEqual({ mode: "terminal", body: "echo" });
    expect(parseMode("！ls")).toEqual({ mode: "ai", body: "！ls" });
    expect(parseMode("hi !world")).toEqual({ mode: "ai", body: "hi !world" });
  });
});

describe("完成结论归纳（§7 短结论，无摘要时如实归纳）", () => {
  const base = {
    id: "r", sessionId: "s", parentRunId: null, plan: null, stepResults: [], directoryDisplay: "", requestId: null, requestFingerprint: "", approvalRequestId: null, origin: "ai" as const, prompt: "p",
    contextId: "ctx", planRevision: "1", policy: "yolo" as const,
    createdAt: "2026-09-19T00:00:00Z", updatedAt: "2026-09-19T00:00:01Z",
  };
  it("成功取输出末行；无输出如实说明", () => {
    expect(runConclusion({ ...base, state: "succeeded", output: "a\nb\nc done", exitStatus: 0 }))
      .toMatchObject({ tone: "success", text: "c done" });
    expect(runConclusion({ ...base, state: "succeeded", output: "", exitStatus: 0 }))
      .toMatchObject({ tone: "success", text: "执行完成（无输出）" });
  });
  it("失败带退出码；部分成功给警示", () => {
    expect(runConclusion({ ...base, state: "failed", output: "not found", exitStatus: 127 }))
      .toMatchObject({ tone: "error", text: "执行失败（退出码 127）：not found" });
    expect(runConclusion({ ...base, state: "partiallySucceeded", output: "", exitStatus: null }))
      .toMatchObject({ tone: "warning", text: "部分成功：部分步骤未通过" });
  });
});

describe("输入条窗口", () => {
  it("同步期间的终端输入撤销等待命令后保留草稿并清除排队提示", async () => {
    const user = userEvent.setup();
    const adapter = createPreviewAdapter({ delayMs: 0 });
    await adapter.hotkeyCommit("warm", "CommandOrControl+Shift+F");
    const surface = await adapter.surfaceShow();
    const handlers = new Set<(event: AppEvent) => void>();
    let queued: QueuedLine | null = null;
    let withdrawn = false;
    Object.assign(adapter, {
      subscribe(handler: (event: AppEvent) => void) { handlers.add(handler); return () => { handlers.delete(handler); }; },
      async terminalSubmitLine(requestId: string, sessionId: string) {
        queued = { requestId, sessionId, contextRevision: "1", target: "target", text: "echo queued" };
        return "queued" as const;
      },
      async terminalWithdrawnLine() {
        if (!withdrawn) return null;
        const line = queued; queued = null; return line;
      },
    });
    go("#/composer");
    render(<App adapter={adapter} />);
    await waitFor(() => expect(screen.getByTestId("composer").getAttribute("data-surface")).toBe("visible"));
    const input = screen.getByTestId("composer-input") as HTMLTextAreaElement;
    await user.type(input, "!echo queued{Enter}");
    await waitFor(() => expect(screen.getByTestId("queued-line")).toBeDefined());
    withdrawn = true;
    await act(async () => { for (const handler of handlers) handler({ kind: "terminalChanged", sessionId: surface.visibleSessionId!, revision: "2" }); });
    await waitFor(() => expect(screen.queryByTestId("queued-line")).toBeNull());
    expect(input.value).toBe("!echo queued");
    expect(screen.getByTestId("composer-notice").textContent).toContain("等待命令已撤销");
  });
  it("无热键时显示被拒并提示注册快捷键（FR-ENTRY-003）", async () => {
    go("#/composer");
    render(<App adapter={createPreviewAdapter({ delayMs: 0 })} />);
    await waitFor(() => expect(screen.getByTestId("composer")).toBeDefined());
    expect(screen.getByTestId("composer").getAttribute("data-surface")).toBe("userHidden");
    expect(screen.getByTestId("composer-surface-notice").textContent).toContain("快捷键");
  });

  it("注册热键后显示创建会话；`!` 输入全绿并提交成功（AC-FLOW-009）", async () => {
    const user = userEvent.setup();
    const adapter = createPreviewAdapter({ delayMs: 0 });
    await adapter.hotkeyCommit("warm", "CommandOrControl+Shift+F");
    await adapter.surfaceShow();
    go("#/composer");
    render(<App adapter={adapter} />);
    await waitFor(() => expect(screen.getByTestId("composer").getAttribute("data-surface")).toBe("visible"));
    const input = screen.getByTestId("composer-input");
    await user.type(input, "!echo fleqi-m2-pty-ok");
    expect(screen.getByTestId("composer").getAttribute("data-mode")).toBe("terminal");
    expect(screen.getByTestId("composer-mode").textContent).toBe("终端");
    expect(input.className).toContain("text-terminal-mode-text");
    await user.type(input, "{Enter}");
    await waitFor(() => expect(screen.getByTestId("composer-notice").textContent).toContain("已发送到终端"));
    expect((input as HTMLTextAreaElement).value).toBe("");
  });

  it("AI 输入未配置模型：宿主如实引导配置（不伪造提交）", async () => {
    const user = userEvent.setup();
    const adapter = createPreviewAdapter({ delayMs: 0 });
    await adapter.hotkeyCommit("warm", "CommandOrControl+Shift+F");
    await adapter.surfaceShow();
    go("#/composer");
    render(<App adapter={adapter} />);
    await waitFor(() => expect(screen.getByTestId("composer").getAttribute("data-surface")).toBe("visible"));
    await user.type(screen.getByTestId("composer-input"), "总结这个文件夹{Enter}");
    await waitFor(() => expect(screen.getByTestId("composer-notice").textContent).toContain("模型端点"));
    expect(screen.getByTestId("composer").getAttribute("data-mode")).toBe("ai");
    expect(screen.queryByTestId("result-bubble")).toBeNull();
    // 引导带可点的"配置模型 API"入口（ui-design.md §输入条），落到设置模型页。
    const action = screen.getByTestId("notice-action");
    expect(action.getAttribute("data-action")).toBe("settings-models");
    expect(action.textContent).toContain("配置模型 API");
    await user.click(action);
    await waitFor(() => expect(window.location.hash).toBe("#/settings/models"));
  });

  it("配置端点后 AI 提交走规划；等待确认给通知，完成结果经 runChanged 上气泡（§7）", async () => {
    const user = userEvent.setup();
    const adapter = createPreviewAdapter({ delayMs: 0 });
    await adapter.hotkeyCommit("warm", "CommandOrControl+Shift+F");
    await adapter.surfaceShow();
    await adapter.providerSave({
      id: "local",
      displayName: "本地端点",
      baseUrl: "http://127.0.0.1:11434/v1",
      models: ["demo"],
      defaultGenerationModel: "demo",
      summaryModel: null,
      timeoutMs: 5000,
      apiKey: null,
    });
    go("#/composer");
    render(<App adapter={adapter} />);
    await waitFor(() => expect(screen.getByTestId("composer").getAttribute("data-surface")).toBe("visible"));
    await user.type(screen.getByTestId("composer-input"), "列出大文件{Enter}");
    await waitFor(() => expect(screen.getByTestId("composer-notice").textContent).toContain("计划"));
    expect((screen.getByTestId("composer-input") as HTMLTextAreaElement).value).toBe("");
  });

  it("run 终态事件 → §7 结果气泡：短结论、复制真实输出、回填草稿回应", async () => {
    const user = userEvent.setup();
    const adapter = createPreviewAdapter({ delayMs: 0 });
    await adapter.hotkeyCommit("warm", "CommandOrControl+Shift+F");
    await adapter.surfaceShow();
    const surface = await adapter.surfaceShow();
    const sessionId = surface.visibleSessionId ?? "preview-session";
    const handlers = new Set<(event: AppEvent) => void>();
    const runRecord: RunRecord = {
      id: "run-bubble-1",
      sessionId,
      parentRunId: null, plan: null, stepResults: [], directoryDisplay: "", requestId: null, requestFingerprint: "", approvalRequestId: null,
      origin: "ai",
      prompt: "这是一个什么项目?",
      contextId: "ctx",
      planRevision: "1",
      state: "succeeded",
      policy: "yolo",
      output: "total 24\nProject directory with source code and docs",
      exitStatus: 0,
      createdAt: "2026-09-19T00:00:00Z",
      updatedAt: "2026-09-19T00:00:01Z",
    };
    const stub = Object.assign(adapter, {
      subscribe(handler: (event: AppEvent) => void) {
        handlers.add(handler);
        return () => {
          handlers.delete(handler);
        };
      },
      runGet: async (runId: string) => {
        if (runId !== runRecord.id) throw { code: "not_found", message: "run 不存在", retryable: false } satisfies AppError;
        return runRecord;
      },
    });
    go("#/composer");
    render(<App adapter={stub} />);
    await waitFor(() => expect(screen.getByTestId("composer").getAttribute("data-surface")).toBe("visible"));
    expect(screen.queryByTestId("result-bubble")).toBeNull();
    await act(async () => { for (const handler of handlers) handler({ kind: "runChanged", runId: runRecord.id, revision: "1" }); });
    const bubble = await screen.findByTestId("result-bubble");
    expect(bubble.getAttribute("data-bubble-kind")).toBe("result");
    expect(bubble.textContent).toContain("Project directory with source code and docs");
    // 复制真实输出（stub 剪贴板），回应把原问题回填草稿。
    let copied = "";
    Object.defineProperty(navigator, "clipboard", { value: { writeText: async (text: string) => { copied = text; } }, configurable: true });
    await user.click(screen.getByTestId("bubble-copy"));
    expect(copied).toBe(runRecord.output);
    await user.click(screen.getByTestId("bubble-respond"));
    expect((screen.getByTestId("composer-input") as HTMLTextAreaElement).value).toContain("这是一个什么项目?");
  });

  it("主条隐藏期间 run 完成：只登记未读徽标，不弹气泡（§7）", async () => {
    const adapter = createPreviewAdapter({ delayMs: 0 });
    const handlers = new Set<(event: AppEvent) => void>();
    const hiddenRun: RunRecord = {
      id: "run-hidden-1",
      sessionId: "any",
      parentRunId: null, plan: null, stepResults: [], directoryDisplay: "", requestId: null, requestFingerprint: "", approvalRequestId: null,
      origin: "ai",
      prompt: "p",
      contextId: "ctx",
      planRevision: "1",
      state: "succeeded",
      policy: "yolo",
      output: "done",
      exitStatus: 0,
      createdAt: "2026-09-19T00:00:00Z",
      updatedAt: "2026-09-19T00:00:01Z",
    };
    const stub = Object.assign(adapter, {
      subscribe(handler: (event: AppEvent) => void) {
        handlers.add(handler);
        return () => {
          handlers.delete(handler);
        };
      },
      runGet: async (runId: string) => {
        if (runId !== hiddenRun.id) throw { code: "not_found", message: "run 不存在", retryable: false } satisfies AppError;
        return hiddenRun;
      },
    });
    go("#/composer");
    render(<App adapter={stub} />);
    await waitFor(() => expect(screen.getByTestId("composer").getAttribute("data-surface")).toBe("userHidden"));
    for (const handler of handlers) handler({ kind: "runChanged", runId: "run-hidden-1", revision: "1" });
    const unread = await screen.findByTestId("sessions-unread");
    expect(unread.textContent).toBe("1");
    expect(screen.queryByTestId("result-bubble")).toBeNull();
  });

  it("会话选择器：活跃/置顶分组，选择活跃会话连接；结束保留历史", async () => {
    const user = userEvent.setup();
    const adapter = createPreviewAdapter({ delayMs: 0 });
    await adapter.hotkeyCommit("warm", "CommandOrControl+Shift+F");
    await adapter.surfaceShow();
    await adapter.surfaceShow();
    go("#/composer");
    render(<App adapter={adapter} />);
    await waitFor(() => expect(screen.getByTestId("composer").getAttribute("data-surface")).toBe("visible"));
    await user.click(screen.getByRole("button", { name: "会话选择器" }));
    const selector = screen.getByTestId("session-selector");
    const rows = within(selector).getAllByTestId("session-row");
    expect(rows.length).toBeGreaterThanOrEqual(1);
    expect(rows[0]!.getAttribute("data-state")).toBe("active");
    await user.click(within(rows[0]!).getByRole("button", { name: /结束会话/ }));
    await waitFor(() => {
      const ended = screen.getAllByTestId("session-row").find((row) => row.getAttribute("data-state") === "ended");
      expect(ended).toBeTruthy();
    });
  });

  it("历史继续创建关联新会话；删除只移除记录（FR-SESSION-009/010）", async () => {
    const user = userEvent.setup();
    const adapter = createPreviewAdapter({ delayMs: 0 });
    await adapter.hotkeyCommit("warm", "CommandOrControl+Shift+F");
    await adapter.surfaceShow();
    await adapter.surfaceShow();
    go("#/composer");
    render(<App adapter={adapter} />);
    await waitFor(() => expect(screen.getByTestId("composer").getAttribute("data-surface")).toBe("visible"));
    await user.click(screen.getByRole("button", { name: "会话选择器" }));
    const activeRow = screen
      .getAllByTestId("session-row")
      .find((row) => row.getAttribute("data-state") === "active");
    expect(activeRow).toBeTruthy();
    const sourceId = activeRow!.getAttribute("data-session-id")!;
    await user.click(within(activeRow!).getByRole("button", { name: /结束会话/ }));
    await waitFor(() =>
      expect(
        screen
          .getAllByTestId("session-row")
          .some((row) => row.getAttribute("data-session-id") === sourceId && row.getAttribute("data-state") === "ended"),
      ).toBe(true),
    );
    const endedRow = screen
      .getAllByTestId("session-row")
      .find((row) => row.getAttribute("data-session-id") === sourceId)!;
    await user.click(within(endedRow).getByTestId("session-continue"));
    await waitFor(() => expect(screen.queryByTestId("session-selector")).toBeNull());
    await user.click(screen.getByRole("button", { name: "会话选择器" }));
    await waitFor(() => {
      const rows = screen.getAllByTestId("session-row");
      const continued = rows.find((row) => row.getAttribute("data-state") === "active" && row.getAttribute("data-session-id") !== sourceId);
      expect(continued).toBeTruthy();
    });
    const sourceRow = screen
      .getAllByTestId("session-row")
      .find((row) => row.getAttribute("data-session-id") === sourceId)!;
    expect(sourceRow.getAttribute("data-state")).toBe("ended");
    await user.click(within(sourceRow).getByRole("button", { name: /删除会话/ }));
    await waitFor(() =>
      expect(
        screen.getAllByTestId("session-row").some((row) => row.getAttribute("data-session-id") === sourceId),
      ).toBe(false),
    );
  });

  it("会话选择器提供新建会话：创建后成为唯一活跃会话（ui-design.md §429 有效入口）", async () => {
    const user = userEvent.setup();
    const adapter = createPreviewAdapter({ delayMs: 0 });
    await adapter.hotkeyCommit("warm", "CommandOrControl+Shift+F");
    await adapter.surfaceShow();
    await adapter.surfaceShow();
    go("#/composer");
    render(<App adapter={adapter} />);
    await waitFor(() => expect(screen.getByTestId("composer").getAttribute("data-surface")).toBe("visible"));
    await user.click(screen.getByRole("button", { name: "会话选择器" }));
    const activeRow = screen
      .getAllByTestId("session-row")
      .find((row) => row.getAttribute("data-state") === "active");
    expect(activeRow).toBeTruthy();
    const sourceId = activeRow!.getAttribute("data-session-id")!;
    // 结束当前活跃会话 → 活跃区为空。
    await user.click(within(activeRow!).getByRole("button", { name: /结束会话/ }));
    await waitFor(() => expect(screen.getByText("暂无活跃会话")).toBeTruthy());
    // 新建会话：选择器关闭，重开后有新的活跃会话且不是被结束的源会话。
    await user.click(screen.getByTestId("session-create"));
    await waitFor(() => expect(screen.queryByTestId("session-selector")).toBeNull());
    await user.click(screen.getByRole("button", { name: "会话选择器" }));
    await waitFor(() => {
      const rows = screen.getAllByTestId("session-row");
      const active = rows.find((row) => row.getAttribute("data-state") === "active");
      expect(active).toBeTruthy();
      expect(active!.getAttribute("data-session-id")).not.toBe(sourceId);
    });
  });

  it("隐藏按钮调用 surface_hide（FR-SESSION-004 keepAll 保留会话在宿主侧处理）", async () => {
    const user = userEvent.setup();
    const adapter = createPreviewAdapter({ delayMs: 0 });
    await adapter.hotkeyCommit("warm", "CommandOrControl+Shift+F");
    await adapter.surfaceShow();
    go("#/composer");
    render(<App adapter={adapter} />);
    await waitFor(() => expect(screen.getByTestId("composer").getAttribute("data-surface")).toBe("visible"));
    await user.click(screen.getByRole("button", { name: "隐藏输入条" }));
    await waitFor(() => expect(screen.getByTestId("composer").getAttribute("data-surface")).toBe("userHidden"));
  });
});
