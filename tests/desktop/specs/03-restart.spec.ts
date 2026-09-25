import { $, browser, expect } from "@wdio/globals";
import { evidencePath, writeEvidence } from "../lib/evidence";

// 本 spec 由 scripts/test-desktop.mjs 作为第二次 wdio 运行执行：新的宿主进程读取同一数据目录，
// 验证重启后从 SQLite 恢复设置，不重跑任何旧动作。
// run-a 内各 spec（02 设置流、04 热键、07 AC-FLOW 的设置 UI 变更）都会推进 revision，
// 因此版本断言为"至少覆盖 02 的四次保存"；外观值只由 02 修改，保持精确断言。
describe("M1.6 · 重启恢复", () => {
  it("新进程加载后设置版本与外观值来自持久化记录", async () => {
    await $("[data-phase='ready']").waitForExist({ timeout: 30_000 });
    await browser.waitUntil(async () => (await $("[data-testid='host-state']").getAttribute("data-host-state")) === "ready", { timeout: 20_000 });
    const revision = Number(await $("[data-field='settings-revision']").getText());
    expect(Number.isFinite(revision) && revision >= 4).toBe(true);
    expect(await $("html").getAttribute("data-theme")).toBe("light");
    expect(await $("html").getAttribute("data-transparency")).toBe("off");
    expect(await $("html").getAttribute("data-motion")).toBe("reduce");
    await browser.saveScreenshot(evidencePath("m1-console-after-restart.png"));
    writeEvidence("m1-restart-evidence.json", {
      capturedAt: new Date().toISOString(),
      settingsRevision: String(revision),
      persistedHotkey: "run-a 内各 spec 的热键动作（04/07）以最后状态持久化",
      theme: "light",
      transparency: false,
      motionMode: "reduce",
      note: "第二个宿主进程读取同一测试数据目录；未重跑任何设置更新；revision ≥4（02 的四次保存 + 04/07 的热键与策略保存）。",
    });
  });

  it("重启恢复：上次进程遗留的活跃会话标记 interrupted，不复活进程（FR-SESSION-011）", async () => {
    const summary = (await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      const list = await bridge
        .invoke("session_list", { offset: 0, limit: 100 })
        .then(
          (value: { active: { id: string; state: string; terminalId: string | null }[]; history: { id: string; state: string; terminalId: string | null }[] }) => value,
          (error: { code?: string }) => null,
        );
      if (!list) return { step: "list", ok: false };
      const interrupted = list.history.filter((s) => s.state === "interrupted");
      return {
        step: "done",
        ok: true,
        activeCount: list.active.length,
        interruptedCount: interrupted.length,
        interruptedHaveNoTerminal: interrupted.every((s) => s.terminalId === null),
      };
    })) as Record<string, unknown>;
    expect(summary.ok).toBe(true);
    expect(summary.activeCount as number).toBe(0);
    expect(summary.interruptedCount as number).toBeGreaterThanOrEqual(2);
    expect(summary.interruptedHaveNoTerminal).toBe(true);
    writeEvidence("m4-restart-interrupted.json", {
      capturedAt: new Date().toISOString(),
      ...summary,
      note: "run-a 留下的活跃会话在新进程加载时标记 interrupted 并清空 terminalId；不重跑任何进程。",
    });
  });
});
