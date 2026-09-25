import { $, $$, browser, expect } from "@wdio/globals";
import { evidencePath, hostFacts, writeEvidence } from "../lib/evidence";

const htmlAttr = (name: string) => $("html").getAttribute(name);

describe("M1.6 · 启动、自检与上下文（真实 WKWebView + 真实 IPC）", () => {
  it("控制台窗口加载 app_bootstrap：宿主 ready、存储正常 schema 5、设置已持久化", async () => {
    await $("[data-phase='ready']").waitForExist({ timeout: 30_000 });
    expect((await browser.getUrl()).startsWith("tauri://localhost")).toBe(true);
    expect(await $("[data-phase='ready']").getAttribute("data-window-role")).toBe("console");
    expect(await $("[data-testid='host-kind']").getAttribute("data-host-kind")).toBe("desktop");
    await browser.waitUntil(async () => (await $("[data-testid='host-state']").getAttribute("data-host-state")) === "ready", {
      timeout: 20_000,
      timeoutMsg: "宿主未在 20s 内进入 ready",
    });
    expect(await $("[data-testid='storage-state']").getAttribute("data-storage-state")).toBe("ready");
    expect(await $("[data-testid='storage-state']").getText()).toBe("存储正常");
    expect(await $("[data-field='settings-revision']").getText()).toBe("0");
    expect(await $("[data-field='version']").getText()).toBe("0.0.2");
    const boot = await browser.execute(async () => window.__TAURI_INTERNALS__.invoke("app_bootstrap")) as Record<string, unknown>;
    expect(boot).toMatchObject({ storage: { schemaVersion: 5 }, buildInfo: { version: "0.0.2", stage: "BETA" } });
    const capabilities = await $$("[data-testid='platform-capabilities'] li");
    const count = await capabilities.length;
    expect(count).toBe(4);
    const capabilityStates: Array<{ id: string | null; state: string | null }> = [];
    for (let i = 0; i < count; i += 1) {
      const li = await capabilities[i];
      capabilityStates.push({ id: await li.getAttribute("data-capability"), state: await li.getAttribute("data-state") });
    }
    expect(await $("[data-capability='credentialStore']").getAttribute("data-state")).toBe("supported");
    expect(await $("[data-capability='directoryPicker']").getAttribute("data-state")).toBe("supported");
    expect(await htmlAttr("data-theme")).toBe("dark");

    await browser.saveScreenshot(evidencePath("m1-console-overview.png"));
    writeEvidence("m1-bootstrap-evidence.json", {
      capturedAt: new Date().toISOString(),
      url: await browser.getUrl(),
      hostState: "ready",
      storage: await $("[data-testid='storage-state']").getText(),
      settingsRevision: "0",
      version: await $("[data-field='version']").getText(),
      capabilities: capabilityStates,
      host: hostFacts(),
      note: "字段来自真实 IPC app_bootstrap 渲染结果；权限状态为本机 TCC 事实（无提示检测）。",
    });
  });

  it("权限页：无提示重检返回明确状态；Finder 上下文可用性与权限事实一致", async () => {
    await $("button[data-sidebar='menu-button']*=权限与自检").click();
    await $("h2*=权限与自检").waitForExist();
    await $("button*=重新检查全部").click();
    await browser.waitUntil(async () => !(await $("[data-testid='status-finderAutomation']").getText()).includes("正在检查"), { timeout: 20_000 });
    const finderStatus = await $("[data-permission='finderAutomation']").getAttribute("data-status");
    const axStatus = await $("[data-permission='accessibility']").getAttribute("data-status");
    expect(["allowed", "needsConsent", "denied", "targetNotRunning", "failed"]).toContain(finderStatus);
    expect(["allowed", "needsConsent", "denied", "failed"]).toContain(axStatus);

    await $("button[aria-label='刷新 Finder 上下文']").click();
    await $("[data-testid='context-snapshot']").waitForExist({ timeout: 20_000 });
    const availability = await $("[data-testid='context-snapshot']").getAttribute("data-availability");
    if (finderStatus === "allowed") {
      expect(["available", "noDirectory", "selectionOverLimit", "finderNotRunning", "failed"]).toContain(availability);
    } else if (finderStatus === "targetNotRunning") {
      expect(availability).toBe("finderNotRunning");
    } else {
      expect(["permissionRequired", "finderNotRunning", "failed"]).toContain(availability);
    }
    const directory = await $("[data-field='context-directory']").getText();
    if (availability === "available") {
      expect(directory.startsWith("/")).toBe(true);
    } else {
      expect(directory).toBe("—");
    }
    await browser.saveScreenshot(evidencePath("m1-console-permissions.png"));
    writeEvidence("m1-permissions-context-evidence.json", {
      capturedAt: new Date().toISOString(),
      finderAutomation: finderStatus,
      accessibility: axStatus,
      contextAvailability: availability,
      contextDirectory: directory,
      note: "显式申请与目录选择面板需要用户交互，属于显式原生验收模式，不在无人值守运行中触发系统对话框。",
    });
  });

  it("关于页：诊断只含安全字段并反映真实存储与凭据自检", async () => {
    await $("button[data-sidebar='menu-button']*=关于与更新").click();
    const updateStatus = await browser.execute(async () => window.__TAURI_INTERNALS__.invoke("app_update_status")) as Record<string, unknown>;
    expect(updateStatus).toMatchObject({ phase: "idle", version: null, error: null });
    expect(await $("button=检查更新").isExisting()).toBe(true);
    const iconLoaded = await browser.execute(() => {
      const icon = document.querySelector<HTMLImageElement>("img[alt='Fleqi']");
      return !!icon?.complete && icon.naturalWidth === 128;
    });
    expect(iconLoaded).toBe(true);
    await $("summary=诊断信息").click();
    await $("[data-field='diag-host-state']").waitForExist({ timeout: 20_000 });
    expect(await $("[data-field='diag-host-state']").getText()).toBe("就绪");
    expect(await $("[data-field='diag-storage']").getText()).toContain("schema 5");
    expect(await $("[data-field='diag-credentials']").getText()).toContain("可用");
    expect(await $("[data-field='diag-credentials']").getText()).toContain("app.fleqi.desktop.test");
    await browser.saveScreenshot(evidencePath("m1-console-about.png"));
  });
});
