import { readFileSync } from "node:fs";
import { $, browser, expect } from "@wdio/globals";
import { evidencePath, writeEvidence } from "../lib/evidence";

// 经页面内 Tauri 桥调用真实 IPC（与 UI 同一条桥，不 mock）。
// 会话/PTY 编排已由 Rust 集成测试（m2_services、terminal，真实 zsh）覆盖；
// 本文件验证宿主 IPC 的真实拒绝/接受路径，全部使用零参字面量闭包。


type Outcome = { ok: boolean; value?: unknown; code?: string; message?: string };

async function switchToConsole(): Promise<void> {
  await browser.waitUntil(
    async () => {
      for (const handle of await browser.getWindowHandles()) {
        await browser.switchToWindow(handle);
        if ((await browser.getUrl()).includes("#/console")) return true;
      }
      return false;
    },
    { timeout: 30_000 },
  );
}

describe("M2.6 · 输入条显示状态机与热键（真实 IPC）", () => {
  it("manual 无热键：surface_show 被拒绝并引导快捷键（FR-ENTRY-003）", async () => {
    await switchToConsole();
    await $("[data-phase='ready']").waitForExist({ timeout: 30_000 });
    await browser.waitUntil(async () => (await $("[data-testid='host-state']").getAttribute("data-host-state")) === "ready", { timeout: 20_000 });
    const restored = await browser.execute(async () => {
      const api = window.__TAURI_INTERNALS__;
      const boot = await api.invoke("app_bootstrap") as { settings: { hotkey: unknown } };
      const registered = await api.invoke("hotkey_get") as { registered: string | null };
      await api.invoke("hotkey_clear", { requestId: "m2-unbound-fixture" });
      return { persisted: !!boot.settings.hotkey, registered: !!registered.registered };
    });
    expect(restored.registered).toBe(restored.persisted);
    const result = (await browser.execute(() =>
      window.__TAURI_INTERNALS__
        .invoke("surface_show")
        .then((value) => ({ ok: true, value }))
        .catch((error) => ({ ok: false, code: (error as { code?: string })?.code, message: (error as { message?: string })?.message })),
    )) as Outcome;
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.code).toBe("unavailable");
      expect(result.message).toContain("快捷键");
    }
  });

  it("热键真实注册 → 显式显示（自动建会话）→ keepAll 隐藏 → 再显式显示新建（FR-SESSION-002/004）", async () => {
    const committed = (await browser.execute(() =>
      window.__TAURI_INTERNALS__
        .invoke("hotkey_commit", { requestId: "m2-hotkey", accelerator: "CommandOrControl+Shift+F" })
        .then((value) => ({ ok: true, value }))
        .catch((error) => ({ ok: false, code: (error as { code?: string })?.code, message: (error as { message?: string })?.message })),
    )) as Outcome;
    expect(committed.ok).toBe(true);
    if (committed.ok) {
      const value = committed.value as { registered: string | null; message: string | null };
      expect(value.registered).toBe("CommandOrControl+Shift+F");
      expect(value.message).toBeNull();
    }

    const shown = (await browser.execute(() =>
      window.__TAURI_INTERNALS__
        .invoke("surface_show")
        .then((value) => ({ ok: true, value }))
        .catch((error) => ({ ok: false, code: (error as { code?: string })?.code, message: (error as { message?: string })?.message })),
    )) as Outcome;
    expect(shown.ok).toBe(true);
    let firstSession = "";
    if (shown.ok) {
      const value = shown.value as { visibility: string; visibleSessionId: string | null };
      expect(value.visibility).toBe("visible");
      expect(typeof value.visibleSessionId).toBe("string");
      firstSession = value.visibleSessionId ?? "";
    }

    const hidden = (await browser.execute(() =>
      window.__TAURI_INTERNALS__
        .invoke("surface_hide")
        .then((value) => ({ ok: true, value }))
        .catch((error) => ({ ok: false, code: (error as { code?: string })?.code, message: (error as { message?: string })?.message })),
    )) as Outcome;
    expect(hidden.ok).toBe(true);
    if (hidden.ok) {
      expect((hidden.value as { visibility: string }).visibility).toBe("userHidden");
    }

    const reshow = (await browser.execute(() =>
      window.__TAURI_INTERNALS__
        .invoke("surface_show")
        .then((value) => ({ ok: true, value }))
        .catch((error) => ({ ok: false, code: (error as { code?: string })?.code, message: (error as { message?: string })?.message })),
    )) as Outcome;
    expect(reshow.ok).toBe(true);
    if (reshow.ok) {
      const value = reshow.value as { visibleSessionId: string | null };
      expect((value.visibleSessionId ?? "")).not.toBe(firstSession);
    }

    writeEvidence("m2-surface-evidence.json", {
      capturedAt: new Date().toISOString(),
      hotkey: committed.ok ? committed.value : null,
      firstShow: shown.ok ? shown.value : null,
      hide: hidden.ok ? hidden.value : null,
      reshow: reshow.ok ? reshow.value : null,
      note: "真实 surface_show/hide 与 global-shortcut 注册；keepAll 隐藏后再次显式显示创建新会话（FR-SESSION-002）。",
    });
  });

  it("AC-FLOW-009：! 标记命令经宿主去标记后进 PTY，真实 zsh 执行输出可见（FR-TERM-001/002）", async () => {
    const summary = (await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      // 隔离运行时上下文可能尚未捕获过：显式刷新，保证会话有有效目录可开 PTY。
      await bridge.invoke("context_refresh").then(() => null, () => null);
      const sessions = await bridge.invoke("session_list", { offset: 0, limit: 50 }).then(
        (value: { active: Array<{ id: string }> }) => value,
        (error: unknown) => ({ raw: String(error) }),
      );
      if (!("active" in sessions)) return { step: "session_list", ok: false, raw: sessions.raw };
      const session = sessions.active[0];
      if (!session) return { step: "no-active-session", ok: false };

      const opened = await bridge.invoke("terminal_open", { sessionId: session.id, cols: 100, rows: 30 }).then(
        () => ({ ok: true as const }),
        (error: { message?: string }) => ({ ok: false as const, message: error.message ?? "" }),
      );
      if (!opened.ok) return { step: "terminal_open", ok: false, message: opened.message };
      // 轮询 shellReadiness=ready（安全提示符），再持租约提交。
      let readiness = "";
      for (let i = 0; i < 40; i += 1) {
        readiness = await bridge
          .invoke("terminal_snapshot", { sessionId: session.id })
          .then((value: { shellReadiness: string }) => value.shellReadiness, () => "error");
        if (readiness === "ready") break;
        await new Promise((resolve) => setTimeout(resolve, 250));
      }
      if (readiness !== "ready") return { step: "readiness", ok: false, readiness };

      // 目标目录取 PTY 的真实 cwd：会话记录的目录可能落后于按需创建的 PTY。
      const ptyCwd = await bridge
        .invoke("terminal_snapshot", { sessionId: session.id })
        .then((value: { currentDirectory: string }) => value.currentDirectory, () => "");
      if (!ptyCwd) return { step: "pty-cwd", ok: false };

      const lease = await bridge.invoke("terminal_acquire_lease", { sessionId: session.id, owner: "spec-m2-009" }).then(
        (value: string) => value,
        (error: unknown) => String(error),
      );
      if (typeof lease !== "string" || !lease) return { step: "lease", ok: false, lease };

      const bootstrap = await bridge.invoke("app_bootstrap").then(
        (value: { context: { revision: string } | null }) => value,
        () => null,
      );
      const contextRevision = bootstrap?.context?.revision ?? "1";
      const submitted = await bridge
        .invoke("terminal_submit_line", {
          requestId: "m2-009-submit",
          sessionId: session.id,
          line: "!echo fleqi-009-marker",
          contextRevision,
          targetDisplay: ptyCwd,
        })
        .then(
          (value: string) => ({ ok: true as const, value }),
          (error: { message?: string } | string) => ({ ok: false as const, raw: typeof error === "string" ? error : error.message ?? "" }),
        );
      if (!submitted.ok) return { step: "submit", ok: false, raw: submitted.raw };
      // sent = 立即投递；queued = 等安全提示符自动投递（随后同样执行）。
      if (submitted.value !== "sent" && submitted.value !== "queued") {
        return { step: "submit-state", ok: false, state: submitted.value };
      }

      // 轮询屏幕直到标记出现（命令回显与输出都含标记；出现即证明 ! 被剥掉且命令真实执行）。
      let screenText = "";
      for (let i = 0; i < 32; i += 1) {
        screenText = await bridge
          .invoke("terminal_snapshot", { sessionId: session.id })
          .then((value: { screen: string }) => value.screen, () => "");
        if (screenText.includes("fleqi-009-marker")) break;
        await new Promise((resolve) => setTimeout(resolve, 250));
      }
      return {
        step: "done",
        ok: screenText.includes("fleqi-009-marker") && !screenText.includes("!echo"),
        screen: screenText.slice(-600),
        sessionId: session.id,
        submittedState: submitted.value,
      };
    })) as Record<string, unknown>;
    writeEvidence("m2-ac-flow-009.json", { capturedAt: new Date().toISOString(), ...summary });
    expect(summary.ok).toBe(true);
    expect(summary.step).toBe("done");
  });

  it("无边框窗口色证据：输入条自绘红绿灯与拖动条（截图）", async () => {
    await switchToConsole();
    await $("[data-phase='ready']").waitForExist({ timeout: 30_000 });
    // 经宿主真实路径创建输入条窗口：注册热键后 surface_show（manual 唤起）。
    await browser.execute(() =>
      window.__TAURI_INTERNALS__
        .invoke("hotkey_commit", { requestId: "m2-chrome-hotkey", accelerator: "CommandOrControl+Shift+U" })
        .then((value: unknown) => ({ ok: true }), (error: { code?: string }) => ({ ok: false, code: error?.code })),
    );
    await browser.execute(() =>
      window.__TAURI_INTERNALS__
        .invoke("surface_show")
        .then((value: unknown) => ({ ok: true }), (error: { code?: string }) => ({ ok: false, code: error?.code })),
    );
    await browser.waitUntil(
      async () => {
        for (const handle of await browser.getWindowHandles()) {
          await browser.switchToWindow(handle);
          if ((await browser.getUrl()).includes("#/composer")) return true;
        }
        return false;
      },
      { timeout: 30_000 },
    );
    await $("[data-testid='composer']").waitForExist({ timeout: 10_000 });
    await browser.saveScreenshot(evidencePath("m2-composer-borderless.png"));
    const dragRegions = await browser.execute(() =>
      Array.from(document.querySelectorAll("[data-tauri-drag-region]")).map((el) => el.getAttribute("data-testid")),
    );
    expect(dragRegions).toEqual(["window-drag-region"]);
    // 贴附几何断言（ui-design.md §4.1）：webview 屏幕坐标（逻辑点、左上原点）
    // 对照 runner 自建 Finder 目标窗的 bounds；输入条保持可见留出采样时间。
    await new Promise((resolve) => setTimeout(resolve, 1500));
    const finderTarget = JSON.parse(readFileSync("/tmp/fleqi-attach-finder.json", "utf8")) as {
      x1: number;
      y1: number;
      x2: number;
      y2: number;
    };
    // WKWebView 不向页面暴露真实窗口几何：经宿主 core:window 只读能力取外框（物理像素），
    // 除以 scale_factor 得逻辑点，与 Finder AppleScript 点坐标同基准。
    const composerWindow = await browser.execute(async () => {
      const invoke = window.__TAURI_INTERNALS__.invoke as (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
      const scale = (await invoke("plugin:window|scale_factor")) as number;
      const position = (await invoke("plugin:window|outer_position")) as { x: number; y: number };
      const size = (await invoke("plugin:window|outer_size")) as { width: number; height: number };
      return {
        x: position.x / scale,
        y: position.y / scale,
        width: size.width / scale,
        height: size.height / scale,
      };
    });
    const expectedWidth = Math.max(finderTarget.x2 - finderTarget.x1, 560);
    const checks = {
      高度72: Math.abs(composerWindow.height - 72) <= 2,
      与Finder等宽最小560: Math.abs(composerWindow.width - expectedWidth) <= 2,
      左缘对齐Finder: Math.abs(composerWindow.x - finderTarget.x1) <= 2,
      贴Finder外侧下方4px: Math.abs(composerWindow.y - (finderTarget.y2 + 4)) <= 4,
    };
    writeEvidence("m2-attach-bounds.json", {
      capturedAt: new Date().toISOString(),
      finderTarget,
      composerWindow,
      checks,
      ok: Object.values(checks).every(Boolean),
    });
    expect(Object.values(checks).every(Boolean)).toBe(true);
  });
});
