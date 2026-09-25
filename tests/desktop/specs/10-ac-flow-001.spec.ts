import { browser, expect, $ } from "@wdio/globals";
import fs from "node:fs";
import { writeEvidence } from "../lib/evidence";

// AC-FLOW-001 · 新安装语义（全新数据目录上的第一个运行，FR-ENTRY-003/FR-SET-001）：
// 默认 manual + keepAll + 未绑定快捷键；切换 Finder 不自动出现输入条；
// surface_show 被拒给出引导文案（菜单引导的后端语义）；真实注册快捷键成功后
// 显式唤起创建新会话并显示有效目录。
// runner 使本运行位于数据目录生命周期的最前（run-a1-001 排序在最前）。
// Finder 切换由 runner 驱动器执行（标志/标记文件协议；用户已许可的自动驱动）。

const SWITCH_FLAG = "/tmp/fleqi-001-switch";
const SWITCH_MARKER = "/tmp/fleqi-001-switch-done";

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

describe("AC-FLOW-001 · 新安装默认与引导（全新数据目录）", () => {
  it("默认 manual/keepAll/未绑定；切换 Finder 不自动出现；注册成功后显式唤起新会话", async () => {
    await switchToWindowWithHash("#/console");
    await $("[data-phase='ready']").waitForExist({ timeout: 30_000 });
    await browser.waitUntil(
      async () => (await $("[data-testid='host-state']").getAttribute("data-host-state")) === "ready",
      { timeout: 20_000 },
    );

    // 1) 全新安装默认值 + 输入条未显示。
    const fresh = (await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      const bootstrap = await bridge.invoke("app_bootstrap").then(
        (value: { settings: { activation: string; hideBehavior: string; hotkey: unknown; persisted: boolean } }) => value.settings,
        (error: { code?: string; message?: string }) => ({ code: error.code ?? "", message: error.message ?? "" }),
      );
      const surface = await bridge.invoke("surface_get").then(
        (value: { visibility: string }) => value,
        (error: { code?: string; message?: string }) => ({ code: error.code ?? "", message: error.message ?? "" }),
      );
      return { bootstrap, surface };
    })) as { bootstrap: { activation: string; hideBehavior: string; hotkey: unknown; persisted: boolean }; surface: { visibility?: string } };
    writeEvidence("m10-fresh-defaults.json", { capturedAt: new Date().toISOString(), ...fresh });
    expect(fresh.bootstrap.activation).toBe("manual");
    expect(fresh.bootstrap.hideBehavior).toBe("keepAll");
    expect(fresh.bootstrap.hotkey).toBeNull();
    expect(fresh.bootstrap.persisted).toBe(true);
    expect(fresh.surface.visibility).not.toBe("visible");

    // 2) 切换 Finder（真实激活事件）：manual 模式不得自动出现输入条。
    fs.writeFileSync(SWITCH_FLAG, new Date().toISOString());
    let switchedFolder = "";
    try {
      await browser.waitUntil(
        () => {
          const done = fs.existsSync(SWITCH_MARKER);
          if (done) {
            switchedFolder = (JSON.parse(fs.readFileSync(SWITCH_MARKER, "utf8")) as { folder: string }).folder;
          }
          return done;
        },
        { timeout: 20_000, interval: 500 },
      );
      // 留观察窗（自动显示若有必在此发生）；Finder 已在前台时激活事件可能不重发，
      // 用产品的手动刷新路径确保上下文跟上切换，再断言输入条保持未显示。
      await new Promise((resolve) => setTimeout(resolve, 3000));
      const afterSwitch = (await browser.execute(async () => {
        const bridge = window.__TAURI_INTERNALS__;
        const refreshed = await bridge.invoke("context_refresh").then(
          (value: { directoryRef: { displayPath: string } | null }) => value,
          () => null,
        );
        const surface = await bridge.invoke("surface_get").then(
          (value: { visibility: string }) => value,
          () => null,
        );
        return { refreshed, surface };
      })) as { refreshed: { directoryRef: { displayPath: string } | null } | null; surface: { visibility: string } | null };
      writeEvidence("m10-after-switch.json", { capturedAt: new Date().toISOString(), switchedFolder, afterSwitch });
      expect(afterSwitch?.surface?.visibility).not.toBe("visible");
      expect(afterSwitch?.refreshed?.directoryRef?.displayPath ?? "").toContain("fleqi-001-dir");
    } finally {
      fs.rmSync(SWITCH_FLAG, { force: true });
    }

    // 3) 未注册时显式显示被拒并引导（菜单"显示输入条"被拒后引导设置的后端语义）。
    const refused = (await browser.execute(async () => {
      return await window.__TAURI_INTERNALS__.invoke("surface_show").then(
        () => ({ refused: false }),
        (error: { code?: string; message?: string }) => ({ refused: true, code: error.code ?? "", message: error.message ?? "" }),
      );
    })) as { refused: boolean; code?: string; message?: string };
    writeEvidence("m10-refused.json", { capturedAt: new Date().toISOString(), ...refused });
    expect(refused.refused).toBe(true);
    expect(refused.code).toBe("unavailable");
    expect(refused.message).toContain("快捷键");

    // 4) 真实注册快捷键成功 → 显式唤起创建新会话并显示有效目录。
    const shown = (await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      const hotkey = await bridge
        .invoke("hotkey_commit", { requestId: "m10-hotkey", accelerator: "CommandOrControl+Shift+K" })
        .then(
          (value: { registered: string | null; message: string | null }) => value,
          (error: { code?: string; message?: string }) => ({ code: error.code ?? "", message: error.message ?? "" }),
        );
      const show = await bridge.invoke("surface_show").then(
        (value: { visibility: string; visibleSessionId: string | null }) => value,
        (error: { code?: string; message?: string }) => ({ code: error.code ?? "", message: error.message ?? "" }),
      );
      const bootstrap = await bridge.invoke("app_bootstrap").then(
        (value: { context: { directoryRef: { displayPath: string } | null } | null }) => value.context,
        () => null,
      );
      return { hotkey, show, context: bootstrap };
    })) as { hotkey: { registered?: string | null }; show: { visibility?: string; visibleSessionId?: string | null }; context: { directoryRef: { displayPath: string } | null } | null };
    writeEvidence("m10-registered-shown.json", { capturedAt: new Date().toISOString(), ...shown });
    expect(shown.hotkey.registered).toBe("CommandOrControl+Shift+K");
    expect(shown.show.visibility).toBe("visible");
    expect(shown.show.visibleSessionId).toBeTruthy();
    const displayPath = shown.context?.directoryRef?.displayPath ?? "";
    expect(displayPath.length).toBeGreaterThan(0);
  });
});
