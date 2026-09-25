import { browser, expect, $ } from "@wdio/globals";
import { writeEvidence, artifactsDir } from "../lib/evidence";
import path from "node:path";
import { execFileSync } from "node:child_process";
import type { CapabilityForm, RunRecord } from "@fleqi/contracts";
import fs from "node:fs";

describe("UI 实际窗口回归", () => {
  it("挂载不创建额外会话；面板可见并在关闭后恢复 72px", async () => {
    await $("[data-phase='ready']").waitForExist({ timeout: 30_000 });
    const shown = await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      await bridge.invoke("context_refresh");
      await bridge.invoke("hotkey_commit", { requestId: "ui-hotkey", accelerator: "CommandOrControl+Shift+F" });
      return await bridge.invoke("surface_show") as { visibleSessionId: string };
    });
    await browser.waitUntil(async () => {
      for (const handle of await browser.getWindowHandles()) {
        await browser.switchToWindow(handle);
        if ((await browser.getUrl()).includes("#/composer")) return true;
      }
      return false;
    }, { timeout: 30_000 });
    await $("[data-testid='composer']").waitForExist({ timeout: 20_000 });
    const before = await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      return { surface: await bridge.invoke("surface_get"), sessions: await bridge.invoke("session_list", { offset: 0, limit: 50 }) } as { surface: { visibleSessionId: string }; sessions: { active: unknown[] } };
    });
    expect(before.surface.visibleSessionId).toBe(shown.visibleSessionId);
    expect(before.sessions.active.length).toBe(1);
    await $("button[aria-label='会话选择器']").click();
    await browser.waitUntil(async () => browser.execute(() => window.innerHeight > 72), { timeout: 10_000 });
    const expanded = await browser.execute(() => {
      const panel = document.querySelector("[data-testid='session-selector']")!.getBoundingClientRect();
      const bar = document.querySelector("[data-testid='composer']")!.getBoundingClientRect();
      return { height: window.innerHeight, panel: { top: panel.top, bottom: panel.bottom, width: panel.width }, bar: { top: bar.top, bottom: bar.bottom } };
    });
    expect(expanded.panel.top).toBeGreaterThanOrEqual(0);
    expect(expanded.panel.bottom).toBeLessThanOrEqual(expanded.bar.top);
    expect(expanded.bar.bottom).toBeLessThanOrEqual(expanded.height);
    await browser.saveScreenshot(path.join(artifactsDir, "ui-session-panel.png"));
    await $("button[aria-label='关闭会话选择器']").click();
    await browser.waitUntil(async () => browser.execute(() => window.innerHeight === 72), { timeout: 10_000 });
    await $("button[aria-label='打开终端面板']").click();
    await $("[data-testid='terminal-panel'][data-status='running']").waitForExist({ timeout: 20_000 });
    await browser.waitUntil(async () => (await $("[data-testid='terminal-readiness']").getText()).includes("可输入"), { timeout: 20_000 });
    await browser.waitUntil(async () => browser.execute(async (sessionId: string) => {
      const snapshot = await window.__TAURI_INTERNALS__.invoke("terminal_snapshot", { sessionId }) as { size: { rows: number } };
      return snapshot.size.rows > 5;
    }, shown.visibleSessionId), { timeout: 10_000 });
    // embedded driver 的 keys 会在 keydown 已取消后仍发 input，并用小写 ASCII
    // 当 keyCode，导致 xterm 重复输入/误认功能键。这里驱动真实粘贴入口及 Enter，
    // 不 mock IPC、xterm onData 或 PTY；逐键物理输入与 IME 留人工走查。
    await browser.execute(() => {
      const input = document.querySelector<HTMLTextAreaElement>(".xterm-helper-textarea")!;
      input.focus();
      const clipboardData = new DataTransfer();
      clipboardData.setData("text/plain", "printf '\\146\\154\\145\\161\\151\\055\\165\\151\\055\\157\\153\\n'");
      input.dispatchEvent(new ClipboardEvent("paste", { clipboardData, bubbles: true, cancelable: true }));
      input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", code: "Enter", keyCode: 13, which: 13, bubbles: true, cancelable: true }));
    });
    try { await browser.waitUntil(async () => browser.execute(async (sessionId: string) => {
      const snapshot = await window.__TAURI_INTERNALS__.invoke("terminal_snapshot", { sessionId }) as { screen: string };
      return snapshot.screen.includes("fleqi-ui-ok");
    }, shown.visibleSessionId), { timeout: 15_000 }); } finally {
      const inputEvidence = await browser.execute(async (sessionId: string) => ({
        snapshot: await window.__TAURI_INTERNALS__.invoke("terminal_snapshot", { sessionId }),
        focus: document.activeElement?.outerHTML,
        error: document.querySelector("[data-testid='terminal-panel'] [data-tone='error']")?.textContent,
      }), shown.visibleSessionId);
      writeEvidence("ui-terminal-input.json", inputEvidence);
      await browser.saveScreenshot(path.join(artifactsDir, "ui-terminal-input.png"));
    }
    const terminal = await browser.execute(() => {
      const rect = document.querySelector("[data-testid='terminal-output']")!.getBoundingClientRect();
      const panel = document.querySelector("[data-testid='terminal-panel']")!;
      const bounds = panel.getBoundingClientRect();
      const close = panel.querySelector("button[aria-label='收起终端']")!;
      const text = panel.querySelector(".xterm-rows span")!;
      return { top: rect.top, bottom: rect.bottom, height: rect.height, viewport: window.innerHeight,
        motion: { product: document.documentElement.dataset.motion, systemReduced: matchMedia("(prefers-reduced-motion: reduce)").matches, duration: getComputedStyle(document.documentElement).getPropertyValue("--fleqi-motion-feedback") },
        foreground: getComputedStyle(text).color, background: getComputedStyle(panel.querySelector(".xterm")!).backgroundColor,
        fontFamily: getComputedStyle(text).fontFamily,
        styles: [...panel.querySelectorAll<HTMLStyleElement>("style")].map((style) => ({ noncePresent: !!style.nonce, sheetPresent: !!style.sheet, rules: style.sheet?.cssRules.length, bytes: style.textContent?.length })),
        terminalStylesAuthorized: [...panel.querySelectorAll<HTMLStyleElement>("style")].filter((style) => !!style.textContent).every((style) => !!style.nonce && !!style.sheet?.cssRules.length),
        leftInset: rect.left - bounds.left, rightInset: bounds.right - rect.right, bottomInset: bounds.bottom - rect.bottom,
        closeOffset: close.getBoundingClientRect().left - bounds.left, redClose: close.classList.contains("traffic-close") };
    });
    writeEvidence("ui-terminal-style.json", terminal);
    expect(terminal.height).toBeGreaterThan(100);
    expect(terminal.top).toBeGreaterThanOrEqual(0);
    expect(terminal.bottom).toBeLessThanOrEqual(terminal.viewport - 72);
    expect(terminal.foreground).toBe("rgb(243, 244, 246)");
    expect(terminal.background).toBe("rgb(24, 25, 27)");
    expect(terminal.fontFamily).toContain("monospace");
    expect(terminal.terminalStylesAuthorized).toBe(true);
    expect(Math.min(terminal.leftInset, terminal.rightInset, terminal.bottomInset)).toBeGreaterThanOrEqual(12);
    expect(terminal.redClose).toBe(true);
    expect(terminal.closeOffset).toBeLessThan(32);
    await browser.saveScreenshot(path.join(artifactsDir, "ui-terminal-panel.png"));
    const exitFrames = await browser.execute(async () => {
      const frames: { time: number; bottom: number; height: number; opacity: number | null }[] = [];
      const start = performance.now();
      const api = window.__TAURI_INTERNALS__;
      const scale = await api.invoke("plugin:window|scale_factor") as number;
      document.querySelector<HTMLButtonElement>("button[aria-label='收起终端']")!.click();
      await new Promise<void>((resolve) => {
        const sample = async () => {
          // WKWebView screenY is always zero; read the real native frame instead.
          const position = await api.invoke("plugin:window|outer_position") as { y: number };
          const size = await api.invoke("plugin:window|outer_size") as { height: number };
          const positionAfter = await api.invoke("plugin:window|outer_position") as { y: number };
          const panel = document.querySelector("[data-testid='terminal-panel']");
          // Discard an IPC round trip spanning the resize, whose old position and
          // new WebView height do not describe any single displayed frame.
          if (position.y === positionAfter.y && Math.abs(size.height / scale - window.innerHeight) <= 1) frames.push({ time: performance.now() - start, bottom: position.y / scale + size.height / scale, height: window.innerHeight, opacity: panel ? Number(getComputedStyle(panel).opacity) : null });
          if (performance.now() - start < 400) requestAnimationFrame(sample); else resolve();
        };
        requestAnimationFrame(sample);
      });
      return frames;
    });
    await browser.waitUntil(async () => browser.execute(() => window.innerHeight === 72), { timeout: 10_000 });
    writeEvidence("ui-window-regressions.json", { shown, before, expanded, terminal, exitFrames });
    const bottoms = exitFrames.map((frame) => frame.bottom);
    expect(Math.max(...bottoms) - Math.min(...bottoms)).toBeLessThanOrEqual(2);
    expect(exitFrames.at(-1)?.opacity).toBeNull();
    if (terminal.motion.product !== "reduce" && !terminal.motion.systemReduced) {
      expect(exitFrames.some((frame) => frame.opacity !== null && frame.opacity > 0 && frame.opacity < .99)).toBe(true);
      expect(exitFrames.find((frame) => frame.opacity === null)!.time).toBeGreaterThanOrEqual(100);
    }
  });
  it("目录查询读取真实 Finder 与忙碌 PTY 状态，不向程序注入 pwd", async () => {
    const state = await browser.execute(async () => {
      const api = window.__TAURI_INTERNALS__;
      const surface = await api.invoke("surface_get") as { visibleSessionId: string };
      const context = await api.invoke("context_get") as { id: string; directoryRef: { displayPath: string } };
      const lease = await api.invoke("terminal_acquire_lease", { sessionId: surface.visibleSessionId, owner: "directory-probe" }) as string;
      await api.invoke("terminal_input", { sessionId: surface.visibleSessionId, lease, input: Array.from(new TextEncoder().encode("sleep 20\r")) });
      return { sessionId: surface.visibleSessionId, context, lease };
    });
    try {
      await browser.waitUntil(async () => browser.execute(async (sessionId: string) => {
        const snapshot = await window.__TAURI_INTERNALS__.invoke("terminal_snapshot", { sessionId }) as { shellReadiness: string };
        return snapshot.shellReadiness === "busy";
      }, state.sessionId), { timeout: 5000 });
      const before = await browser.execute((sessionId: string) => window.__TAURI_INTERNALS__.invoke("terminal_snapshot", { sessionId }), state.sessionId) as { currentDirectory: string; screen: string };
      const run = await browser.execute((payload: { sessionId: string; contextId: string }) => window.__TAURI_INTERNALS__.invoke("capability_submit", { requestId: "directory-capability", capabilityId: "CAP-FILE-012", parameters: {}, ...payload }), { sessionId: state.sessionId, contextId: state.context.id }) as { id: string };
      let result: { state: string; output: string } = { state: "", output: "" };
      await browser.waitUntil(async () => {
        result = await browser.execute((runId: string) => window.__TAURI_INTERNALS__.invoke("run_get", { runId }), run.id) as typeof result;
        return result.state === "succeeded";
      }, { timeout: 5000 });
      const json = result.output.split("\n").find((line) => line.startsWith("{"));
      const actual = JSON.parse(json!) as { finderTarget: string; terminalCwd: string; terminalStarted: boolean };
      expect(actual.finderTarget).toBe(state.context.directoryRef.displayPath);
      expect(actual.terminalCwd).toBe(before.currentDirectory);
      expect(actual.terminalStarted).toBe(true);
      const after = await browser.execute((sessionId: string) => window.__TAURI_INTERNALS__.invoke("terminal_snapshot", { sessionId }), state.sessionId) as { screen: string };
      expect(after.screen).toBe(before.screen);
      writeEvidence("ui-directory-capability.json", { actual, before, after, runId: run.id });
    } finally {
      await browser.execute(async ({ sessionId, lease }: { sessionId: string; lease: string }) => {
        await window.__TAURI_INTERNALS__.invoke("terminal_input", { sessionId, lease, input: [3] });
        await window.__TAURI_INTERNALS__.invoke("terminal_release_lease", { sessionId, lease });
      }, { sessionId: state.sessionId, lease: state.lease });
    }
  });

  it("无模型：能力表单 → 影响预览 → 确认 → 真实 UTF-8 文件", async () => {
    await browser.execute(() => window.__TAURI_INTERNALS__.invoke("app_open_window", { role: "console", page: "library" }));
    await browser.waitUntil(async () => {
      for (const handle of await browser.getWindowHandles()) {
        await browser.switchToWindow(handle);
        if ((await browser.getUrl()).includes("#/console/library")) return true;
      }
      return false;
    }, { timeout: 15_000 });
    await $("[data-capability-id='CAP-TEXT-002'] button").waitForExist();
    await $("[data-capability-id='CAP-TEXT-002'] button").click();
    await $("[data-testid='capability-parameter-name']").waitForExist();
    const fixture = JSON.parse(fs.readFileSync("/tmp/fleqi-attach-finder.json", "utf8")) as { folder: string };
    const capturedDirectory = await $("form[data-capability-directory]").getAttribute("data-capability-directory");
    expect(fs.realpathSync(capturedDirectory!)).toBe(fs.realpathSync(fixture.folder));
    await $("[data-testid='capability-parameter-name']").setValue("local-capability.txt");
    await $("[data-testid='capability-parameter-content']").setValue("Fleqi 本地能力真实输出");
    await $("button=生成操作计划").click();
    await $("[data-testid='run-row']").waitForExist();
    await $("[data-testid='run-row']").click();
    await $("button=确认执行").waitForEnabled();
    const target = path.join(fixture.folder, "local-capability.txt");
    expect(fs.existsSync(target)).toBe(false);
    await $("button=确认执行").click();
    await browser.waitUntil(async () => fs.existsSync(target), { timeout: 15_000 });
    expect(fs.readFileSync(target, "utf8")).toBe("Fleqi 本地能力真实输出");
    await browser.waitUntil(async () => (await $("[data-testid='run-row']").getAttribute("data-run-state")) === "succeeded", { timeout: 15_000 });
    await browser.saveScreenshot(path.join(artifactsDir, "ui-local-capability.png"));
    writeEvidence("ui-local-capability.json", { capability: "CAP-TEXT-002", target, content: fs.readFileSync(target, "utf8"), usesModel: false });
  });

  (process.env.FLEQI_INTERACTIVE_SELECTION ? it : it.skip)("同一 Finder 窗口 A→B 改选：拒绝旧计划，只把 B 的蓝色 PNG 转成 JPG", async () => {
    const fixture = JSON.parse(fs.readFileSync("/tmp/fleqi-attach-finder.json", "utf8")) as { folder: string; windowId: string };
    expect(fixture.folder.startsWith("/tmp/fleqi-attach-")).toBe(true);
    const a = path.join(fixture.folder, "selection-a.png");
    const b = path.join(fixture.folder, "selection-b.png");
    for (const [file, color] of [[a, "red"], [b, "blue"]]) {
      execFileSync("ffmpeg", ["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", `color=c=${color}:s=16x16`, "-frames:v", "1", "-pix_fmt", "rgb24", file!]);
    }
    const selectionWindow = execFileSync("osascript", ["-e", 'on run argv\ntell application "Finder"\nset w to make new Finder window to (POSIX file (item 1 of argv) as alias)\nactivate\nreturn id of w\nend tell\nend run', fixture.folder], { encoding: "utf8" }).trim();
    try {
    const select = async (file: string) => {
      const marker = "/tmp/fleqi-selection-ready";
      fs.rmSync(marker, { force: true });
      fs.writeFileSync("/tmp/fleqi-selection-phase.json", JSON.stringify({ file, folder: fixture.folder, windowId: selectionWindow }));
      await browser.waitUntil(async () => fs.existsSync(marker), { timeout: 120000, interval: 250 });
      fs.rmSync(marker, { force: true });
      return fs.realpathSync(file);
    };
    const selectionReadback = await select(a);
    expect(fs.realpathSync(selectionReadback)).toBe(fs.realpathSync(a));
    // No context_refresh: the form itself must read the selection at the time it is opened.
    const formA = await browser.execute(() => window.__TAURI_INTERNALS__.invoke("capability_form", { capabilityId: "CAP-IMAGE-001" })) as CapabilityForm;
    writeEvidence("ui-selection-initial.json", { formA, selectionReadback });
    expect(formA.context.selectedItems.map((item) => path.basename(item.displayPath))).toEqual(["selection-a.png"]);
    await select(b);
    const session = await browser.execute(() => window.__TAURI_INTERNALS__.invoke("session_create", { requestId: "selection-test-session" })) as { id: string };
    const stale = await browser.execute(async (payload: { sessionId: string; contextId: string }) => {
      try {
        await window.__TAURI_INTERNALS__.invoke("capability_submit", { requestId: "selection-stale", capabilityId: "CAP-IMAGE-001", parameters: { format: "jpg", quality: "95" }, ...payload });
        return "executed";
      } catch (error) { return (error as { code: string }).code; }
    }, { sessionId: session.id, contextId: formA.context.id });
    expect(stale).toBe("conflict");
    expect(fs.existsSync(path.join(fixture.folder, "selection-a.jpg"))).toBe(false);
    const formB = await browser.execute(() => window.__TAURI_INTERNALS__.invoke("capability_form", { capabilityId: "CAP-IMAGE-001" })) as CapabilityForm;
    expect(formB.context.sourceWindowId).toBe(formA.context.sourceWindowId);
    expect(formB.context.selectedItems.map((item) => path.basename(item.displayPath))).toEqual(["selection-b.png"]);
    const run = await browser.execute((payload: { sessionId: string; contextId: string }) => window.__TAURI_INTERNALS__.invoke("capability_submit", { requestId: "selection-fresh", capabilityId: "CAP-IMAGE-001", parameters: { format: "jpg", quality: "95" }, ...payload }), { sessionId: session.id, contextId: formB.context.id }) as RunRecord;
    expect(run.state).toBe("awaitingApproval");
    await browser.execute((payload: { runId: string; planRevision: string }) => window.__TAURI_INTERNALS__.invoke("run_approve", { requestId: "selection-approve", ...payload }), { runId: run.id, planRevision: run.planRevision });
    let completed = run;
    await browser.waitUntil(async () => {
      completed = await browser.execute((runId: string) => window.__TAURI_INTERNALS__.invoke("run_get", { runId }), run.id) as RunRecord;
      return completed.state === "succeeded" || completed.state === "failed";
    }, { timeout: 15000 });
    expect(completed.state).toBe("succeeded");
    const target = path.join(fixture.folder, "selection-b.jpg");
    expect(fs.existsSync(target)).toBe(true);
    expect(fs.existsSync(path.join(fixture.folder, "selection-a.jpg"))).toBe(false);
    const pixel = execFileSync("ffmpeg", ["-hide_banner", "-loglevel", "error", "-i", target, "-frames:v", "1", "-vf", "scale=1:1", "-f", "rawvideo", "-pix_fmt", "rgb24", "pipe:1"]);
    expect(pixel[2]!).toBeGreaterThan(240);
    expect(pixel[0]!).toBeLessThan(15);
    writeEvidence("ui-selection-conversion.json", { formA, formB, stale, run: completed, pixel: Array.from(pixel), onlySelectedFileProduced: true });
    } finally {
      fs.rmSync("/tmp/fleqi-selection-phase.json", { force: true });
      fs.rmSync("/tmp/fleqi-selection-ready", { force: true });
      try { execFileSync("osascript", ["-e", 'on run argv\ntell application "Finder" to close window id ((item 1 of argv) as integer)\nend run', selectionWindow]); } catch { /* The user may already have closed this test-owned window. */ }
    }
  });

});
