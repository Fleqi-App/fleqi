import { browser, expect, $ } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { artifactsDir, repoRoot, runningTestBinaryPids, writeEvidence } from "../lib/evidence";

async function switchRole(role: string) {
  let selected = "";
  await browser.waitUntil(async () => {
    for (const handle of await browser.getWindowHandles()) {
      await browser.switchToWindow(handle);
      if ((await browser.getUrl()).includes(`#/${role}`)) { selected = handle; return true; }
    }
    return false;
  }, { timeout: 20000, timeoutMsg: `Window not found: ${role}` });
  await $("[data-phase='ready']").waitForExist({ timeout: 20000 });
  return selected;
}

describe("Dock、同名文件与输入条尺寸", () => {
  it("同名设置真实保存并替换文件，连续尺寸请求保持输入条底边", async () => {
    await $("[data-phase='ready']").waitForExist({ timeout: 30000 });
    const probe = path.join(artifactsDir, "activation-policy");
    execFileSync("swiftc", [path.join(repoRoot, "tests/desktop/lib/activation-policy.swift"), "-o", probe]);
    const pid = runningTestBinaryPids()[0]!;
    const policy = () => Number(execFileSync(probe, [String(pid)], { encoding: "utf8" }).trim());
    expect(policy()).toBe(0);
    await browser.execute(() => window.__TAURI_INTERNALS__.invoke("app_open_window", { role: "settings", page: "files" }));
    await switchRole("settings");
    await $("button=替换已有文件").click();
    await browser.waitUntil(async () => browser.execute(async () => (await window.__TAURI_INTERNALS__.invoke("app_bootstrap") as { settings: { nameConflict: string } }).settings.nameConflict === "overwrite"));
    expect(await $("body").getText()).not.toContain("unknown variant");
    const fixture = JSON.parse(fs.readFileSync("/tmp/fleqi-attach-finder.json", "utf8")) as { folder: string };
    const target = path.join(fixture.folder, "conflict.txt");
    fs.writeFileSync(target, "old file");
    const submitted = await browser.execute(async () => {
      const api = window.__TAURI_INTERNALS__;
      await api.invoke("context_refresh");
      const form = await api.invoke("capability_form", { capabilityId: "CAP-TEXT-002" }) as { context: { id: string } };
      const session = await api.invoke("session_create", { requestId: "conflict-session" }) as { id: string };
      const run = await api.invoke("capability_submit", { requestId: "conflict-run", sessionId: session.id, contextId: form.context.id, capabilityId: "CAP-TEXT-002", parameters: { name: "conflict.txt", content: "new file", encoding: "utf-8", newline: "lf" } }) as { id: string; planRevision: string; state: string };
      return run;
    });
    expect(submitted.state).toBe("awaitingApproval");
    expect(fs.readFileSync(target, "utf8")).toBe("old file");
    await browser.execute((run: { id: string; planRevision: string }) => window.__TAURI_INTERNALS__.invoke("run_approve", { requestId: "conflict-approve", runId: run.id, planRevision: run.planRevision }), submitted);
    await browser.waitUntil(async () => fs.readFileSync(target, "utf8") === "new file");
    expect(fs.existsSync(path.join(fixture.folder, "conflict (1).txt"))).toBe(false);
    await $("button=保留两份").click();
    await browser.waitUntil(async () => browser.execute(async () => (await window.__TAURI_INTERNALS__.invoke("app_bootstrap") as { settings: { nameConflict: string } }).settings.nameConflict === "uniqueName"));
    await browser.saveScreenshot(path.join(artifactsDir, "name-conflict-settings.png"));
    await browser.execute(async () => {
      const api = window.__TAURI_INTERNALS__;
      await api.invoke("hotkey_commit", { requestId: "layout-hotkey", accelerator: "CommandOrControl+Shift+F" });
      await api.invoke("surface_show");
    });
    const composer = await switchRole("composer");
    const geometry = () => browser.execute(async () => {
      const api = window.__TAURI_INTERNALS__;
      const scale = await api.invoke("plugin:window|scale_factor") as number;
      const p = await api.invoke("plugin:window|outer_position") as { y: number };
      const size = await api.invoke("plugin:window|outer_size") as { height: number };
      return { bottom: (p.y + size.height) / scale, height: size.height / scale };
    });
    await browser.waitUntil(async () => (await geometry()).height === 72);
    const initial = await geometry();
    const bursts = [];
    for (let index = 0; index < 5; index++) {
      await browser.execute(async () => {
        const api = window.__TAURI_INTERNALS__;
        await Promise.all([360, 0, 160, 360, 0, 320].map((extraHeight) => api.invoke("surface_layout", { extraHeight })));
        await api.invoke("surface_layout", { extraHeight: 0 });
      });
      await browser.waitUntil(async () => (await geometry()).height === 72);
      const current = await geometry();
      expect(Math.abs(current.bottom - initial.bottom)).toBeLessThanOrEqual(1);
      bursts.push(current);
    }
    await switchRole("settings");
    await browser.closeWindow();
    expect(policy()).toBe(0); // console is still open
    await switchRole("console");
    await browser.closeWindow();
    await browser.switchToWindow(composer);
    await browser.waitUntil(async () => policy() === 1); // background utility
    await browser.execute(() => window.__TAURI_INTERNALS__.invoke("app_open_window", { role: "console" }));
    await switchRole("console");
    await browser.waitUntil(async () => policy() === 0);
    writeEvidence("launch-conflict.json", { submitted, output: fs.readFileSync(target, "utf8"), initial, bursts, foregroundPolicy: 0, backgroundPolicy: 1, reopenedPolicy: policy() });
  });
});
