import { describe, expect, it } from "vitest";
import { createHostAdapter, PREVIEW_FAILURE } from "./adapters/host";
import { parseRoute, routeHash } from "./router";
import { applyAppearance, resolveProductTheme, resolveTheme } from "./theme";

function fakeWindow(search: string, withBridge: boolean): Window {
  const target = { location: { search } } as unknown as Window & { __TAURI_INTERNALS__?: object };
  if (withBridge) target.__TAURI_INTERNALS__ = {};
  return target;
}

describe("宿主适配器选择", () => {
  it("存在 Tauri 桥时选择 desktop", () => {
    expect(createHostAdapter(fakeWindow("", true)).kind).toBe("desktop");
  });

  it("没有桥时选择 preview，且 ?preview=fail 模拟失败", async () => {
    const preview = createHostAdapter(fakeWindow("", false));
    expect(preview.kind).toBe("preview");
    const failing = createHostAdapter(fakeWindow("?preview=fail", false));
    await expect(failing.getBuildInfo()).rejects.toEqual(PREVIEW_FAILURE);
  });
});

describe("主题解析与外观应用", () => {
  it("默认深色，?theme=light 为浅色预览", () => {
    expect(resolveTheme("")).toBe("dark");
    expect(resolveTheme("?theme=light")).toBe("light");
  });

  it("产品主题 system 跟随系统；reduce 只能更少", () => {
    expect(resolveProductTheme("system", true)).toBe("dark");
    expect(resolveProductTheme("system", false)).toBe("light");
    expect(resolveProductTheme("light", true)).toBe("light");
    const root = document.createElement("div");
    applyAppearance({ theme: "system", transparency: false, motionMode: "reduce" }, root, (() => ({ matches: false })) as unknown as typeof window.matchMedia);
    expect(root.dataset.theme).toBe("light");
    expect(root.dataset.transparency).toBe("off");
    expect(root.dataset.motion).toBe("reduce");
  });
});

describe("哈希路由", () => {
  it("解析窗口角色与页面并给默认页", () => {
    expect(parseRoute("")).toEqual({ window: "console", page: "overview" });
    expect(parseRoute("#/console")).toEqual({ window: "console", page: "overview" });
    expect(parseRoute("#/settings")).toEqual({ window: "settings", page: "appearance" });
    expect(parseRoute("#/settings/about")).toEqual({ window: "settings", page: "about" });
    expect(parseRoute("#/unknown/x")).toEqual({ window: "console", page: "overview" });
    expect(routeHash("settings", "general")).toBe("#/settings/general");
  });
});
