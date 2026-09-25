import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi, afterEach } from "vitest";
import { App } from "./App";
import { createPreviewAdapter } from "./adapters/host";

function desktopStub() {
  document.documentElement.dataset.chrome = "native";
  return Object.assign(createPreviewAdapter({ delayMs: 0 }), { kind: "desktop" as const, windowControl: vi.fn().mockResolvedValue(undefined) });
}
afterEach(() => { delete document.documentElement.dataset.chrome; });
describe("原生窗口控制", () => {
  it("macOS 控制台不绘制第二套红绿灯，内容滚动区不作为拖动区", async () => {
    window.location.hash = "#/console";
    render(<App adapter={desktopStub()} />);
    await waitFor(() => expect(screen.getByTestId("breadcrumb-page")).toBeDefined());
    expect(screen.queryByTestId("traffic-close")).toBeNull();
    expect(screen.queryByTestId("traffic-minimize")).toBeNull();
    expect(screen.queryByTestId("traffic-maximize")).toBeNull();
    expect(document.querySelectorAll("[data-tauri-drag-region]").length).toBe(1);
    expect(document.querySelector("main")?.hasAttribute("data-tauri-drag-region")).toBe(false);
  });
  it("输入条隐藏只调用产品隐藏动作，不销毁窗口", async () => {
    window.location.hash = "#/composer";
    const adapter = desktopStub();
    const hide = vi.spyOn(adapter, "surfaceHide");
    render(<App adapter={adapter} />);
    await screen.findByTestId("composer");
    await userEvent.setup().click(screen.getByTestId("traffic-close"));
    expect(hide).toHaveBeenCalledOnce();
    expect(adapter.windowControl).not.toHaveBeenCalled();
  });
});
