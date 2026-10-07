import { $, browser, expect } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { readFileSync, existsSync } from "node:fs";
import path from "node:path";
import type { AppBootstrap, ContextSnapshot, DiagnosticsSnapshot, DirectoryPickResult, PlanOutcome, ProviderView, RunRecord, Session, TerminalSnapshot } from "@fleqi/contracts";
import { repoRoot, evidencePath, writeEvidence, runningTestBinaryPids } from "../lib/evidence";

const phase = process.env.FLEQI_TEST_PHASE;
const fixtures = process.env.FLEQI_WINDOWS_FIXTURE_DIR!;
const providerId = `win-${path.basename(process.env.FLEQI_TEST_DATA_DIR!)}`;
const request = () => `windows-${Date.now()}-${Math.random()}`;
async function invoke<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  return browser.execute(async (name, payload) => window.__TAURI_INTERNALS__.invoke(name, payload), command, args) as Promise<T>;
}
function explorer(action: string, folder: string, file = "one.txt", windowId = 0) {
  const result = JSON.parse(execFileSync("powershell.exe", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", path.join(repoRoot, "tests/desktop/lib/windows-explorer.ps1"), "-Action", action, "-Folder", folder, "-File", file, "-WindowId", String(windowId)], { encoding: "utf8", windowsHide: true }));
  writeEvidence("windows-explorer-driver.json", result);
  return result;
}
async function context(folder: string, file: string) {
  let result!: ContextSnapshot;
  await browser.waitUntil(async () => {
    result = await invoke<ContextSnapshot>("context_refresh");
    return result.availability.kind === "available" && result.directoryRef?.displayPath.replace(/^\\\\\?\\/, "") === path.join(fixtures, folder) && result.selectedItems.length === 1 && result.selectedItems[0].displayPath.endsWith(file);
  }, { timeout: 15_000, timeoutMsg: `未读到真实 Explorer 目录和选区 ${folder}/${file}` }).catch((error) => {
    writeEvidence("windows-context-failure.json", result);
    throw new Error(`${String(error)}; last=${JSON.stringify(result)}`);
  });
  return result;
}

describe(`Windows 11 native core · ${phase}`, () => {
  let sessionId = "";
  before(async () => { await $("[data-phase='ready']").waitForExist({ timeout: 30_000 }); });

  it("宿主、原生凭据和能力支持状态真实可用", async () => {
    await browser.waitUntil(async () => (await invoke<DiagnosticsSnapshot>("diagnostics_get")).credentialStore.available, { timeout: 15_000 });
    const boot = await invoke<AppBootstrap>("app_bootstrap");
    expect(boot.buildInfo.targetOs).toBe("windows");
    expect(boot.hostState).toBe("ready");
    expect(boot.storage.state).toBe("ready");
    const entries = await invoke<Array<{ id: string; availability: string }>>("catalog_query");
    expect(entries.filter((entry) => entry.availability === "supported")).toHaveLength(21);
    expect(entries.find((entry) => entry.id === "CAP-MEDIA-001")?.availability).toBe("unsupported");
    let rejected = false;
    try { await invoke("capability_form", { capabilityId: "CAP-MEDIA-001" }); } catch { rejected = true; }
    expect(rejected).toBe(true);
    writeEvidence(`windows-${phase}-bootstrap.json`, { boot, entries });
    await browser.saveScreenshot(evidencePath(`windows-${phase}-console.png`));
    if (phase === "initial") {
      await invoke("settings_update", { request: { requestId: request(), expectedRevision: boot.settings.revision, patch: { motionMode: "reduce" } } });
      await browser.waitUntil(async () => await browser.execute(() => document.documentElement.dataset.motion) === "reduce");
      expect(await browser.execute(() => getComputedStyle(document.querySelector("button")!).transitionDuration)).toBe("0s");
    }
  });

  if (phase === "initial") {
    it("原生目录选择器返回真实目录，取消保持上下文", async () => {
      const pids = runningTestBinaryPids();
      expect(pids).toHaveLength(1);
      for (const action of ["select", "cancel"]) {
        const before = await invoke<ContextSnapshot>("context_get");
        await browser.execute((requestId) => {
          Reflect.set(window, "__pickerResult", null);
          void window.__TAURI_INTERNALS__.invoke("context_pick_directory", { requestId }).then((value) => Reflect.set(window, "__pickerResult", value));
        }, request());
        execFileSync("powershell.exe", ["-Mta", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", path.join(repoRoot, "tests/desktop/lib/windows-picker.ps1"), "-Action", action, "-HostProcessId", String(pids[0])], { env: { ...process.env, FLEQI_PICKER_TARGET: path.join(fixtures, "A 中文") }, encoding: "utf8", windowsHide: true });
        await browser.waitUntil(async () => await browser.execute(() => Reflect.get(window, "__pickerResult")) !== null, { timeout: 10_000 });
        const result = await browser.execute(() => Reflect.get(window, "__pickerResult")) as DirectoryPickResult;
        if (action === "select") {
          expect(result.kind).toBe("selected");
          if (result.kind !== "selected") throw new Error(JSON.stringify(result));
          expect(result.snapshot.source).toBe("picker");
          expect(result.snapshot.directoryRef?.displayPath.replace(/^\\\\\?\\/, "")).toBe(path.join(fixtures, "A 中文"));
        } else {
          expect(result.kind).toBe("cancelled");
          expect((await invoke<ContextSnapshot>("context_get")).id).toBe(before.id);
        }
        writeEvidence(`windows-picker-${action}.json`, result);
      }
    });

    it("手动唤起不会在下一次几何轮询中消失", async () => {
      await invoke("hotkey_commit", { requestId: request(), accelerator: "Control+Shift+F12" });
      execFileSync("powershell.exe", ["-NoProfile", "-Command", "Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.SendKeys]::SendWait('^+{F12}')"], { windowsHide: true });
      await browser.waitUntil(async () => (await invoke<{ visibility: string }>("surface_get")).visibility === "visible", { timeout: 10_000 });
      await browser.pause(1800);
      expect((await invoke<{ visibility: string }>("surface_get")).visibility).toBe("visible");
      await invoke("surface_hide");
    });

    it("读取活动 Explorer 选区，变更选区拒绝旧表单", async () => {
      const window = explorer("open", "A 中文");
      const first = await context("A 中文", "one.txt");
      expect(first.source).toBe("explorer");
      explorer("select", "A 中文", "two.txt");
      const second = await context("A 中文", "two.txt");
      expect(second.id).not.toBe(first.id);
      explorer("tab", "B '空格'", "one.txt", window.hwnd);
      const otherTab = await context("B '空格'", "one.txt");
      expect(otherTab.sourceWindowId).toBe(first.sourceWindowId);
      explorer("select", "A 中文", "two.txt", window.hwnd);
      await context("A 中文", "two.txt");
      sessionId = (await invoke<Session>("session_create", { requestId: request() })).id;
      let rejected = false;
      try { await invoke("capability_submit", { requestId: request(), sessionId, capabilityId: "CAP-FILE-003", contextId: first.id, parameters: {} }); } catch { rejected = true; }
      expect(rejected).toBe(true);
      writeEvidence("windows-explorer-context.json", { first, second, otherTab });
    });

    it("通过真实模型 HTTP、审批和 PowerShell 生成可检查文件", async () => {
      await invoke("provider_save", { request: { id: providerId, displayName: "Windows fixture", baseUrl: process.env.FLEQI_TEST_MODEL_URL, models: ["windows-fixture-model"], defaultGenerationModel: "windows-fixture-model", summaryModel: "windows-fixture-model", timeoutMs: 10000, apiKey: "synthetic-local-fixture" } });
      const snapshot = await context("A 中文", "two.txt");
      const outcome = await invoke<PlanOutcome>("run_plan_submit", { requestId: request(), sessionId, contextId: snapshot.id, prompt: "在当前目录创建测试结果" });
      expect(outcome.kind).toBe("execute");
      if (outcome.kind !== "execute") throw new Error("model did not produce a plan");
      expect(outcome.run.state).toBe("awaitingApproval");
      await invoke("run_approve", { requestId: request(), runId: outcome.run.id, planRevision: outcome.run.planRevision });
      let result!: RunRecord;
      await browser.waitUntil(async () => { result = await invoke<RunRecord>("run_get", { runId: outcome.run.id }); return ["succeeded", "failed"].includes(result.state); }, { timeout: 20_000 });
      expect(result.state).toBe("succeeded");
      expect(readFileSync(path.join(fixtures, "A 中文", "ai-result.txt"), "utf8")).toBe("verified");
      writeEvidence("windows-ai-run.json", result);
    });

    it("能力表单执行文件创建和真实目录状态", async () => {
      const snapshot = await context("A 中文", "two.txt");
      const created = await invoke<RunRecord>("capability_submit", { requestId: request(), sessionId, capabilityId: "CAP-FILE-001", contextId: snapshot.id, parameters: { name: "native.txt", content: "本地结果", encoding: "utf-8", newline: "lf" } });
      await invoke("run_approve", { requestId: request(), runId: created.id, planRevision: created.planRevision });
      await browser.waitUntil(async () => (await invoke<RunRecord>("run_get", { runId: created.id })).state === "succeeded", { timeout: 15_000 });
      expect(readFileSync(path.join(fixtures, "A 中文", "native.txt"), "utf8")).toBe("本地结果");
      const status = await invoke<RunRecord>("capability_submit", { requestId: request(), sessionId, capabilityId: "CAP-FILE-012", contextId: snapshot.id, parameters: {} });
      let result!: RunRecord;
      await browser.waitUntil(async () => { result = await invoke<RunRecord>("run_get", { runId: status.id }); return result.state === "succeeded"; }, { timeout: 10_000 });
      expect(result.output).toContain("A 中文");
      writeEvidence("windows-native-capabilities.json", { created, directoryStatus: result });
    });

    it("持续终端通过真实 IPC 执行命令", async () => {
      await invoke("terminal_open", { sessionId, cols: 100, rows: 30 });
      await browser.waitUntil(async () => (await invoke<TerminalSnapshot>("terminal_snapshot", { sessionId })).shellReadiness === "ready", { timeout: 15_000 });
      const snapshot = await invoke<ContextSnapshot>("context_get");
      await invoke("terminal_submit_line", { requestId: request(), sessionId, line: "! [IO.File]::WriteAllText((Join-Path $PWD 'manual.txt'), 'manual')", contextRevision: snapshot.revision, targetDisplay: path.join(fixtures, "A 中文") });
      await browser.waitUntil(async () => existsSync(path.join(fixtures, "A 中文", "manual.txt")), { timeout: 15_000 });
      const current = await invoke<ContextSnapshot>("context_get");
      const directoryForm = await invoke("capability_form", { capabilityId: "CAP-FILE-012", contextId: current.id });
      expect(directoryForm).toBeDefined();
    });

    it("Explorer 前台时自动跟随、切目录及移动恢复", async function () {
      let boot = await invoke<AppBootstrap>("app_bootstrap");
      await invoke("settings_update", { request: { requestId: request(), expectedRevision: boot.settings.revision, patch: { activation: "followFinder", barEnabled: false } } });
      await browser.switchToWindow("composer");
      await invoke("plugin:window|close", { label: "console" });
      const driver = explorer("select", "A 中文", "two.txt");
      if (!driver.focused) {
        writeEvidence("windows-follow-condition.json", { verified: false, reason: "在输入条禁用时，测试驱动仍无法将自建 Explorer 窗口置于前台；此项不计通过", driver });
        this.skip();
      }
      boot = await invoke<AppBootstrap>("app_bootstrap");
      await invoke("settings_update", { request: { requestId: request(), expectedRevision: boot.settings.revision, patch: { barEnabled: true } } });
      await browser.waitUntil(async () => (await invoke<{ visibility: string }>("surface_get")).visibility === "visible", { timeout: 10_000 });
      await invoke("session_select", { requestId: request(), sessionId });
      explorer("select", "B '空格'");
      await context("B '空格'", "one.txt");
      await browser.waitUntil(async () => (await invoke<TerminalSnapshot>("terminal_snapshot", { sessionId })).currentDirectory.replace(/^\\\\\?\\/, "") === path.join(fixtures, "B '空格'"), { timeout: 15_000 });
      explorer("move", "B '空格'");
      await browser.pause(700);
      expect((await invoke<{ visibility: string }>("surface_get")).visibility).toBe("visible");
      await browser.saveScreenshot(evidencePath("windows-composer.png"));
      await invoke("surface_hide");
      const sessions = await invoke<{ active: Session[] }>("session_list");
      expect(sessions.active.some((session) => session.id === sessionId)).toBe(true);
      writeEvidence("windows-follow-condition.json", { verified: true, driver });
    });
  } else {
    it("新进程恢复设置、历史和凭据，不重放命令", async () => {
      const boot = await invoke<AppBootstrap>("app_bootstrap");
      expect(boot.settings.activation).toBe("followFinder");
      const sessions = await invoke<{ active: Session[]; history: Session[] }>("session_list");
      expect(sessions.history.length).toBeGreaterThan(0);
      expect(sessions.history.some((session) => session.state === "interrupted")).toBe(true);
      const providers = await invoke<ProviderView[]>("provider_list");
      expect(providers.some((provider) => provider.record.id === providerId && provider.credentialConfigured)).toBe(true);
      expect(JSON.stringify(providers)).not.toContain("synthetic-local-fixture");
      await invoke("provider_delete", { providerId });
      writeEvidence("windows-restart.json", { boot, sessions, providers });
    });
  }
});
