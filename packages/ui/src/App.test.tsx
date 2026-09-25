import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { App } from "./App";
import { createPreviewAdapter, PREVIEW_FAILURE } from "./adapters/host";

function go(hash: string) {
  window.location.hash = hash;
}

describe("控制台", () => {
  it("加载宿主快照后展示真实宿主/存储状态并标识预览", async () => {
    go("#/console/overview");
    render(<App adapter={createPreviewAdapter({ delayMs: 0 })} />);
    expect(screen.getByRole("status").textContent).toContain("正在读取宿主状态");
    await waitFor(() => expect(screen.getByTestId("host-state").getAttribute("data-host-state")).toBe("ready"));
    expect(screen.getByTestId("storage-state").getAttribute("data-storage-state")).toBe("ready");
    expect(screen.getByTestId("host-kind").getAttribute("data-host-kind")).toBe("preview");
    expect(document.querySelector('[data-field="settings-revision"]')?.textContent).toBe("0");
    expect(document.documentElement.dataset.theme).toBe("dark");
  });

  it("失败态显示错误并可重试", async () => {
    go("#/console/overview");
    render(<App adapter={createPreviewAdapter({ delayMs: 0, fail: true })} />);
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain(PREVIEW_FAILURE.message);
  });

  it("存储降级：宿主 degraded、设置未持久化提示", async () => {
    go("#/console/overview");
    render(<App adapter={createPreviewAdapter({ delayMs: 0, degraded: true })} />);
    await waitFor(() => expect(screen.getByTestId("host-state").getAttribute("data-host-state")).toBe("degraded"));
    expect(screen.getByTestId("storage-state").textContent).toContain("存储降级");
    expect(document.querySelector('[data-field="settings-revision"]')?.textContent).toContain("未持久化");
  });

  it("权限页：重新检查更新状态，显式申请后变为已授权，上下文可刷新", async () => {
    const user = userEvent.setup();
    go("#/console/permissions");
    render(<App adapter={createPreviewAdapter({ delayMs: 0 })} />);
    await screen.findByRole("heading", { name: "权限与自检" });
    expect(screen.getByTestId("status-finderAutomation").textContent).toContain("尚未检查");
    await user.click(screen.getByRole("button", { name: "重新检查全部" }));
    await waitFor(() => expect(screen.getByTestId("status-finderAutomation").textContent).toContain("未授权"));
    const finderCard = document.querySelector('[data-permission="finderAutomation"]') as HTMLElement;
    await user.click(within(finderCard).getByRole("button", { name: "显式申请" }));
    await waitFor(() => expect(screen.getByTestId("status-finderAutomation").textContent).toContain("已授权"), { timeout: 2000 });
    await user.click(screen.getByRole("button", { name: "刷新 Finder 上下文" }));
    await waitFor(() => expect(screen.getByTestId("context-snapshot").getAttribute("data-availability")).toBe("available"));
    expect(document.querySelector('[data-field="context-directory"]')?.textContent).toContain("示例 文件夹");
    await user.click(screen.getByRole("button", { name: "选择文件夹…" }));
    await waitFor(() => expect(screen.getByTestId("context-notice").textContent).toContain("已取消"));
    expect(screen.getByTestId("context-snapshot").getAttribute("data-context-id")).toBe("ctx-1");
  });

  it("概览首次配置：唤起与模型两步给真实状态与可点的前往入口", async () => {
    const user = userEvent.setup();
    const adapter = createPreviewAdapter({ delayMs: 0 });
    go("#/console/overview");
    render(<App adapter={adapter} />);
    await screen.findByRole("heading", { name: "概览" });
    // 默认 manual + 无热键 + 零端点：两步都是"未配置"。
    await waitFor(() => expect(screen.getByTestId("activation-status").textContent).toContain("未配置"));
    await waitFor(() => expect(screen.getByTestId("provider-status").textContent).toContain("未配置"));
    // 唤起一步的前往打开设置窗口通用页（预览里体现为 hash 导航）。
    await user.click(screen.getByTestId("activation-status").closest("button")!);
    await waitFor(() => expect(window.location.hash).toBe("#/settings/general"));
  });

  it("概览首次配置：注册热键与配置端点后徽章变为已配置", async () => {
    const adapter = createPreviewAdapter({ delayMs: 0 });
    await adapter.hotkeyCommit("warm", "CommandOrControl+Shift+F");
    await adapter.providerSave({
      id: "local",
      displayName: "本地端点",
      baseUrl: "https://api.example.com/v1",
      models: ["demo"],
      defaultGenerationModel: "demo",
      summaryModel: null,
      timeoutMs: 5000,
      apiKey: null,
    });
    go("#/console/overview");
    render(<App adapter={adapter} />);
    await waitFor(() => expect(screen.getByTestId("activation-status").textContent).toContain("快捷键"));
    await waitFor(() => expect(screen.getByTestId("provider-status").textContent).toContain("已配置"));
  });
});

