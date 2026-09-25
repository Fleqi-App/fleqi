import { browser, expect, $ } from "@wdio/globals";
import fs from "node:fs";
import { writeEvidence } from "../lib/evidence";

// AC-FLOW-005 · vim 占用前台时连续切换 Finder A→B→C（真实 vim + 真实 Finder 激活）：
// 终端运行 vim 期间不注入目录命令、终端原始键可用（插入文本真实到达 vim 缓冲）；
// 序列结束后退出 vim，安全提示符同步到 C。Finder 序列由 runner 驱动器执行
//（标志/标记文件协议；用户已许可的自动驱动）。由 run-a2-005 独占串行运行。
// 会话 ID 经 body data 属性在多条 browser.execute 间传递（零参闭包）。

const SEQ_A_FLAG = "/tmp/fleqi-005-open-a";
const SEQ_A_MARKER = "/tmp/fleqi-005-a-done";
const SEQ_FLAG = "/tmp/fleqi-005-sequence";
const SEQ_MARKER = "/tmp/fleqi-005-seq-done";

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

type Snapshot = {
  shellReadiness: string;
  foregroundProcess: string | null;
  currentDirectory: string;
  pendingDirectory: string | null;
  screen: string;
};

async function snapshot(): Promise<Snapshot | null> {
  return browser.execute(async () => {
    const bridge = window.__TAURI_INTERNALS__;
    const sid = document.body.dataset.fleqi11Session ?? "";
    return await bridge.invoke("terminal_snapshot", { sessionId: sid }).then(
      (value: Snapshot) => value,
      () => null,
    );
  });
}

async function sendRaw(text: string): Promise<boolean> {
  const payload = text;
  return browser.execute(async (raw: string) => {
    const bridge = window.__TAURI_INTERNALS__;
    const sid = document.body.dataset.fleqi11Session ?? "";
    const lease = await bridge.invoke("terminal_acquire_lease", { sessionId: sid, owner: "spec-m11" }).then(
      (value: string) => value,
      () => "",
    );
    if (!lease) return false;
    const bytes = Array.from(new TextEncoder().encode(raw));
    return await bridge.invoke("terminal_input", { sessionId: sid, lease, input: bytes }).then(
      () => true,
      () => false,
    );
  }, payload);
}

/** /tmp 在 macOS 是 /private/tmp 的符号链接：比较前归一化前缀。 */
function normPath(value: string): string {
  return value.replace(/\/$/, "").replace(/^\/private/, "");
}

