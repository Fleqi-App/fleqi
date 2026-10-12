import { browser, expect } from "@wdio/globals";
import { evidencePath, writeEvidence } from "../lib/evidence";

describe("Windows · 模型/API 管理", () => {
  it("保存的凭据用于模型获取与真实调用；错误反馈、手动模型和删除确认可用", async () => {
    for (const handle of await browser.getWindowHandles()) {
      await browser.switchToWindow(handle);
      if ((await browser.getUrl()).includes("#/console")) break;
    }
    const id = `model-manager-${Date.now()}`;
    const baseUrl = `${process.env.FLEQI_TEST_MODEL_URL}/auth`;
    await browser.execute(async (providerId, url) => {
      const bridge = window.__TAURI_INTERNALS__;
      await bridge.invoke("provider_save", { request: { id: providerId, displayName: "模型管理原生回归", baseUrl: url, models: ["manual-only", "windows-fixture-model"], defaultGenerationModel: "windows-fixture-model", summaryModel: null, timeoutMs: 5000, apiKey: "fixture-key" } });
      await bridge.invoke("app_open_window", { role: "settings", page: "models" });
    }, id, baseUrl);
    const probe = await browser.execute(async (providerId, url) => {
      const bridge = window.__TAURI_INTERNALS__;
      const options = { providerId, baseUrl: url, apiKey: null, timeoutMs: 5000 };
      const models = await bridge.invoke("provider_probe", options);
      const chat = await bridge.invoke("provider_probe", { ...options, model: "windows-fixture-model" });
      const missing = await bridge.invoke("provider_probe", { ...options, model: "missing-model" });
      const wrongKey = await bridge.invoke("provider_probe", { ...options, apiKey: "wrong-key" });
      const changedUrl = await bridge.invoke("provider_probe", { ...options, baseUrl: `${url}/other` }).then(() => false, () => true);
      return { models, chat, missing, wrongKey, changedUrl };
    }, id, baseUrl) as { models: { ok: boolean; models: string[] }; chat: { ok: boolean }; missing: { ok: boolean; error: string }; wrongKey: { ok: boolean; error: string }; changedUrl: boolean };
    expect(probe.models.ok).toBe(true);
    expect(probe.models.models).toEqual(["windows-fixture-model"]);
    expect(probe.chat.ok).toBe(true);
    expect(probe.missing.ok).toBe(false);
    expect(probe.missing.error).toContain("模型不存在");
    expect(probe.wrongKey.ok).toBe(false);
    expect(probe.wrongKey.error).toContain("认证失败");
    expect(probe.changedUrl).toBe(true);
    const usage = await browser.execute(async (providerId) => {
      const bridge = window.__TAURI_INTERNALS__;
      const bootstrap = await bridge.invoke("app_bootstrap") as { settings: { revision: string } };
      return bridge.invoke("settings_update", { request: { requestId: `summary-${providerId}`, expectedRevision: bootstrap.settings.revision, patch: { summaryModel: { kind: "model", model: JSON.stringify([providerId, "windows-fixture-model"]) } } } });
    }, id) as { summaryModel: { kind: string; model: string } };
    expect(usage.summaryModel.model).toBe(JSON.stringify([id, "windows-fixture-model"]));

    await $(`[data-testid='provider-edit-${id}']`).click();
    const editor = $("[role='dialog']:has(input[type='password'])");
    await expect(editor).toBeDisplayed();
    expect(await editor.$("input[type='password']").getValue()).toBe("");
    await editor.$("button=获取模型").click();
    await expect(editor).toHaveText(expect.stringContaining("获取成功"));
    await expect(editor.$("[data-testid='model-row-manual-only']")).toBeDisplayed();
    await editor.$("button=测试模型").click();
    await expect(editor).toHaveText(expect.stringContaining("调用成功"));
    await browser.saveScreenshot(evidencePath("models-editor-windows.png"));
    await editor.$("button=保存端点").click();
    await expect(editor).not.toBeDisplayed();
    await $(`[data-testid='provider-clear-secret-${id}']`).click();
    await $("button=取消").click();
    await expect($(`[data-testid='provider-clear-secret-${id}']`)).toBeDisplayed();
    await $(`button[aria-label='删除端点 模型管理原生回归']`).click();
    await $("button=确认删除").click();
    await expect($(`[data-testid='provider-edit-${id}']`)).not.toExist();
    const list = await browser.execute(() => window.__TAURI_INTERNALS__.invoke("provider_list")) as { record: { id: string } }[];
    expect(list.some((entry) => entry.record.id === id)).toBe(false);
    writeEvidence("models-management-windows.json", { capturedAt: new Date().toISOString(), probe, deleted: true });
  });
});
