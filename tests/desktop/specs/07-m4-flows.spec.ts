import { $, browser, expect } from "@wdio/globals";
import { writeEvidence } from "../lib/evidence";

// M4 · AC-FLOW 整矩阵原生运行（真实 IPC + 真实 zsh + 真实 surface 状态机）：
// 003 keepAll 会话保留与切换回 A；004 endAll（经真实设置 UI 切换）主动隐藏结束全部；
// 006/014 忙碌终端的排队命令与取消恢复草稿；007 Run 上下文独立；
// 008 yolo（经真实设置 UI 切换）下未知效果免确认；010 热键注册失败保留旧绑定。

type Outcome = { ok: boolean; value?: unknown; code?: string; message?: string };

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

describe("M4 · AC-FLOW 矩阵（真实 IPC + 真实设置 UI）", () => {
  it("AC-FLOW-003：keepAll 隐藏保留 A；再唤起建 B；切回 A 会话仍活跃", async () => {
    await switchToWindowWithHash("#/console");
    await browser.execute(() =>
      window.__TAURI_INTERNALS__
        .invoke("hotkey_commit", { requestId: "m4-hotkey-003", accelerator: "CommandOrControl+Shift+D" })
        .then((value: unknown) => ({ ok: true, value }), (error: { code?: string }) => ({ ok: false, code: error.code })),
    );
    const summary = (await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      const first = await bridge
        .invoke("surface_show")
        .then((value: { visibleSessionId: string | null }) => value, () => null);
      if (!first?.visibleSessionId) return { step: "show-a", ok: false };
      const sessionA = first.visibleSessionId;
      const hidden = await bridge
        .invoke("surface_hide")
        .then((value: unknown) => ({ ok: true, value }), () => ({ ok: false }));
      if (!hidden.ok) return { step: "hide-a", ok: false };
      const second = await bridge
        .invoke("surface_show")
        .then((value: { visibleSessionId: string | null }) => value, () => null);
      if (!second?.visibleSessionId) return { step: "show-b", ok: false };
      const sessionB = second.visibleSessionId;
      const backToA = await bridge
        .invoke("session_select", { requestId: "m4-select-a", sessionId: sessionA })
        .then((value: { id: string; state: string }) => value, (error: { code?: string }) => ({ code: error.code }));
      const list = await bridge
        .invoke("session_list", { offset: 0, limit: 50 })
        .then((value: { active: { id: string; state: string }[]; history: { id: string; state: string }[] }) => value, () => null);
      return { step: "done", ok: true, sessionA, sessionB, backToA, activeStates: list?.active.map((s) => `${s.id}:${s.state}`) ?? [] };
    })) as Record<string, unknown>;
    expect(summary.ok).toBe(true);
    expect(summary.step).toBe("done");
    expect(summary.sessionA).not.toBe(summary.sessionB);
    const backToA = summary.backToA as { id?: string; state?: string; code?: string };
    expect(backToA.id ?? "").toBe(summary.sessionA);
    const states = summary.activeStates as string[];
    expect(states.filter((entry) => entry.endsWith(":active")).length).toBeGreaterThanOrEqual(2);
  });

  it("AC-FLOW-004：endAll（经真实设置 UI）主动隐藏结束全部", async () => {
    await switchToWindowWithHash("#/console");
    await $("button[data-sidebar='menu-button']*=设置").click();
    await switchToWindowWithHash("#/settings");
    await $("[data-phase='ready']").waitForExist({ timeout: 20_000 });
    // run-a 的 spec 文件为并行 worker：设置窗口可能被 02 号 spec 同时导航走。
    // 点击后未见“已保存”则回通用页重试（最多 3 次）；endAll 是否真正生效由下方
    // surface_hide 后的会话终态断言把关，不依赖此处的保存回执作唯一证据。
    let saved = false;
    for (let attempt = 0; attempt < 3 && !saved; attempt += 1) {
      await $("button[data-sidebar='menu-button']*=通用").click();
      const endAllRadio = $("button*=结束全部");
      await endAllRadio.waitForExist({ timeout: 10_000 });
      await endAllRadio.click();
      saved = await browser
        .waitUntil(async () => (await $("span*=已保存").isExisting()), { timeout: 6_000 })
        .then(() => true, () => false);
    }
    await browser.waitUntil(async () => (await $("span*=已保存").isExisting()), { timeout: 15_000 });
    await switchToWindowWithHash("#/console");
    const summary = (await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      const shown = await bridge
        .invoke("surface_show")
        .then((value: { visibleSessionId: string | null }) => value, () => null);
      if (!shown?.visibleSessionId) return { step: "show", ok: false };
      const beforeList = await bridge
        .invoke("session_list", { offset: 0, limit: 50 })
        .then((value: { active: { id: string }[] }) => value.active.map((s) => s.id), () => null);
      if (!beforeList) return { step: "before-list", ok: false };
      const hidden = await bridge
        .invoke("surface_hide")
        .then((value: unknown) => ({ ok: true }), () => ({ ok: false }));
      if (!hidden.ok) return { step: "hide", ok: false };
      // endAll 后会话经异步 PTY 关闭才落到 ended：轮询等待，不拍单帧快照。
      let missing: string[] = [];
      let missingStates: Record<string, string> = {};
      let endedCount = 0;
      for (let i = 0; i < 24; i += 1) {
        const after = await bridge
          .invoke("session_list", { offset: 0, limit: 50 })
          .then(
            (value: { active: { id: string; state: string }[]; history: { id: string; state: string }[] }) => ({
              all: [...value.active, ...value.history].map((s) => [s.id, s.state] as const),
            }),
            () => null,
          );
        if (!after) return { step: "after-list", ok: false };
        // endAll 的合同语义：endAll 前活跃的每个会话（按 ID 追踪）最终都已结束；
        // 不按 ended 总数断言，避免并行 worker 共享数据目录下的并发增删干扰。
        const stateById = new Map(after.all);
        endedCount = [...stateById.values()].filter((state) => state === "ended").length;
        missing = beforeList.filter((id) => stateById.get(id) !== "ended");
        missingStates = Object.fromEntries(missing.map((id) => [id, stateById.get(id) ?? "（不存在）"]));
        if (missing.length === 0) break;
        await new Promise((resolve) => setTimeout(resolve, 500));
      }
      return { step: "done", ok: missing.length === 0, beforeCount: beforeList.length, endedCount, missing, missingStates };
    })) as Record<string, unknown>;
    writeEvidence("m4-flow-004.json", { capturedAt: new Date().toISOString(), ...summary });
    expect(summary.ok).toBe(true);
    expect(summary.step).toBe("done");
    expect(summary.beforeCount as number).toBeGreaterThan(0);
    // endAll 结束的是隐藏时刻的全部活跃会话；此后其它进程新建的会话不受其约束。
    expect(summary.missing as string[]).toEqual([]);
  });

  it("AC-FLOW-006/014：忙碌终端排队命令等待目录同步；取消恢复草稿不执行", async () => {
    const summary = (await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      // 先取得真实 Finder 目录（本机 Finder 自动化已 allowed；01-bootstrap 同源验证），
      // 会话创建时携带目录，终端才能打开。
      const refreshed = await bridge
        .invoke("context_refresh")
        .then((value: { directoryRef: { displayPath: string } | null }) => value, () => null);
      if (!refreshed?.directoryRef) return { step: "context", ok: false };
      const shown = await bridge
        .invoke("surface_show")
        .then((value: { visibleSessionId: string | null }) => value, () => null);
      if (!shown?.visibleSessionId) return { step: "show", ok: false };
      const sessionId = shown.visibleSessionId;
      const opened = await bridge
        .invoke("terminal_open", { sessionId, cols: 100, rows: 30 })
        .then(() => ({ ok: true }), (error: { message?: string }) => ({ ok: false, message: error.message }));
      if (!opened.ok) return { step: "terminal_open", ok: false, message: (opened as { message?: string }).message };
      // 真正持有租约并等待安全提示符；不能吞掉输入被拒后仍声称终端忙碌。
      const lease = await bridge.invoke("terminal_acquire_lease", { sessionId, owner: "m4-queued" });
      const waitForShell = async (ready: boolean, directory?: string) => {
        for (let attempt = 0; attempt < 80; attempt += 1) {
          const snapshot = await bridge.invoke("terminal_snapshot", { sessionId }) as { shellReadiness: string; currentDirectory: string };
          if ((snapshot.shellReadiness === "ready") === ready && (!directory || snapshot.currentDirectory === directory)) return true;
          await new Promise((resolve) => setTimeout(resolve, 100));
        }
        return false;
      };
      if (!await waitForShell(true)) return { step: "initial-prompt", ok: false };
      // 让 PTY 目录与下方 /tmp 目标不同，构造真实的“等待目录同步”。
      const otherDirectory = "/";
      await bridge.invoke("terminal_input", { sessionId, lease, input: Array.from(new TextEncoder().encode(`cd ${otherDirectory}\n`)) });
      if (!await waitForShell(true, otherDirectory)) return { step: "different-directory", ok: false };
      await bridge.invoke("terminal_input", { sessionId, lease, input: Array.from(new TextEncoder().encode("sleep 12\n")) });
      if (!await waitForShell(false)) return { step: "busy", ok: false };
      const context = await bridge
        .invoke("context_get", { contextId: null })
        .then((value: { revision: string }) => value, () => null);
      const request = {
        requestId: "m4-queued-1",
        sessionId,
        line: "!echo queued-marker",
        contextRevision: context ? context.revision : "1",
        targetDisplay: "/tmp",
      };
      const queued = await bridge
        .invoke("terminal_submit_line", { ...request })
        .then(
          (value: "sent" | "queued") => ({ ok: true, result: value }),
          (error: { code?: string; message?: string }) => ({ ok: false, code: error.code, message: error.message }),
        );
      const cancelled = await bridge
        .invoke("terminal_cancel_queued", { sessionId })
        .then(
          (value: { text: string } | null) => ({ ok: true, text: value?.text ?? null }),
          (error: { code?: string }) => ({ ok: false, code: error.code }),
        );
      const snapshot = await bridge
        .invoke("terminal_snapshot", { sessionId })
        .then((value: { screen: string }) => value, () => null);
      await bridge.invoke("session_end", { requestId: "m4-end-busy", sessionId }).then(() => null, () => null);
      return { step: "done", ok: true, queued, cancelled, screenHasMarker: snapshot ? snapshot.screen.includes("queued-marker") : null };
    })) as Record<string, unknown>;
    writeEvidence("m4-flow-006.json", { at: new Date().toISOString(), summary });
    expect(summary.ok).toBe(true);
    const queued = summary.queued as { ok: boolean; result?: string; code?: string; message?: string };
    expect(queued.ok).toBe(true);
    expect(queued.result).toBe("queued");
    const cancelled = summary.cancelled as { ok: boolean; text?: string | null };
    expect(cancelled.ok).toBe(true);
    expect(cancelled.text ?? "").toContain("queued-marker");
    expect(summary.screenHasMarker).toBe(false);
  });

  it("AC-FLOW-007/008：Run 上下文独立；yolo（经真实设置 UI）下未知效果免确认", async () => {
    await switchToWindowWithHash("#/settings");
    await $("button[data-sidebar='menu-button']*=任务与诊断").click();
    const yoloRadio = $("button*=全部免确认");
    await yoloRadio.waitForExist({ timeout: 10_000 });
    await yoloRadio.click();
    await browser.waitUntil(async () => (await $("span*=已保存").isExisting()), { timeout: 15_000 });
    await switchToWindowWithHash("#/console");
    const summary = (await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      const contextA = await bridge.invoke("context_refresh") as { id: string };
      const created = await bridge.invoke("session_create", { requestId: "m4-current-session" }) as { id: string };
      const sessionId = created.id;
      const runA = await bridge
        .invoke("run_submit", {
          requestId: "m4-run-a",
          sessionId,
          prompt: "上下文 A 探针",
          plan: { revision: "61", contextId: contextA.id, scripts: ["printf run-a-ok"], effects: ["unknown"], previewComplete: true },
        })
        .then((value: { id: string; state: string; contextId: string }) => value, () => null);
      if (!runA) return { step: "run", ok: false };
      const deadline = Date.now() + 20000;
      let state = "";
      while (Date.now() < deadline) {
        const record = await bridge
          .invoke("run_get", { runId: runA.id })
          .then((value: { state: string }) => value, () => null);
        state = record?.state ?? "";
        if (["succeeded", "failed", "cancelled"].includes(state)) break;
        await new Promise((resolve) => setTimeout(resolve, 150));
      }
      await bridge.invoke("context_refresh");
      const persisted = await bridge.invoke("run_get", { runId: runA.id }) as { contextId: string };
      return { step: "done", ok: true, state, contextId: persisted.contextId, expectedContextId: contextA.id };
    })) as Record<string, unknown>;
    expect(summary.ok).toBe(true);
    // yolo：未知效果免确认直接执行（FR-POLICY-002）。
    expect(summary.state).toBe("succeeded");
    // Run 记录保持提交时明示的上下文，不随 Finder 漂移。
    expect(summary.contextId).toBe(summary.expectedContextId);
  });

  it("AC-FLOW-010：注册失败保留旧有效绑定；清除后为未绑定", async () => {
    const committed = (await browser.execute(() =>
      window.__TAURI_INTERNALS__
        .invoke("hotkey_commit", { requestId: "m4-hotkey-old", accelerator: "CommandOrControl+Shift+G" })
        .then((value: { registered: string | null; message: string | null }) => ({ ok: value.registered != null, registered: value.registered, message: value.message }), (error: { message?: string }) => ({ ok: false, registered: null as string | null, message: error.message })),
    )) as { ok: boolean; registered?: string | null; message?: string };
    writeEvidence("m4-flow-010.json", { at: new Date().toISOString(), committed });
    expect(committed.ok).toBe(true);
    expect(committed.registered).toBe("CommandOrControl+Shift+G");

    // 非法候选：注册失败以 message 反馈（Ok 携带失败说明）或 IPC 错误，二者都算被拒。
    const failed = (await browser.execute(() =>
      window.__TAURI_INTERNALS__
        .invoke("hotkey_commit", { requestId: "m4-hotkey-bad", accelerator: "NotARealKey" })
        .then((value: { registered: string | null; message: string | null }) => ({ refused: value.message != null, registered: value.registered, message: value.message }), (error: { code?: string; message?: string }) => ({ refused: true, registered: null as string | null, message: error.message })),
    )) as { refused: boolean; registered?: string | null; message?: string | null };
    expect(failed.refused).toBe(true);

    const current = (await browser.execute(() =>
      window.__TAURI_INTERNALS__
        .invoke("hotkey_get")
        .then((value: { registered: string | null }) => ({ ok: true, registered: value.registered }), () => ({ ok: false })),
    )) as { ok: boolean; registered?: string | null };
    expect(current.ok).toBe(true);
    // 失败不覆盖旧有效绑定。
    expect(current.registered).toBe("CommandOrControl+Shift+G");

    const cleared = (await browser.execute(() =>
      window.__TAURI_INTERNALS__
        .invoke("hotkey_clear", { requestId: "m4-hotkey-clear" })
        .then((value: { registered: string | null }) => ({ ok: true, registered: value.registered }), () => ({ ok: false })),
    )) as { ok: boolean; registered?: string | null };
    expect(cleared.ok).toBe(true);
    expect(cleared.registered).toBeNull();

    writeEvidence("m4-flows-evidence.json", {
      capturedAt: new Date().toISOString(),
      hotkeyRetainedAfterFailure: current.registered,
      hotkeyAfterClear: cleared.registered,
      note: "AC-FLOW-003/004/006/007/008/010/014 原生矩阵：keepAll 保留与切回、endAll（真实设置 UI 切换）结束全部、忙碌排队与取消恢复草稿、Run 上下文独立、yolo（真实设置 UI）免确认、注册失败保留旧绑定与清除。",
    });
  });
});
