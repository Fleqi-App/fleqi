import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, it, vi } from "vitest";
import { App } from "./App";
import { createPreviewAdapter } from "./adapters/host";

it("更新失败保留重试入口，不把网络错误显示为已是最新", async () => {
  const adapter = { ...createPreviewAdapter({ delayMs: 0 }), kind: "desktop" as const };
  adapter.appUpdateCheck = vi.fn().mockRejectedValue(new Error("网络暂时不可用"));
  window.location.hash = "#/console/about";
  render(<App adapter={adapter} />);
  const button = await screen.findByRole("button", { name: "检查更新" });
  await userEvent.click(button);
  await screen.findByText(/网络暂时不可用/);
  expect(screen.queryByText("当前已是最新版本")).toBeNull();
  await waitFor(() => expect(button.hasAttribute("disabled")).toBe(false));
});

it("发现更新后由用户安装，宿主拒绝时呈现原因并允许重试", async () => {
  const adapter = { ...createPreviewAdapter({ delayMs: 0 }), kind: "desktop" as const };
  adapter.appUpdateStatus = async () => ({ phase: "available", version: "0.0.3", notes: "改进 PDF 读取", downloadedBytes: 0, totalBytes: null, error: null });
  adapter.appUpdateInstall = vi.fn().mockRejectedValue(new Error("请先结束所有会话"));
  window.location.hash = "#/console/about";
  render(<App adapter={adapter} />);
  const install = await screen.findByRole("button", { name: "安装并重启" });
  expect(adapter.appUpdateInstall).not.toHaveBeenCalled();
  await userEvent.click(install);
  await screen.findByText(/请先结束所有会话/);
  expect(adapter.appUpdateInstall).toHaveBeenCalledTimes(1);
  await waitFor(() => expect(install.hasAttribute("disabled")).toBe(false));
});
