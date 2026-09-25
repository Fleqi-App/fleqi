import { browser, expect, $ } from "@wdio/globals";
import { createServer, type ServerResponse } from "node:http";
import path from "node:path";
import { artifactsDir, writeEvidence } from "../lib/evidence";

describe("固定看板、转换偏好与统一任务浮层", () => {
  (process.env.FLEQI_VERIFY_TOOL_PREPARATION ? it : it.skip)("真实工具准备只补装缺失项，完成后所有内建工具可用", async function () {
    this.timeout(300000);
    await $("[data-phase='ready']").waitForExist({ timeout: 30_000 });
    const before = await browser.execute(() => window.__TAURI_INTERNALS__.invoke("tools_list")) as { manifest: { id: string }; status: { kind: string; path?: string } }[];
    await browser.execute(() => window.__TAURI_INTERNALS__.invoke("tools_prepare"));
    let final: { running: boolean; completed: number; total: number; errors: string[] } | null = null;
    await browser.waitUntil(async () => {
      final = await browser.execute(() => window.__TAURI_INTERNALS__.invoke("tools_prepare_status")) as typeof final;
      return final !== null && !final.running;
    }, { timeout: 270000, interval: 800 });
    const after = await browser.execute(() => window.__TAURI_INTERNALS__.invoke("tools_list")) as typeof before;
    writeEvidence("workflow-tool-preparation.json", { before, after, final });
    expect(final!.errors).toEqual([]);
    expect(after.every((tool) => tool.status.kind === "available")).toBe(true);
    for (const existing of before.filter((tool) => tool.status.kind === "available")) {
      expect(after.find((tool) => tool.manifest.id === existing.manifest.id)!.status.path).toBe(existing.status.path);
    }
  });
  it("真实设置驱动转换表单；规划阻止发送但保留草稿，结果占满同一右侧面板", async () => {
    await $("[data-phase='ready']").waitForExist({ timeout: 30_000 });
    const board = await browser.execute(() => {
      const main = document.querySelector<HTMLElement>("[data-fixed-board]")!;
      return { height: main.clientHeight, scrollHeight: main.scrollHeight, cards: document.querySelectorAll("[data-testid='capability-category']").length };
    });
    expect(board.scrollHeight).toBeLessThanOrEqual(board.height);
    expect(board.cards).toBe(6);
    await browser.saveScreenshot(path.join(artifactsDir, "workflow-board.png"));
    let pending: ServerResponse | null = null;
    const server = createServer((request, response) => {
      request.resume();
      request.on("end", () => { pending = response; });
    });
    await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
    const port = (server.address() as { port: number }).port;
    try {
      const saved = await browser.execute(async (baseUrl: string) => {
        const api = window.__TAURI_INTERNALS__;
        const bootstrap = await api.invoke("app_bootstrap") as { settings: { revision: string } };
        await api.invoke("settings_update", { request: { requestId: "workflow-conversion", expectedRevision: bootstrap.settings.revision, patch: { conversionSourceHandling: "trashAfterSuccess" } } });
        await api.invoke("context_refresh");
        const form = await api.invoke("capability_form", { capabilityId: "CAP-MEDIA-002" }) as { fields: { key: string; defaultValue: string; choices: string[] }[] };
        await api.invoke("provider_save", { request: { id: "workflow-local", displayName: "本地验收", baseUrl, models: ["fixture"], defaultGenerationModel: "fixture", summaryModel: null, timeoutMs: 30000, apiKey: null } });
        await api.invoke("hotkey_commit", { requestId: "workflow-hotkey", accelerator: "CommandOrControl+Shift+F" });
        await api.invoke("surface_show");
        return form;
      }, `http://127.0.0.1:${port}/v1`);
      expect(saved.fields.find((field) => field.key === "sourceHandling")!.defaultValue).toBe("trashAfterSuccess");
      expect(saved.fields.find((field) => field.key === "format")!.choices).toContain("mov");
      await browser.waitUntil(async () => {
        for (const handle of await browser.getWindowHandles()) { await browser.switchToWindow(handle); if ((await browser.getUrl()).includes("#/composer")) return true; }
        return false;
      }, { timeout: 20000 });
      await browser.execute(() => window.__TAURI_INTERNALS__.invoke("plugin:window|set_focus"));
      await $("[data-testid='composer-input']").setValue("显示验证文字");
      await $("[data-testid='composer-submit']").click();
      await $("[data-testid='planning-status']").waitForExist();
      try { await browser.waitUntil(async () => browser.execute(() => getComputedStyle(document.querySelector("[data-testid='task-panel']")!).opacity === "1"), { timeout: 10000 }); }
      finally { writeEvidence("workflow-panel-state.json", await browser.execute(() => {
        const panel = document.querySelector("[data-testid='task-panel']")!;
        return { parent: panel.parentElement?.outerHTML.slice(0, 500), opacity: getComputedStyle(panel).opacity, transform: getComputedStyle(panel).transform, visibility: document.visibilityState, focused: document.hasFocus(), animations: panel.getAnimations().map((animation) => ({ time: animation.currentTime, playState: animation.playState })), height: innerHeight, style: getComputedStyle(panel).cssText };
      })); }
      expect(await $("[data-testid='composer-submit']").isEnabled()).toBe(false);
      await $("[data-testid='composer-input']").setValue("提前准备的下一条");
      const planning = await browser.execute(() => {
        const panel = document.querySelector("[data-testid='task-panel']")!.getBoundingClientRect();
        const input = document.querySelector("[data-testid='composer-input']")!;
        return { left: panel.left, width: panel.width, right: panel.right, windowWidth: innerWidth, borderColor: getComputedStyle(input).borderColor, planning: document.querySelector("[data-testid='composer']")!.getAttribute("data-planning") };
      });
      expect(planning.planning).toBe("true");
      expect(planning.width).toBeGreaterThan(500);
      expect(planning.width).toBeLessThanOrEqual(620);
      expect(Math.abs(planning.windowWidth - planning.right - 8)).toBeLessThanOrEqual(1);
      await browser.saveScreenshot(path.join(artifactsDir, "workflow-planning.png"));
      await browser.waitUntil(async () => pending !== null, { timeout: 10000 });
      const content = JSON.stringify({ scripts: ["printf 'fleqi-workflow-output\\n'"], effects: ["read"], previewComplete: true });
      (pending as unknown as ServerResponse).writeHead(200, { "Content-Type": "text/event-stream" });
      (pending as unknown as ServerResponse).end(`data: ${JSON.stringify({ choices: [{ delta: { content } }] })}\n\ndata: [DONE]\n\n`);
      // The policy can require confirmation for an inline script with escaped
      // characters even when its model-declared effect is read-only.
      await $("button=确认执行").waitForExist({ timeout: 15000 });
      await $("button=确认执行").click();
      await $("[data-testid='task-panel-output']").waitForExist({ timeout: 15000 });
      expect(await $("[data-testid='task-panel-output']").getText()).toContain("fleqi-workflow-output");
      expect(await $("[data-testid='composer-input']").getValue()).toBe("提前准备的下一条");
      expect(await $("[data-testid='composer-submit']").isEnabled()).toBe(true);
      expect(await $("[data-testid='planning-status']").isExisting()).toBe(false);
      await browser.saveScreenshot(path.join(artifactsDir, "workflow-output.png"));
      writeEvidence("workflow.json", { board, saved, planning, result: "real printf output and draft preserved" });
    } finally { server.closeAllConnections(); await new Promise<void>((resolve) => server.close(() => resolve())); }
  });
});