describe("设置 · 模型与 API", () => {
  function saveLocalProvider(adapter: ReturnType<typeof createPreviewAdapter>) {
    return adapter.providerSave({
      id: "local",
      displayName: "本地端点",
      baseUrl: "https://api.example.com/v1",
      models: ["demo"],
      defaultGenerationModel: "demo",
      summaryModel: null,
      timeoutMs: 5000,
      apiKey: null,
    });
  }

  it("端点卡片支持编辑与清除密钥；清除后凭据徽章消失且记录保留", async () => {
    const user = userEvent.setup();
    const adapter = createPreviewAdapter({ delayMs: 0 });
    await saveLocalProvider(adapter);
    go("#/settings/models");
    render(<App adapter={adapter} />);
    await screen.findByText("本地端点");
    // 无凭据时不出现清除密钥控件。
    expect(screen.queryByTestId("provider-clear-secret-local")).toBeNull();
    // 经编辑器真实输入密钥（UI 路径，不在源码写凭据）并保存。
    await user.click(screen.getByTestId("provider-edit-local"));
    expect(screen.getByRole("dialog", { name: "模型连接" })).toBeTruthy();
    expect((screen.getByLabelText("显示名称") as HTMLInputElement).value).toBe("本地端点");
    await user.type(document.querySelector('input[type="password"]') as HTMLInputElement, "abc123");
    await user.click(screen.getByRole("button", { name: "保存端点" }));
    await waitFor(() => expect(screen.getByText("已保存凭据")).toBeTruthy());
    // 清除密钥：凭据徽章消失，端点记录仍在。
    await user.click(screen.getByTestId("provider-clear-secret-local"));
    await waitFor(() => expect(screen.getByText("无凭据")).toBeTruthy());
    expect(screen.getByText("本地端点")).toBeTruthy();
    expect(screen.queryByTestId("provider-clear-secret-local")).toBeNull();
  });

  it("默认模型卡：从端点模型列表选择并真实保存到设置", async () => {
    const user = userEvent.setup();
    const adapter = createPreviewAdapter({ delayMs: 0 });
    await saveLocalProvider(adapter);
    go("#/settings/models");
    render(<App adapter={adapter} />);
    const select = (await screen.findByLabelText("默认模型")) as HTMLSelectElement;
    expect(select.value).toBe("");
    await user.selectOptions(select, JSON.stringify(["local", "demo"]));
    await waitFor(() => expect(screen.getByText("已保存")).toBeTruthy());
    const snapshot = await adapter.bootstrap();
    expect(snapshot.settings.defaultModel).toBe(JSON.stringify(["local", "demo"]));
  });
});

describe("设置 · 外观", () => {
  it("修改主题真实保存：保存中 → 已保存，版本递增，主题生效", async () => {
    const user = userEvent.setup();
    go("#/settings/appearance");
    render(<App adapter={createPreviewAdapter({ delayMs: 0 })} />);
    const group = await screen.findByRole("radiogroup", { name: "主题" });
    await user.click(within(group).getByRole("radio", { name: "浅色" }));
    await waitFor(() => expect(screen.getByTestId("save-theme").getAttribute("data-save-state")).toBe("saved"));
    expect(group.getAttribute("data-value")).toBe("light");
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(screen.getByText(/设置版本 1/)).toBeTruthy();
  });

  it("冲突：其他窗口已修改时展示最新值与草稿，可用草稿重试", async () => {
    const user = userEvent.setup();
    const adapter = createPreviewAdapter({ delayMs: 0 });
    go("#/settings/appearance");
    render(<App adapter={adapter} />);
    const toggle = await screen.findByLabelText("透明材质");
    // Deliver the other window's write after this window freezes its revision.
    // This models a real race without depending on the 60ms event refresh timer.
    const updateSettings = adapter.updateSettings.bind(adapter);
    let race = true;
    adapter.updateSettings = async (request) => {
      if (race) {
        race = false;
        await updateSettings({ requestId: "other-window", expectedRevision: "0", patch: { theme: "system" } });
      }
      return updateSettings(request);
    };
    await user.click(toggle);
    await waitFor(() => expect(screen.getByTestId("save-transparency").getAttribute("data-save-state")).toBe("conflict"));
    expect(screen.getByTestId("save-transparency").textContent).toContain("其他窗口已修改");
    await waitFor(() => expect(screen.getByText(/设置版本 1/)).toBeTruthy());
    await user.click(screen.getByRole("button", { name: "用草稿重试透明材质" }));
    await waitFor(() => expect(screen.getByTestId("save-transparency").getAttribute("data-save-state")).toBe("saved"));
    expect(screen.getByText(/设置版本 2/)).toBeTruthy();
  });

  it("存储降级：控件禁用且提示不会保存", async () => {
    go("#/settings/appearance");
    render(<App adapter={createPreviewAdapter({ delayMs: 0, degraded: true })} />);
    const group = await screen.findByRole("radiogroup", { name: "主题" });
    expect(within(group).getAllByRole("radio").every((radio) => (radio as HTMLButtonElement).disabled)).toBe(true);
    expect(screen.getByText(/存储不可用/)).toBeTruthy();
  });
});
