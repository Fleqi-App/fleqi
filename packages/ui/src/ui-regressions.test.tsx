import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { AppEvent, RunRecord } from "@fleqi/contracts";
import { App } from "./App";
import { createPreviewAdapter } from "./adapters/host";

async function composer() {
  const adapter = createPreviewAdapter({ delayMs: 0 });
  await adapter.hotkeyCommit("setup", "CommandOrControl+Shift+F");
  const surface = await adapter.surfaceShow();
  window.location.hash = "#/composer";
  return { adapter, sessionId: surface.visibleSessionId! };
}

describe("UI 宿主链路回归", () => {
  it.each(["ready", "unknown"] as const)("Bash 终端按集成状态显示自动同步提示（%s）", async (readiness) => {
    const user = userEvent.setup();
    const { adapter } = await composer();
    const snapshot = adapter.terminalSnapshot.bind(adapter);
    adapter.terminalSnapshot = async (id) => ({ ...await snapshot(id), shell: "/bin/bash", shellReadiness: readiness });
    render(<App adapter={adapter} />);
    await waitFor(() => expect(screen.getByTestId("composer").dataset.surface).toBe("visible"));
    await user.click(screen.getByRole("button", { name: "打开终端面板" }));
    await waitFor(() => expect(screen.getByTestId("terminal-readiness").textContent).toBe(readiness === "ready" ? "可输入" : "状态未知"));
    expect(screen.queryByText(/Bash 自动目录同步尚未就绪或不可用/) !== null).toBe(readiness === "unknown");
  });

  it.each([true, false])("Windows 概览使用实际 Explorer 能力状态（可用=%s）", async (available) => {
    const adapter = createPreviewAdapter({ delayMs: 0 });
    const boot = await adapter.bootstrap();
    adapter.bootstrap = async () => ({
      ...boot,
      buildInfo: { ...boot.buildInfo, targetOs: "windows" },
      permissions: { ...boot.permissions, records: boot.permissions.records.map((record) => ({ ...record, status: "failed" as const })) },
      platform: { ...boot.platform, items: boot.platform.items.map((item) => item.id === "finderContext" ? { ...item, state: available ? "supported" as const : "temporarilyUnavailable" as const } : item) },
    });
    window.location.hash = "#/console/overview";
    render(<App adapter={adapter} />);
    expect(await screen.findByRole("button", { name: `资源管理器 读取：${available ? "可用" : "暂不可用"}` })).toBeDefined();
    expect(screen.queryByRole("button", { name: "资源管理器 权限：待检查" })).toBeNull();
  });

  it("页面挂载只读 surface；选择另一个会话后命令发送到新会话", async () => {
    const user = userEvent.setup();
    const { adapter, sessionId } = await composer();
    const next = await adapter.sessionCreate("second");
    const show = vi.spyOn(adapter, "surfaceShow");
    const submit = vi.spyOn(adapter, "terminalSubmitLine");
    render(<App adapter={adapter} />);
    await screen.findByTestId("composer-input");
    await user.click(screen.getByRole("button", { name: "会话选择器" }));
    const row = (await screen.findAllByTestId("session-row")).find((item) => item.dataset.sessionId === next.id)!;
    await user.click(within(row).getAllByRole("button")[0]!);
    await waitFor(() => expect(screen.queryByTestId("session-selector")).toBeNull());
    await user.type(screen.getByTestId("composer-input"), "!pwd{Enter}");
    await waitFor(() => expect(submit).toHaveBeenCalled());
    expect(submit.mock.calls[0]![1]).toBe(next.id);
    expect(next.id).not.toBe(sessionId);
    expect(show).not.toHaveBeenCalled();
  });

  it("输入法确认 Enter 不提交；在途规划重复 Enter 只提交一次", async () => {
    const { adapter } = await composer();
    const submit = vi.spyOn(adapter, "runPlanSubmit").mockImplementation(() => new Promise(() => {}));
    render(<App adapter={adapter} />);
    const input = await screen.findByTestId("composer-input");
    await waitFor(() => expect(screen.getByTestId("composer").dataset.surface).toBe("visible"));
    fireEvent.change(input, { target: { value: "测试" } });
    fireEvent.keyDown(input, { key: "Enter", isComposing: true });
    expect(submit).not.toHaveBeenCalled();
    fireEvent.keyDown(input, { key: "Enter" });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(submit).toHaveBeenCalledTimes(1);
  });

  it("规划与结果使用同一右侧面板，忙碌时保留下一条草稿且禁止发送", async () => {
    const user = userEvent.setup();
    const { adapter } = await composer();
    let cancel!: (error: unknown) => void;
    let completeCancellation!: () => void;
    const cancellation = new Promise<void>((resolve) => { completeCancellation = resolve; });
    vi.spyOn(adapter, "runPlanSubmit").mockImplementation(() => new Promise((_resolve, reject) => { cancel = reject; }));
    vi.spyOn(adapter, "runPlanCancel").mockImplementation(async () => { await cancellation; cancel({ code: "unavailable", message: "已取消", retryable: false }); });
    const layout = vi.spyOn(adapter, "surfaceLayout");
    render(<App adapter={adapter} />);
    await waitFor(() => expect(screen.getByTestId("composer").dataset.surface).toBe("visible"));
    await user.type(screen.getByTestId("composer-input"), "等待取消{Enter}");
    await waitFor(() => expect(layout).toHaveBeenLastCalledWith(360));
    expect(screen.getByTestId("composer").dataset.planning).toBe("true");
    expect(screen.getByTestId("composer-submit").hasAttribute("disabled")).toBe(true);
    await user.type(screen.getByTestId("composer-input"), "下一条草稿");
    fireEvent.keyDown(screen.getByTestId("composer-input"), { key: "Enter" });
    expect(adapter.runPlanSubmit).toHaveBeenCalledTimes(1);
    await user.click(screen.getByTestId("planning-cancel"));
    expect(screen.getByTestId("planning-status").textContent).toContain("正在取消");
    await act(async () => { completeCancellation(); });
    await waitFor(() => expect(screen.queryByTestId("planning-status")).toBeNull());
    expect(screen.getByTestId("task-panel-message").textContent).toContain("已取消");
    expect((screen.getByTestId("composer-input") as HTMLTextAreaElement).value).toBe("下一条草稿");
    expect(screen.getByTestId("composer").dataset.planning).toBe("false");
    expect(screen.getByTestId("composer-submit").hasAttribute("disabled")).toBe(false);
  });

  it("本次提交的 Run 完成后仍展示结果；连续同类事件刷新任务状态", async () => {
    const user = userEvent.setup();
    const { adapter, sessionId } = await composer();
    const settings = (await adapter.bootstrap()).settings;
    await adapter.updateSettings({ requestId: "persistent", expectedRevision: settings.revision, patch: { bubbleSeconds: null } });
    const handlers = new Set<(event: AppEvent) => void>();
    adapter.subscribe = (handler) => { handlers.add(handler); return () => { handlers.delete(handler); }; };
    let run: RunRecord = { id: "submitted-run", sessionId, parentRunId: null, plan: null, stepResults: [], directoryDisplay: "", requestId: null, requestFingerprint: "", approvalRequestId: null, origin: "ai", prompt: "列出文件", contextId: "ctx", planRevision: "1", state: "running", policy: "yolo", output: "", exitStatus: null, createdAt: "now", updatedAt: "now" };
    adapter.runPlanSubmit = async () => ({ kind: "execute", run });
    adapter.runGet = async () => run;
    render(<App adapter={adapter} />);
    await waitFor(() => expect(screen.getByTestId("composer").dataset.surface).toBe("visible"));
    await user.type(screen.getByTestId("composer-input"), "列出文件{Enter}");
    await waitFor(() => expect(screen.getByTestId("composer-notice").textContent).toContain("已提交"));
    await user.click(await screen.findByRole("button", { name: "关闭任务详情" }));
    act(() => { for (const handler of handlers) handler({ kind: "runChanged", runId: run.id, revision: "1" }); });
    run = { ...run, state: "succeeded", output: "真实完成结果", exitStatus: 0 };
    act(() => { for (const handler of handlers) handler({ kind: "runChanged", runId: run.id, revision: "1" }); });
    const bubble = await screen.findByTestId("result-bubble");
    expect(bubble.textContent).toContain("真实完成结果");
    expect(bubble.textContent).toContain("常驻");
  });

  it("手动 cd 后的 ! 命令使用终端实际目录；pending 时绑定最新目标", async () => {
    const user = userEvent.setup();
    const { adapter, sessionId } = await composer();
    const list = await adapter.sessionList();
    let active = { ...list.active.find((session) => session.id === sessionId)!, currentDirectory: "/tmp/manual-cwd", targetDirectory: "/tmp/finder-old", directorySync: "synced" as const };
    adapter.sessionList = async () => ({ active: [active], history: [] });
    const submit = vi.spyOn(adapter, "terminalSubmitLine").mockResolvedValue("sent");
    const view = render(<App adapter={adapter} />);
    await waitFor(() => expect(screen.getByTestId("composer").dataset.surface).toBe("visible"));
    await user.type(screen.getByTestId("composer-input"), "!pwd{Enter}");
    expect(submit.mock.calls[0]![4]).toBe("/tmp/manual-cwd");
    view.unmount();
    adapter.sessionList = async () => ({ active: [{ ...active, targetDirectory: "/tmp/finder-latest", directorySync: "pending" }], history: [] });
    render(<App adapter={adapter} />);
    await waitFor(() => expect(screen.getByTestId("composer").dataset.surface).toBe("visible"));
    await user.type(screen.getByTestId("composer-input"), "!pwd{Enter}");
    expect(submit.mock.calls[1]![4]).toBe("/tmp/finder-latest");
  });

  it("能力卡片进入同源分类，搜索无结果有明确空态", async () => {
    const user = userEvent.setup();
    window.location.hash = "#/console/overview";
    render(<App adapter={createPreviewAdapter({ delayMs: 0 })} />);
    await user.click((await screen.findAllByTestId("capability-category"))[1]!);
    const category = await screen.findByLabelText("能力分类");
    expect((category as HTMLSelectElement).value).toBe("zip");
    expect(screen.getAllByTestId("catalog-entry").length).toBe(2);
    await user.type(screen.getByLabelText("搜索能力"), "不存在的能力");
    expect(await screen.findByText("没有匹配的能力")).toBeDefined();
  });
});
