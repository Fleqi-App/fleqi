import { $, browser, expect } from "@wdio/globals";
import { evidencePath } from "../lib/evidence";

describe("主窗口内的设置", () => {
  it("设置弹层保存真实配置，关闭后回到原页面，宿主入口复用主窗口", async () => {
    await $("[data-phase='ready']").waitForExist({ timeout: 30_000 });
    await browser.execute(async () => {
      const boot = await window.__TAURI_INTERNALS__.invoke("app_bootstrap") as { settings: { revision: string } };
      await window.__TAURI_INTERNALS__.invoke("settings_update", { request: { requestId: `settings-modal-motion-${Date.now()}`, expectedRevision: boot.settings.revision, patch: { motionMode: "reduce" } } });
    });
    await $("button[aria-label='关于与更新']").click();
    await expect($("[data-testid='breadcrumb-page']")).toHaveText("关于与更新");
    await expect($("[data-field='diag-host-state']")).toHaveText("就绪");
    const consoleHandle = await browser.getWindowHandle();
    const initialHandles = await browser.getWindowHandles();
    await $("button[aria-label='设置']").click();
    const dialog = $("[role='dialog']");
    await dialog.waitForDisplayed();
    expect(await browser.getWindowHandle()).toBe(consoleHandle);
    expect(await browser.getWindowHandles()).toEqual(initialHandles);
    await expect(dialog.$("[data-testid='breadcrumb-page']")).toHaveText("外观");
    await dialog.$("#theme button[data-value='light']").click();
    await browser.waitUntil(async () => (await $("html").getAttribute("data-theme")) === "light");
    await browser.saveScreenshot(evidencePath("settings-dialog-windows.png"));
    await dialog.$("[data-slot='dialog-close']").click();
    await dialog.waitForDisplayed({ reverse: true });
    await expect($("[data-testid='breadcrumb-page']")).toHaveText("关于与更新");
    await browser.execute(() => window.__TAURI_INTERNALS__.invoke("app_open_window", { role: "settings", page: "models" }));
    await $("[role='dialog']").waitForDisplayed();
    await expect($("[role='dialog'] [data-testid='breadcrumb-page']")).toHaveText("模型与 API");
    expect(await browser.getWindowHandles()).toEqual(initialHandles);
    await browser.keys("Escape");
    await $("[role='dialog']").waitForDisplayed({ reverse: true });
    await expect($("[data-testid='breadcrumb-page']")).toHaveText("关于与更新");
    await browser.execute(() => window.__TAURI_INTERNALS__.invoke("app_open_window", { role: "composer" }));
    let composer = "";
    await browser.waitUntil(async () => {
      for (const handle of await browser.getWindowHandles()) {
        await browser.switchToWindow(handle);
        if ((await browser.getUrl()).includes("#/composer")) { composer = handle; return true; }
      }
      return false;
    });
    await browser.switchToWindow(consoleHandle);
    await browser.closeWindow();
    await browser.switchToWindow(composer);
    await $("[data-phase='ready']").waitForExist();
    await browser.execute(() => window.__TAURI_INTERNALS__.invoke("app_open_window", { role: "settings", page: "files" }));
    await browser.waitUntil(async () => (await browser.getWindowHandles()).length === 2);
    await browser.switchToWindow((await browser.getWindowHandles()).find((handle) => handle !== composer)!);
    await $("[role='dialog']").waitForDisplayed();
    expect(await $("[data-phase='ready']").getAttribute("data-window-role")).toBe("console");
    await expect($("[role='dialog'] [data-testid='breadcrumb-page']")).toHaveText("文件与工具");
  });
});
