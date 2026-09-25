import { browser, expect, $ } from "@wdio/globals";
import fs from "node:fs";
import { writeEvidence } from "../lib/evidence";

// M2 收口 · Finder 窗口移动观察（真实 Finder 驱动）：
// 真实移动 Finder 前窗由 scripts/test-desktop.mjs 的几何驱动器执行——
// 驱动器轮询标志文件 /tmp/fleqi-geom-enabled，存在时以 osascript 真实移动
// Finder 前窗（等价拖动；用户已许可的自动驱动）。本 spec 只负责置位/清除
// 标志，并经真实 IPC 断言 surface 的拖动暂隐/停止恢复时序（零 mock）。
// 由 run-a2 独占串行运行（在 08 之后）。

const GEOMETRY_FLAG = "/tmp/fleqi-geom-enabled";

async function switchToWindowWithHash(fragment: string): Promise<void> {
  await browser.waitUntil(
    async () => {
      for (const handle of await browser.getWindowHandles()) {
        await browser.switchToWindow(handle);
        if ((await browser.getUrl()).includes(fragment)) return true;
      }
      return false;
    },
    { timeout: 30_000 },
  );
}

type SurfaceInfo = { visibility: string; visibleSessionId: string | null; activation: string };

async function surfaceGet(): Promise<SurfaceInfo | null> {
  return browser.execute(async () => {
    return await window.__TAURI_INTERNALS__.invoke("surface_get").then(
      (value: SurfaceInfo) => value,
      () => null as null,
    );
  });
}

async function waitForVisibility(
  target: string,
  timeoutMs: number,
): Promise<{ reached: boolean; last: SurfaceInfo | null }> {
  const deadline = Date.now() + timeoutMs;
  let last: SurfaceInfo | null = null;
  while (Date.now() < deadline) {
    last = await surfaceGet();
    if (last?.visibility === target) return { reached: true, last };
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  return { reached: false, last };
}

describe("M2 收口 · Finder 窗口移动观察（真实 Finder 驱动）", () => {
  it("拖动期间输入条暂隐；停止移动后恢复并保留会话", async () => {
    await switchToWindowWithHash("#/console");
    await $("[data-phase='ready']").waitForExist({ timeout: 30_000 });
    await browser.waitUntil(
      async () => (await $("[data-testid='host-state']").getAttribute("data-host-state")) === "ready",
      { timeout: 20_000 },
    );

    // 需要可见输入条：注册热键并显式显示（创建可见会话）。
    const setup = (await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      const hotkey = await bridge
        .invoke("hotkey_commit", { requestId: "m9-hotkey", accelerator: "CommandOrControl+Shift+G" })
        .then(
          (value: { registered: string | null }) => value,
          (error: unknown) => ({ raw: String(error) }),
        );
      const shown = await bridge.invoke("surface_show").then(
        (value: { visibility: string; visibleSessionId: string | null }) => value,
        (error: { code?: string; message?: string }) => ({ code: error.code ?? "", message: error.message ?? "" }),
      );
      return { hotkey, shown };
    })) as { hotkey: { registered?: string | null; raw?: string }; shown: { visibility?: string; visibleSessionId?: string | null; raw?: string } };
    const sessionId = setup.shown?.visibleSessionId ?? null;
    writeEvidence("m2-geometry-setup.json", {
      capturedAt: new Date().toISOString(),
      hotkey: setup.hotkey,
      shown: setup.shown,
    });
    if (!setup.shown.visibility) {
      // 显示被拒：读宿主热键真实状态后重试一次（诊断持久化与注册状态对齐）。
      const diag = await browser.execute(async () => {
        const bridge = window.__TAURI_INTERNALS__;
        const status = await bridge.invoke("hotkey_get").then(
          (value: unknown) => value,
          (error: { code?: string; message?: string }) => ({ code: error.code ?? "", message: error.message ?? "" }),
        );
        const settingsHotkey = await bridge.invoke("app_bootstrap").then(
          (value: { settings: { hotkey: unknown; activation: string } }) => ({ hotkey: value.settings.hotkey, activation: value.settings.activation }),
          () => null,
        );
        const retry = await bridge.invoke("surface_show").then(
          (value: { visibility: string; visibleSessionId: string | null }) => value,
          (error: { code?: string; message?: string }) => ({ code: error.code ?? "", message: error.message ?? "" }),
        );
        return { status, settingsHotkey, retry };
      }) as { status: unknown; settingsHotkey: unknown; retry: unknown };
      setup.shown = diag.retry as typeof setup.shown;
      writeEvidence("m2-geometry-setup-retry.json", {
        capturedAt: new Date().toISOString(),
        hotkeyStatus: diag.status,
        settingsHotkey: diag.settingsHotkey,
        shown: setup.shown,
      });
    }
    expect(setup.hotkey.registered ?? null).toBe("CommandOrControl+Shift+G");
    expect(setup.shown.visibility).toBe("visible");
    expect(sessionId).toBeTruthy();

    // 置位标志 → 驱动器开始真实移动 Finder 前窗 → 断言拖动暂隐。
    fs.writeFileSync(GEOMETRY_FLAG, new Date().toISOString());
    const hiddenPhase = await waitForVisibility("temporarilyHidden", 60_000);
    // 清除标志 → 驱动器停止移动 → 窗口稳定 → 断言恢复且可见会话不丢。
    fs.rmSync(GEOMETRY_FLAG, { force: true });
    const restoredPhase = await waitForVisibility("visible", 20_000);
    const after = await surfaceGet();

    writeEvidence("m2-geometry-watch.json", {
      capturedAt: new Date().toISOString(),
      sessionId,
      hiddenPhase,
      restoredPhase,
      finalSurface: after,
    });
    expect(hiddenPhase.reached).toBe(true);
    expect(restoredPhase.reached).toBe(true);
    expect(after?.visibleSessionId).toBe(sessionId);
  });
});
