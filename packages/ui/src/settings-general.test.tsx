import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { App } from "./App";
import { createPreviewAdapter } from "./adapters/host";

function go(hash: string) {
  window.location.hash = hash;
}

describe("设置 · 通用（M2 真实保存）", () => {
  it("关闭操作栏真实保存（FR-SET-001，barEnabled）", async () => {
    const user = userEvent.setup();
    go("#/settings/general");
    render(<App adapter={createPreviewAdapter({ delayMs: 0 })} />);
    const toggle = await screen.findByLabelText("启用操作栏");
    expect(toggle.getAttribute("aria-checked")).toBe("true");
    await user.click(toggle);
    await waitFor(() => expect(screen.getByText(/已保存/)).toBeTruthy());
    expect((await screen.findByLabelText("启用操作栏")).getAttribute("aria-checked")).toBe("false");
  });

  it("切换唤起方式与隐藏策略真实保存并递增版本", async () => {
    const user = userEvent.setup();
    go("#/settings/general");
    const adapter = createPreviewAdapter({ delayMs: 0 });
    render(<App adapter={adapter} />);
    await screen.findByLabelText("唤起方式");
    await user.click(screen.getByRole("radio", { name: "随 Finder" }));
    await waitFor(async () => expect((await adapter.bootstrap()).settings.revision).toBe("1"));
    await user.click(screen.getByRole("radio", { name: "结束全部" }));
    await waitFor(async () => expect((await adapter.bootstrap()).settings.revision).toBe("2"));
  });

  it("模型页呈现空态与编辑器（M3 真实端点管理入口）", async () => {
    go("#/settings/models");
    render(<App adapter={createPreviewAdapter({ delayMs: 0 })} />);
    expect(await screen.findByText("尚未配置端点")).toBeTruthy();
    await userEvent.setup().click(screen.getByRole("button", { name: "连接 OpenAI" }));
    expect((screen.getByLabelText(/端点地址/) as HTMLInputElement).value).toBe("https://api.openai.com/v1");
    expect(screen.getByPlaceholderText(/只写不读/)).toBeTruthy();
  });

  it("aiPolicy 真实保存（任务与诊断页）", async () => {
    const user = userEvent.setup();
    go("#/settings/tasks");
    render(<App adapter={createPreviewAdapter({ delayMs: 0 })} />);
    await screen.findByLabelText("AI 确认策略");
    await user.click(screen.getByRole("radio", { name: "全部免确认" }));
    await waitFor(() => expect(screen.getByText(/已保存/)).toBeTruthy());
  });

  it("快捷键录入：候选 → 真实注册成功；Esc 取消录入（FR-SET-002/003）", async () => {
    const user = userEvent.setup();
    go("#/settings/general");
    render(<App adapter={createPreviewAdapter({ delayMs: 0 })} />);
    await screen.findByText("快捷键与登录启动");
    // 录入中 Esc 取消。
    await user.click(screen.getByTestId("hotkey-record"));
    expect(screen.getByTestId("hotkey-recording")).toBeTruthy();
    await user.keyboard("{Escape}");
    expect(screen.queryByTestId("hotkey-recording")).toBeNull();
    // 再次录入：单键 F6 形成候选并注册成功（单键不要求修饰键）。
    await user.click(screen.getByTestId("hotkey-record"));
    await user.keyboard("{Shift>}{F6}{/Shift}");
    const candidate = await screen.findByTestId("hotkey-candidate");
    expect(candidate.textContent).toContain("F6");
    await user.click(screen.getByRole("button", { name: "注册并保存" }));
    await waitFor(() => expect(screen.getByText(/已注册并保存/)).toBeTruthy());
  });
});

describe("控制台 · 工具与任务（M3 UI）", () => {
  it("工具页：已有系统工具复用、受管包安装后可卸载（FR-TOOLS-001/003）", async () => {
    const user = userEvent.setup();
    go("#/console/tools");
    render(<App adapter={createPreviewAdapter({ delayMs: 0 })} />);
    expect(await screen.findByText("git")).toBeTruthy();
    expect(screen.getByText(/已有系统工具 \/ Homebrew 官方仓库/)).toBeTruthy();
    // 受管包：安装 → 可用 → 卸载回到未安装。
    await user.click(await screen.findByRole("button", { name: /^安装$/ }));
    await waitFor(() => expect(screen.getByText(/已安装 tesseract/)).toBeTruthy());
    await user.click(await screen.findByRole("button", { name: /^卸载 tesseract$/ }));
    await waitFor(() => expect(screen.getByText(/已卸载 tesseract/)).toBeTruthy());
  });

  it("任务页：无会话空态引导（不伪造任务）", async () => {
    go("#/console/runs");
    render(<App adapter={createPreviewAdapter({ delayMs: 0 })} />);
    expect(await screen.findByText("暂无会话")).toBeTruthy();
  });

  it("能力库：目录分组呈现且规则可增删（FR-RULE-001）", async () => {
    const user = userEvent.setup();
    go("#/console/library");
    render(<App adapter={createPreviewAdapter({ delayMs: 0 })} />);
    expect(await screen.findByText("能力目录")).toBeTruthy();
    // 目录项来自异步 catalog_query：等它到位再断言（避免与加载态竞态）。
    await waitFor(() => expect(screen.getAllByTestId("catalog-entry").length).toBeGreaterThanOrEqual(6));
    await user.type(await screen.findByPlaceholderText("规则名称"), "全局规则");
    await user.type(screen.getByPlaceholderText("规则内容（注入任务提示）"), "回答保持简洁");
    await user.click(screen.getByRole("button", { name: /添加规则/ }));
    await waitFor(() => expect(screen.getByText("全局规则")).toBeTruthy());
    await user.click(screen.getByRole("button", { name: "删除规则 全局规则" }));
    await waitFor(() => expect(screen.getByText("暂无规则")).toBeTruthy());
  });
});