describe("AC-FLOW-005 · vim 占用 + 连续切换 Finder A→B→C（真实 vim + 真实 Finder）", () => {
  it("忙时不注入目录命令、原始键可用；退出后同步到 C", async () => {
    await switchToWindowWithHash("#/console");
    await $("[data-phase='ready']").waitForExist({ timeout: 30_000 });
    await browser.waitUntil(
      async () => (await $("[data-testid='host-state']").getAttribute("data-host-state")) === "ready",
      { timeout: 20_000 },
    );

    // 1) 显式唤起；驱动器阶段一打开 A → 上下文/工作目录落在 A。
    const setup = (await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      await bridge
        .invoke("hotkey_commit", { requestId: "m11-hotkey", accelerator: "CommandOrControl+Shift+L" })
        .then(() => null, () => null);
      const shown = await bridge.invoke("surface_show").then(
        (value: { visibleSessionId: string | null }) => value,
        (error: { code?: string; message?: string }) => ({ code: error.code ?? "", message: error.message ?? "" }),
      );
      const session = shown && "visibleSessionId" in shown ? shown.visibleSessionId ?? "" : "";
      document.body.dataset.fleqi11Session = session;
      return { shown };
    })) as { shown: { visibleSessionId?: string | null } };
    const sessionId = setup.shown?.visibleSessionId ?? null;
    expect(sessionId).toBeTruthy();

    fs.writeFileSync(SEQ_A_FLAG, new Date().toISOString());
    let dirA = "";
    try {
      await browser.waitUntil(
        () => {
          const done = fs.existsSync(SEQ_A_MARKER);
          if (done) {
            dirA = (JSON.parse(fs.readFileSync(SEQ_A_MARKER, "utf8")) as { a: string }).a;
          }
          return done;
        },
        { timeout: 20_000, interval: 500 },
      );
    } finally {
      fs.rmSync(SEQ_A_FLAG, { force: true });
    }
    expect(dirA).toContain("fleqi-005-");
    // 上下文跟上 A（手动刷新路径；激活事件可能因 Finder 已在前台而不重发）。
    const refreshedContext = (await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      return await bridge.invoke("context_refresh").then(
        (value: { directoryRef: { displayPath: string } | null }) => value?.directoryRef?.displayPath ?? "",
        () => "",
      );
    })) as string;
    expect(refreshedContext).toContain("fleqi-005-");

    const terminalId = (await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      const sid = document.body.dataset.fleqi11Session ?? "";
      return await bridge.invoke("terminal_open", { sessionId: sid, cols: 100, rows: 30 }).then(
        (value: string) => value,
        (error: { code?: string; message?: string }) => "",
      );
    })) as string;
    if (!terminalId) {
      throw new Error("terminal_open 失败：未取得 terminalId");
    }
    writeEvidence("m11-opened.json", { capturedAt: new Date().toISOString(), terminalId });

    // 2) 等安全提示符 → 启动 vim（真实 /usr/bin/vim，无配置）。
    let ready = false;
    for (let i = 0; i < 40; i += 1) {
      const snap = await snapshot();
      if (snap?.shellReadiness === "ready") {
        ready = true;
        break;
      }
      await new Promise((resolve) => setTimeout(resolve, 250));
    }
    expect(ready).toBe(true);

    // 目标目录取 PTY 的真实 cwd（会话记录的目录可能落后于按需创建的 PTY），
    // 写入 dataset 供随后的提交闭包读取。
    const ptyCwd = (await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      const sid = document.body.dataset.fleqi11Session ?? "";
      const cwd = await bridge.invoke("terminal_snapshot", { sessionId: sid }).then(
        (value: { currentDirectory: string }) => value.currentDirectory,
        () => "",
      );
      document.body.dataset.fleqi11Cwd = cwd;
      return cwd;
    })) as string;
    expect(ptyCwd.length).toBeGreaterThan(0);

    const submitted = (await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      const sid = document.body.dataset.fleqi11Session ?? "";
      const lease = await bridge.invoke("terminal_acquire_lease", { sessionId: sid, owner: "spec-m11" }).then(
        (value: string) => value,
        (error: { code?: string; message?: string }) => ({ code: error.code ?? "", message: error.message ?? "" }),
      );
      if (typeof lease !== "string") return { error: lease };
      return await bridge
        .invoke("terminal_submit_line", {
          requestId: "m11-vim",
          sessionId: sid,
          line: "!vim -n -u NONE -i NONE -N /tmp/fleqi-005-note.txt",
          contextRevision: "1",
          targetDisplay: document.body.dataset.fleqi11Cwd ?? "",
        })
        .then(
          (value: string) => ({ result: value }),
          (error: { code?: string; message?: string }) => ({ error: error.message ?? "" }),
        );
    })) as { result?: string; error?: string };
    writeEvidence("m11-vim-submitted.json", { capturedAt: new Date().toISOString(), ...submitted });
    expect(submitted.result === "sent" || submitted.result === "queued").toBe(true);

    // 3) vim 进入前台。
    let vimUp = false;
    for (let i = 0; i < 40; i += 1) {
      const snap = await snapshot();
      if (snap?.foregroundProcess?.includes("vim")) {
        vimUp = true;
        break;
      }
      await new Promise((resolve) => setTimeout(resolve, 250));
    }
    if (!vimUp) {
      const failSnap = await snapshot();
      writeEvidence("m11-vimup-fail.json", {
        capturedAt: new Date().toISOString(),
        foregroundProcess: failSnap?.foregroundProcess ?? null,
        readiness: failSnap?.shellReadiness ?? null,
        currentDirectory: failSnap?.currentDirectory ?? null,
        screenTail: (failSnap?.screen ?? "").slice(-600),
      });
    }
    expect(vimUp).toBe(true);
    fs.rmSync("/tmp/fleqi-005-note.txt", { force: true });

    // 4) 原始键：插入模式写入标记文本（vim 内不经过 AI/目录管道）。
    expect(await sendRaw("ifleqi-005-vim-live\x1b")).toBe(true);
    await new Promise((resolve) => setTimeout(resolve, 800));

    // 5) vim 占用期间切换 Finder B→C（驱动器阶段二）：不注入目录命令。
    fs.writeFileSync(SEQ_FLAG, new Date().toISOString());
    let sequenceDone = false;
    let cPath = "";
    try {
      await browser.waitUntil(
        () => {
          const done = fs.existsSync(SEQ_MARKER);
          if (done) {
            cPath = (JSON.parse(fs.readFileSync(SEQ_MARKER, "utf8")) as { c: string }).c;
            fs.rmSync(SEQ_FLAG, { force: true });
          }
          return done;
        },
        { timeout: 25_000, interval: 500 },
      );
      sequenceDone = true;
    } finally {
      fs.rmSync(SEQ_FLAG, { force: true });
    }
    expect(sequenceDone).toBe(true);
    expect(cPath).toContain("fleqi-005-");

    // 上下文跟上最新目标 C（产品的手动刷新路径；vim 全程占用中）。
    const busyContext = (await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      return await bridge.invoke("context_refresh").then(
        (value: { directoryRef: { displayPath: string } | null }) => value?.directoryRef?.displayPath ?? "",
        () => "",
      );
    })) as string;
    expect(normPath(busyContext)).toBe(normPath(cPath));

    // 宿主快照合并 SyncMachine 的最新待同步目标；pending 不意味着已经注入 cd。
    // 以忙碌前台、实际 cwd 保持 A 和屏幕无控制命令共同验证输入隔离。
    let sessionTarget: string | null = null;
    const targetDeadline = Date.now() + 15_000;
    while (Date.now() < targetDeadline) {
      sessionTarget = (await browser.execute(async () => {
        const bridge = window.__TAURI_INTERNALS__;
        const sid = document.body.dataset.fleqi11Session ?? "";
        const list = await bridge.invoke("session_list", { offset: 0, limit: 50 }).then(
          (value: { active: { id: string; targetDirectory: string | null }[] }) => value,
          () => null,
        );
        return list?.active.find((s) => s.id === sid)?.targetDirectory ?? null;
      })) as string | null;
      if (normPath(sessionTarget ?? "") === normPath(cPath)) break;
      // 重发刷新触发服务端目标更新（vim 依旧占用，不注入）。
      const probe = (await browser.execute(async () => {
        const bridge = window.__TAURI_INTERNALS__;
        const ctx = await bridge.invoke("context_refresh").then(
          (value: { directoryRef: { displayPath: string } | null }) => value?.directoryRef?.displayPath ?? "",
          () => "refresh-error",
        );
        return ctx;
      })) as string;
      console.log(`[spec-005] 刷新后上下文：${probe}（目标 c=${cPath}）`);
      await new Promise((resolve) => setTimeout(resolve, 600));
    }
    const busySnap = await snapshot();
    const busyEvidence = {
      foreground: busySnap?.foregroundProcess,
      sessionTarget,
      pendingDirectory: busySnap?.pendingDirectory ?? null,
      screenTail: (busySnap?.screen ?? "").slice(-500),
    };
    writeEvidence("m11-busy-during-switches.json", { capturedAt: new Date().toISOString(), ...busyEvidence });
    expect(busySnap?.foregroundProcess ?? "").toContain("vim");
    expect(normPath(busySnap?.pendingDirectory ?? "")).toBe(normPath(cPath));
    expect(normPath(busySnap?.currentDirectory ?? "")).toBe(normPath(ptyCwd));
    expect(normPath(sessionTarget ?? "")).toBe(normPath(cPath));
    expect(busySnap?.screen ?? "").not.toContain("__cd__");
    expect(busySnap?.screen ?? "").toContain("fleqi-005-vim-live");

    // 6) 退出 vim 到安全空提示符 → 自动同步到 C。
    expect(await sendRaw(":q!\r")).toBe(true);

    let finalSnap: Snapshot | null = null;
    let syncedToC = false;
    const deadline = Date.now() + 20_000;
    while (Date.now() < deadline) {
      finalSnap = await snapshot();
      if (finalSnap && finalSnap.shellReadiness === "ready") {
        if (normPath(finalSnap.currentDirectory) === normPath(cPath)) {
          syncedToC = true;
          break;
        }
      }
      await new Promise((resolve) => setTimeout(resolve, 300));
    }
    writeEvidence("m11-final-sync.json", {
      capturedAt: new Date().toISOString(),
      syncedToC,
      final: finalSnap
        ? {
            readiness: finalSnap.shellReadiness,
            currentDirectory: finalSnap.currentDirectory,
            pendingDirectory: finalSnap.pendingDirectory,
            screenTail: finalSnap.screen.slice(-400),
          }
        : null,
    });
    expect(finalSnap?.shellReadiness).toBe("ready");
    expect(finalSnap?.foregroundProcess).toBeNull();
    expect(syncedToC).toBe(true);
    fs.rmSync("/tmp/fleqi-005-note.txt", { force: true });
  });
});
