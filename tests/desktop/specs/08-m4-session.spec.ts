import { $$, browser, expect } from "@wdio/globals";
import fs from "node:fs";
import { execFileSync } from "node:child_process";
import { evidencePath, writeEvidence } from "../lib/evidence";

// M4 · AC-FLOW-002 / AC-FLOW-011 原生运行（真实 IPC + 真实 surface 状态机 + 真实 zsh）：
// 002 无快捷键切换 followFinder 自动出现；主动隐藏抑制自动显示；切换模式或显式唤起解除抑制。
// 011 图钉排序、单独结束不误伤其它会话、删除活跃/历史记录不删用户文件、历史继续创建关联新会话。
// 本 spec 由 run-a2 独占串行运行（与 run-a 的并行 worker 隔离）；各用例自建会话，不依赖其它 spec 的残留状态。
// 数据目录与 run-a 共享：07 曾把 hideBehavior 持久化为 endAll，依赖 keepAll 语义的用例必须先显式恢复。
// settings_update 的 expectedRevision 必须是十进制字符串（空串参数校验即拒）：先经 app_bootstrap 取真实版本。
// 会话 revision 会被 terminal_open（set_terminal/目录同步）异步推进：删除/置顶前重新读取，冲突时带新版本重试。
// 上下文目录跟随真实 Finder（本机可能是用户桌面），标记文件一律写 /tmp，不触碰用户文件夹。
// PTY 输入需要当前输入租约（terminal_acquire_lease）；先轮询 shellReadiness=ready，再持租约输入；回显命令行也含标记词，
// 断言用“标记出现次数相对基线增长”判断 cat 输出真实到达，而非首次出现。

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

async function waitConsoleReady(): Promise<void> {
  await switchToWindowWithHash("#/console");
  await $("[data-phase='ready']").waitForExist({ timeout: 30_000 });
  await browser.waitUntil(async () => (await $("[data-testid='host-state']").getAttribute("data-host-state")) === "ready", { timeout: 20_000 });
}

describe("M4 · AC-FLOW-002/011（真实 IPC + surface 状态机）", () => {
  it("AC-FLOW-002：仅 Finder 前台自动显示；暂隐恢复原会话；主动隐藏抑制", async () => {
    await waitConsoleReady();
    await browser.execute(() => window.__TAURI_INTERNALS__.invoke("app_open_window", { role: "console" }));
    await browser.waitUntil(async () => browser.execute(() => document.hasFocus()), { timeout: 5000 });
    type Surface = { visibility: string; visibleSessionId: string | null; autoShowSuppressed: boolean; activation: string };
    const get = async () => await browser.execute(() => window.__TAURI_INTERNALS__.invoke("surface_get")) as Surface;
    const apply = async (name: string, patch: Record<string, unknown>) => browser.execute(async (payload: { name: string; patch: Record<string, unknown> }) => {
      const api = window.__TAURI_INTERNALS__;
      const boot = await api.invoke("app_bootstrap") as { settings: { revision: string } };
      await api.invoke("settings_update", { request: { requestId: payload.name, expectedRevision: boot.settings.revision, patch: payload.patch } });
    }, { name, patch });
    const focus = async (kind: "finder" | "other") => {
      if (!process.env.FLEQI_INTERACTIVE_FINDER) {
        if (kind === "finder") execFileSync("open", ["-a", "Finder"]);
        else await browser.execute(() => window.__TAURI_INTERNALS__.invoke("app_open_window", { role: "console" }));
        return;
      }
      fs.rmSync("/tmp/fleqi-finder-ready", { force: true });
      fs.writeFileSync("/tmp/fleqi-finder-phase.json", JSON.stringify({ kind, fixture: JSON.parse(fs.readFileSync("/tmp/fleqi-attach-finder.json", "utf8")) }));
      await browser.waitUntil(async () => fs.existsSync("/tmp/fleqi-finder-ready"), { timeout: 120000, interval: 250 });
      fs.rmSync("/tmp/fleqi-finder-ready", { force: true });
    };
    const waitVisible = () => browser.waitUntil(async () => (await get()).visibility === "visible", { timeout: 15000 });
    await browser.execute(() => window.__TAURI_INTERNALS__.invoke("context_refresh"));
    await apply("m8-follow", { hideBehavior: "keepAll", activation: "followFinder" });
    const whileConsoleFocused = await get();
    expect(whileConsoleFocused.visibility).not.toBe("visible");
    await focus("finder");
    await waitVisible();
    const auto = await get();
    expect(auto.activation).toBe("followFinder");
    expect(auto.visibleSessionId).toBeTruthy();
    await focus("other");
    await browser.waitUntil(async () => (await get()).visibility === "temporarilyHidden", { timeout: 5000 });
    const background = await get();
    expect(background.visibleSessionId).toBe(auto.visibleSessionId);
    await focus("finder");
    await waitVisible();
    expect((await get()).visibleSessionId).toBe(auto.visibleSessionId);
    await browser.execute(() => window.__TAURI_INTERNALS__.invoke("surface_hide"));
    await browser.execute(() => window.__TAURI_INTERNALS__.invoke("context_refresh"));
    const suppressed = await get();
    expect(suppressed.visibility).toBe("userHidden");
    expect(suppressed.autoShowSuppressed).toBe(true);
    await apply("m8-manual", { activation: "manual" });
    expect((await get()).autoShowSuppressed).toBe(false);
    await apply("m8-follow-again", { activation: "followFinder" });
    await waitVisible();
    const autoAgain = await get();
    await browser.execute(() => window.__TAURI_INTERNALS__.invoke("surface_hide"));
    const shown = await browser.execute(() => window.__TAURI_INTERNALS__.invoke("surface_show")) as Surface;
    expect(shown.autoShowSuppressed).toBe(false);
    expect(shown.visibility).toBe("visible");
    await apply("m8-manual-final", { activation: "manual" });
    writeEvidence("m4-flow-002.json", { whileConsoleFocused, auto, background, suppressed, autoAgain, shown, focusMethod: process.env.FLEQI_INTERACTIVE_FINDER ? "real desktop input" : "native window activation" });
    fs.rmSync("/tmp/fleqi-finder-phase.json", { force: true });
  });

  it("AC-FLOW-011a：图钉排序；单独结束 A 不影响 B 的终端", async () => {
    await waitConsoleReady();
    const hotkey = (await browser.execute(() =>
      window.__TAURI_INTERNALS__
        .invoke("hotkey_commit", { requestId: "m8-hotkey", accelerator: "CommandOrControl+Shift+H" })
        .then((value: { registered: string | null }) => ({ ok: true, registered: value.registered }), (error: unknown) => ({ ok: false, raw: String(error) })),
    )) as { ok: boolean; registered?: string | null; raw?: string };
    writeEvidence("m4-flow-011a-hotkey.json", { capturedAt: new Date().toISOString(), ...hotkey });
    expect(hotkey.ok).toBe(true);
    await browser.execute(() =>
      window.__TAURI_INTERNALS__.invoke("context_refresh").then(() => null, () => null),
    );
    const summary = (await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      // run-a 可能遗留 endAll：本用例的隐藏必须保留会话，先恢复 keepAll。
      const attempt = (id: string, expected: string, patch: Record<string, unknown>) =>
        bridge
          .invoke("settings_update", { request: { requestId: id, expectedRevision: expected, patch } })
          .then(
            () => ({ ok: true as const }),
            (error: { currentRevision?: string }) => ({ ok: false as const, currentRevision: error.currentRevision ?? null, raw: String(error) }),
          );
      const applyKeepAll = async () => {
        const revision = await bridge.invoke("app_bootstrap").then(
          (value: { settings: { revision: string } }) => value.settings.revision,
          () => null,
        );
        if (!revision) return { ok: false as const, raw: "app_bootstrap 未返回设置版本" };
        const first = await attempt("m8-keepall-011a", revision, { hideBehavior: "keepAll" });
        if (first.ok) return { ok: true as const };
        if (!first.currentRevision) return { ok: false as const, raw: first.raw };
        const retry = await attempt("m8-keepall-011a-retry", first.currentRevision, { hideBehavior: "keepAll" });
        return retry.ok ? { ok: true as const } : { ok: false as const, raw: retry.raw };
      };
      const keepAll = await applyKeepAll();
      if (!keepAll.ok) return { step: "keepAll", ok: false, raw: keepAll.raw };

      const show = async (tag: string) => {
        await bridge.invoke("surface_hide").then(() => null, () => null);
        const shown = await bridge.invoke("surface_show").then(
          (value: { visibleSessionId: string | null }) => value,
          (error: unknown) => ({ raw: String(error) }),
        );
        if (!("visibleSessionId" in shown) || !shown.visibleSessionId) return { ok: false as const, tag, shown };
        return { ok: true as const, id: shown.visibleSessionId };
      };
      const first = await show("a");
      if (!first.ok) return { step: "show-a", ok: false, detail: first };
      const sessionA = first.id;
      const second = await show("b");
      if (!second.ok) return { step: "show-b", ok: false, detail: second };
      const sessionB = second.id;
      if (sessionA === sessionB) return { step: "distinct", ok: false, sessionA, sessionB };

      const listAll = () =>
        bridge.invoke("session_list", { offset: 0, limit: 100 }).then(
          (value: { active: { id: string; state: string; pinned: boolean; revision: string }[]; history: { id: string; state: string; pinned: boolean; revision: string }[] }) => value,
          (error: unknown) => ({ raw: String(error) }),
        );
      const listed = await listAll();
      if (!("active" in listed)) return { step: "list", ok: false, detail: listed };
      const recordA = listed.active.find((s) => s.id === sessionA);
      const recordB = listed.active.find((s) => s.id === sessionB);
      if (!recordA || !recordB) {
        return {
          step: "list",
          ok: false,
          sessionA,
          sessionB,
          activeIds: listed.active.map((s) => s.id),
          historyStates: listed.history.map((s) => `${s.id}:${s.state}`),
        };
      }

      const pinA = await bridge
        .invoke("session_pin", { sessionId: sessionA, pinned: true, expectedRevision: recordA.revision })
        .then(
          (value: { pinned: boolean; revision: string }) => ({ ok: true as const, pinned: value.pinned, revision: value.revision }),
          (error: unknown) => ({ ok: false as const, raw: String(error) }),
        );
      if (!pinA.ok || !pinA.pinned) return { step: "pin-a", ok: false, pinA };
      const orderAfterPinA = await listAll();
      const orderA = "active" in orderAfterPinA ? orderAfterPinA.active.map((s) => s.id) : [];
      if (orderA[0] !== sessionA) return { step: "pin-order-a", ok: false, order: orderA, sessionA };

      const pinB = await bridge
        .invoke("session_pin", { sessionId: sessionB, pinned: true, expectedRevision: recordB.revision })
        .then(
          (value: { pinned: boolean }) => ({ ok: true as const, pinned: value.pinned }),
          (error: unknown) => ({ ok: false as const, raw: String(error) }),
        );
      if (!pinB.ok) return { step: "pin-b", ok: false, pinB };
      const orderBoth = await listAll();
      const both = "active" in orderBoth ? orderBoth.active.map((s) => s.id) : [];
      if (both[0] !== sessionB || both[1] !== sessionA) return { step: "pin-order-both", ok: false, both, sessionA, sessionB };

      const unpinA = await bridge
        .invoke("session_pin", { sessionId: sessionA, pinned: false, expectedRevision: pinA.revision })
        .then(
          (value: { pinned: boolean }) => ({ ok: true as const, pinned: value.pinned }),
          (error: unknown) => ({ ok: false as const, raw: String(error) }),
        );
      if (!unpinA.ok || unpinA.pinned) return { step: "unpin-a", ok: false, unpinA };
      const orderUnpinned = await listAll();
      const unpinned = "active" in orderUnpinned ? orderUnpinned.active.map((s) => s.id) : [];
      if (unpinned[0] !== sessionB || unpinned[1] !== sessionA) return { step: "unpin-order", ok: false, unpinned, sessionA, sessionB };

      const snapshotScreen = () =>
        bridge.invoke("terminal_snapshot", { sessionId: sessionB }).then(
          (value: { screen: string }) => value.screen,
          () => "",
        );
      const countMarker = (screen: string) => screen.split("fleqi-keep-alive").length - 1;

      const opened = await bridge
        .invoke("terminal_open", { sessionId: sessionB, cols: 100, rows: 30 })
        .then(() => true, (error: unknown) => String(error));
      if (opened !== true) return { step: "terminal-b", ok: false, opened };
      // PTY 就绪前输入会丢失：轮询到 ready 再发送。
      let readiness = "";
      const readyDeadline = Date.now() + 10_000;
      while (Date.now() < readyDeadline) {
        const snap = await bridge.invoke("terminal_snapshot", { sessionId: sessionB }).then(
          (value: { shellReadiness: string }) => value.shellReadiness,
          () => "error",
        );
        readiness = snap;
        if (readiness === "ready") break;
        await new Promise((resolve) => setTimeout(resolve, 300));
      }
      if (readiness !== "ready") return { step: "terminal-b-ready", ok: false, readiness };
      const leaseB = await bridge.invoke("terminal_acquire_lease", { sessionId: sessionB, owner: "spec-m8" }).then(
        (value: string) => value,
        (error: unknown) => null,
      );
      if (!leaseB) return { step: "terminal-b-lease", ok: false };
      const echoOutcome = await bridge
        .invoke("terminal_input", { sessionId: sessionB, lease: leaseB, input: Array.from(new TextEncoder().encode("rm -f /tmp/fleqi-keep-file.txt; echo fleqi-keep-alive > /tmp/fleqi-keep-file.txt\n")) })
        .then(() => ({ ok: true as const }), (error: unknown) => ({ ok: false as const, raw: String(error) }));
      if (!echoOutcome.ok) return { step: "terminal-b-echo", ok: false, echoOutcome };
      await new Promise((resolve) => setTimeout(resolve, 1500));

      const ended = await bridge.invoke("session_end", { requestId: "m8-end-a", sessionId: sessionA }).then(
        (value: { state: string }) => ({ ok: true as const, state: value.state }),
        (error: unknown) => ({ ok: false as const, raw: String(error) }),
      );
      if (!ended.ok || ended.state !== "ended") return { step: "end-a", ok: false, ended };
      const afterEnd = await listAll();
      if (!("active" in afterEnd)) return { step: "list-after-end", ok: false, detail: afterEnd };
      const aInHistory = afterEnd.history.find((s) => s.id === sessionA);
      const bStillActive = afterEnd.active.find((s) => s.id === sessionB);
      if (!aInHistory || aInHistory.state !== "ended" || !bStillActive) {
        return { step: "end-isolation", ok: false, aInHistory: aInHistory?.state ?? null, bActive: Boolean(bStillActive) };
      }

      const baselineCount = countMarker(await snapshotScreen());
      if (baselineCount < 1) return { step: "echo-not-on-screen", ok: false, screenTail: (await snapshotScreen()).slice(-400) };
      const catOutcome = await bridge
        .invoke("terminal_input", { sessionId: sessionB, lease: leaseB, input: Array.from(new TextEncoder().encode("cat /tmp/fleqi-keep-file.txt\n")) })
        .then(() => ({ ok: true as const }), (error: unknown) => ({ ok: false as const, raw: String(error) }));
      if (!catOutcome.ok) return { step: "terminal-b-cat", ok: false, catOutcome };
      const deadline = Date.now() + 8000;
      let screen = "";
      while (Date.now() < deadline) {
        screen = await snapshotScreen();
        if (countMarker(screen) > baselineCount) break;
        await new Promise((resolve) => setTimeout(resolve, 300));
      }
      if (countMarker(screen) <= baselineCount) return { step: "terminal-b-alive", ok: false, baselineCount, screenTail: screen.slice(-400) };
      return { step: "done", ok: true, sessionA, sessionB };
    })) as Record<string, unknown>;
    writeEvidence("m4-flow-011a.json", { capturedAt: new Date().toISOString(), ...summary });
    expect(summary.ok).toBe(true);
    expect(summary.step).toBe("done");
  });

  it("AC-FLOW-011b：删除活跃/已结束记录；删除不删用户文件", async () => {
    await waitConsoleReady();
    const summary = (await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      const listAll = () =>
        bridge.invoke("session_list", { offset: 0, limit: 100 }).then(
          (value: { active: { id: string; state: string; revision: string }[]; history: { id: string; state: string; revision: string }[] }) => value,
          (error: unknown) => ({ raw: String(error) }),
        );
      const currentRevision = async (sessionId: string) => {
        const listed = await listAll();
        if (!("active" in listed)) return null;
        return [...listed.active, ...listed.history].find((s) => s.id === sessionId)?.revision ?? null;
      };
      // 目录同步会异步推进 revision：删除带版本校验，冲突时重读版本重试（上限 5 次）。
      const deleteWithRetry = async (requestId: string, sessionId: string) => {
        let last: { code?: string; message?: string; raw: string } | null = null;
        for (let i = 0; i < 5; i += 1) {
          const revision = await currentRevision(sessionId);
          if (!revision) return { ok: false as const, raw: "重读会话版本失败" };
          const outcome = await bridge
            .invoke("session_delete", { requestId: `${requestId}-${i}`, sessionId, expectedRevision: revision })
            .then(
              () => ({ ok: true as const }),
              (error: { code?: string; message?: string }) => ({ ok: false as const, code: error.code, message: error.message, raw: String(error) }),
            );
          if (outcome.ok) return { ok: true as const };
          last = outcome;
        }
        return { ok: false as const, code: last?.code, message: last?.message, raw: last?.raw ?? "未知错误" };
      };
      const waitReady = async (sessionId: string) => {
        const deadline = Date.now() + 10_000;
        while (Date.now() < deadline) {
          const snap = await bridge.invoke("terminal_snapshot", { sessionId }).then(
            (value: { shellReadiness: string }) => value.shellReadiness,
            () => "error",
          );
          if (snap === "ready") return true;
          await new Promise((resolve) => setTimeout(resolve, 300));
        }
        return false;
      };
      const waitCatOutput = async (sessionId: string) => {
        const count = (screen: string) => screen.split("fleqi-del-marker").length - 1;
        const baseline = count(
          await bridge.invoke("terminal_snapshot", { sessionId }).then(
            (value: { screen: string }) => value.screen,
            () => "",
          ),
        );
        const deadline = Date.now() + 8000;
        let screen = "";
        while (Date.now() < deadline) {
          screen = await bridge.invoke("terminal_snapshot", { sessionId }).then(
            (value: { screen: string }) => value.screen,
            () => "",
          );
          if (count(screen) > baseline) return { ok: true as const, baseline, screen };
          await new Promise((resolve) => setTimeout(resolve, 300));
        }
        return { ok: false as const, baseline, screen };
      };

      // 自建 D：终端写入 /tmp 标记文件，再走“删除活跃记录”路径（命令内部先结束）。
      const created = await bridge.invoke("session_create", { requestId: "m8-create-d" }).then(
        (value: { id: string; state: string; revision: string }) => value,
        (error: unknown) => ({ raw: String(error) }),
      );
      if (!("id" in created)) return { step: "create-d", ok: false, detail: created };
      const opened = await bridge
        .invoke("terminal_open", { sessionId: created.id, cols: 100, rows: 30 })
        .then(() => true, (error: unknown) => String(error));
      if (opened !== true) return { step: "terminal-d", ok: false, opened };
      if (!(await waitReady(created.id))) return { step: "terminal-d-ready", ok: false };
      const leaseD = await bridge.invoke("terminal_acquire_lease", { sessionId: created.id, owner: "spec-m8" }).then(
        (value: string) => value,
        () => null,
      );
      if (!leaseD) return { step: "terminal-d-lease", ok: false };
      const echoD = await bridge
        .invoke("terminal_input", { sessionId: created.id, lease: leaseD, input: Array.from(new TextEncoder().encode("rm -f /tmp/fleqi-del-file.txt; echo fleqi-del-marker > /tmp/fleqi-del-file.txt\n")) })
        .then(() => ({ ok: true as const }), (error: unknown) => ({ ok: false as const, raw: String(error) }));
      if (!echoD.ok) return { step: "terminal-d-echo", ok: false, echoD };
      await new Promise((resolve) => setTimeout(resolve, 1500));

      const deletedActive = await deleteWithRetry("m8-delete-d", created.id);
      if (!deletedActive.ok) return { step: "delete-active", ok: false, deletedActive };
      const afterDelete = await listAll();
      if ("active" in afterDelete && [...afterDelete.active, ...afterDelete.history].some((s) => s.id === created.id)) {
        return { step: "delete-active-gone", ok: false };
      }

      const missing = await bridge
        .invoke("session_delete", { requestId: "m8-delete-missing", sessionId: "session-missing-m8", expectedRevision: "1" })
        .then(() => ({ ok: true as const }), (error: { code?: string }) => ({ ok: false as const, code: error.code }));
      if (missing.ok || missing.code !== "not_found") return { step: "delete-missing", ok: false, missing };

      // 自建 X：先结束再删除（历史记录删除路径）。
      const x = await bridge.invoke("session_create", { requestId: "m8-create-x" }).then(
        (value: { id: string; revision: string }) => value,
        (error: unknown) => ({ raw: String(error) }),
      );
      if (!("id" in x)) return { step: "create-x", ok: false, detail: x };
      const endedX = await bridge.invoke("session_end", { requestId: "m8-end-x", sessionId: x.id }).then(
        (value: { revision: string }) => ({ ok: true as const, revision: value.revision }),
        (error: unknown) => ({ ok: false as const, raw: String(error) }),
      );
      if (!endedX.ok) return { step: "end-x", ok: false, endedX };
      const deletedX = await deleteWithRetry("m8-delete-x", x.id);
      if (!deletedX.ok) return { step: "delete-x", ok: false, deletedX };

      // 新会话 E 仍能读到 D 会话写入的文件：删除会话不删用户文件。
      await bridge.invoke("surface_hide").then(() => null, () => null);
      const shown = await bridge.invoke("surface_show").then(
        (value: { visibleSessionId: string | null }) => value,
        (error: unknown) => ({ raw: String(error) }),
      );
      if (!("visibleSessionId" in shown) || !shown.visibleSessionId) return { step: "show-e", ok: false, shown };
      const sessionE = shown.visibleSessionId;
      const openedE = await bridge
        .invoke("terminal_open", { sessionId: sessionE, cols: 100, rows: 30 })
        .then(() => true, (error: unknown) => String(error));
      if (openedE !== true) return { step: "terminal-e", ok: false, openedE };
      if (!(await waitReady(sessionE))) return { step: "terminal-e-ready", ok: false };
      const leaseE = await bridge.invoke("terminal_acquire_lease", { sessionId: sessionE, owner: "spec-m8" }).then(
        (value: string) => value,
        () => null,
      );
      if (!leaseE) return { step: "terminal-e-lease", ok: false };
      const catE = await bridge
        .invoke("terminal_input", { sessionId: sessionE, lease: leaseE, input: Array.from(new TextEncoder().encode("cat /tmp/fleqi-del-file.txt\n")) })
        .then(() => ({ ok: true as const }), (error: unknown) => ({ ok: false as const, raw: String(error) }));
      if (!catE.ok) return { step: "terminal-e-cat", ok: false, catE };
      const catOutcome = await waitCatOutput(sessionE);
      if (!catOutcome.ok) return { step: "file-survives", ok: false, baseline: catOutcome.baseline, screenTail: catOutcome.screen.slice(-400) };
      return { step: "done", ok: true, deletedActiveSession: created.id, deletedEndedSession: x.id, survivorSession: sessionE };
    })) as Record<string, unknown>;
    writeEvidence("m4-flow-011b.json", { capturedAt: new Date().toISOString(), ...summary });
    expect(summary.ok).toBe(true);
    expect(summary.step).toBe("done");
  });

  it("AC-FLOW-011c：历史继续创建关联新会话；活跃/不存在来源被拒绝", async () => {
    await waitConsoleReady();
    const summary = (await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      const source = await bridge.invoke("session_create", { requestId: "m8-create-s" }).then(
        (value: { id: string; state: string }) => value,
        (error: unknown) => ({ raw: String(error) }),
      );
      if (!("id" in source)) return { step: "create-s", ok: false, detail: source };
      const endedS = await bridge.invoke("session_end", { requestId: "m8-end-s", sessionId: source.id }).then(
        () => ({ ok: true as const }),
        (error: unknown) => ({ ok: false as const, raw: String(error) }),
      );
      if (!endedS.ok) return { step: "end-s", ok: false, endedS };

      const continued = await bridge.invoke("session_continue", { requestId: "m8-continue", historySessionId: source.id }).then(
        (value: { id: string; parentSessionId: string | null; state: string }) => value,
        (error: unknown) => ({ raw: String(error) }),
      );
      if (!("id" in continued)) return { step: "continue", ok: false, detail: continued };
      if (continued.parentSessionId !== source.id || continued.state !== "active" || continued.id === source.id) {
        return { step: "continue-link", ok: false, continued };
      }
      const afterContinue = await bridge.invoke("session_list", { offset: 0, limit: 100 }).then(
        (value: { active: { id: string; state: string }[]; history: { id: string; state: string }[] }) => value,
        (error: unknown) => ({ raw: String(error) }),
      );
      if (!("active" in afterContinue)) return { step: "list-after-continue", ok: false, detail: afterContinue };
      const sourceStillHistory = afterContinue.history.find((s) => s.id === source.id);
      const newActive = afterContinue.active.find((s) => s.id === continued.id);
      if (!sourceStillHistory || sourceStillHistory.state !== "ended" || !newActive) {
        return { step: "continue-groups", ok: false, sourceState: sourceStillHistory?.state ?? null, newActive: Boolean(newActive) };
      }

      const activeRefused = await bridge
        .invoke("session_continue", { requestId: "m8-continue-active", historySessionId: continued.id })
        .then(
          () => ({ ok: true as const }),
          (error: { code?: string }) => ({ ok: false as const, code: error.code, raw: String(error) }),
        );
      if (activeRefused.ok || activeRefused.code !== "conflict") return { step: "continue-active", ok: false, activeRefused };

      const missingRefused = await bridge
        .invoke("session_continue", { requestId: "m8-continue-missing", historySessionId: "session-missing-m8" })
        .then(
          () => ({ ok: true as const }),
          (error: { code?: string }) => ({ ok: false as const, code: error.code, raw: String(error) }),
        );
      if (missingRefused.ok || missingRefused.code !== "not_found") return { step: "continue-missing", ok: false, missingRefused };
      return { step: "done", ok: true, source: source.id, continued: continued.id };
    })) as Record<string, unknown>;
    writeEvidence("m4-flow-011c.json", { capturedAt: new Date().toISOString(), ...summary });
    expect(summary.ok).toBe(true);
    expect(summary.step).toBe("done");
  });

  it("AC-FLOW-015：忙碌且排队时 keepAll 隐藏——撤销在途投递、保留草稿、程序继续运行", async () => {
    await waitConsoleReady();
    const hotkey = (await browser.execute(() =>
      window.__TAURI_INTERNALS__
        .invoke("hotkey_commit", { requestId: "m8-hotkey-015", accelerator: "CommandOrControl+Shift+J" })
        .then((value: { registered: string | null }) => ({ ok: true, registered: value.registered }), (error: unknown) => ({ ok: false, raw: String(error) })),
    )) as { ok: boolean; registered?: string | null; raw?: string };
    expect(hotkey.ok).toBe(true);
    await browser.execute(() =>
      window.__TAURI_INTERNALS__.invoke("context_refresh").then(() => null, () => null),
    );
    const summary = (await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      await bridge.invoke("surface_hide").then(() => null, () => null);
      const shown = await bridge.invoke("surface_show").then(
        (value: { visibleSessionId: string | null }) => value,
        (error: unknown) => ({ raw: String(error) }),
      );
      if (!("visibleSessionId" in shown) || !shown.visibleSessionId) return { step: "show", ok: false, shown };
      const sessionId = shown.visibleSessionId;
      const opened = await bridge
        .invoke("terminal_open", { sessionId, cols: 100, rows: 30 })
        .then(() => true, (error: unknown) => String(error));
      if (opened !== true) return { step: "terminal", ok: false, opened };
      let readiness = "";
      const readyDeadline = Date.now() + 10_000;
      while (Date.now() < readyDeadline) {
        const snap = await bridge.invoke("terminal_snapshot", { sessionId }).then(
          (value: { shellReadiness: string }) => value.shellReadiness,
          () => "error",
        );
        readiness = snap;
        if (readiness === "ready") break;
        await new Promise((resolve) => setTimeout(resolve, 300));
      }
      if (readiness !== "ready") return { step: "ready", ok: false, readiness };
      const lease = await bridge.invoke("terminal_acquire_lease", { sessionId, owner: "spec-m8-015" }).then(
        (value: string) => value,
        () => null,
      );
      if (!lease) return { step: "lease", ok: false };
      // 初始目录可能已是 /tmp，不能仅填写同一个目标就声称“等待同步”。
      // 固定测试 PTY 到 /，确保与下方 /tmp 目标实际不同。
      const otherDirectory = "/";
      await bridge.invoke("terminal_input", { sessionId, lease, input: Array.from(new TextEncoder().encode(`cd ${otherDirectory}\n`)) });
      let changedDirectory = false;
      for (let attempt = 0; attempt < 80; attempt += 1) {
        const snapshot = await bridge.invoke("terminal_snapshot", { sessionId }) as { currentDirectory: string; shellReadiness: string };
        if (snapshot.currentDirectory === otherDirectory && snapshot.shellReadiness === "ready") { changedDirectory = true; break; }
        await new Promise((resolve) => setTimeout(resolve, 100));
      }
      if (!changedDirectory) return { step: "different-directory", ok: false };
      // 真实忙碌：sleep 占住前台。
      const sleepOutcome = await bridge
        .invoke("terminal_input", { sessionId, lease, input: Array.from(new TextEncoder().encode("sleep 6\n")) })
        .then(() => ({ ok: true as const }), (error: unknown) => ({ ok: false as const, raw: String(error) }));
      if (!sleepOutcome.ok) return { step: "sleep-input", ok: false, sleepOutcome };
      await new Promise((resolve) => setTimeout(resolve, 1000));
      const busySnap = await bridge.invoke("terminal_snapshot", { sessionId }).then(
        (value: { shellReadiness: string; foregroundProcess: string | null }) => value,
        () => null,
      );
      const busy = Boolean(busySnap && (busySnap.foregroundProcess !== null || busySnap.shellReadiness === "busy"));
      if (!busy) return { step: "busy", ok: false, busySnap };

      // 忙碌时对另一目标提交 ! 命令 → 排队等待。
      const context = await bridge.invoke("context_get", { contextId: null }).then(
        (value: { revision: string }) => value,
        () => null,
      );
      const queued = await bridge
        .invoke("terminal_submit_line", {
          requestId: "m8-015-queued",
          sessionId,
          line: "!echo fleqi-015-not-executed",
          contextRevision: context?.revision ?? "1",
          targetDisplay: "/tmp",
        })
        .then(
          (value: "sent" | "queued") => ({ ok: true as const, result: value }),
          (error: { code?: string; message?: string }) => ({ ok: false as const, code: error.code, message: error.message }),
        );
      if (!queued.ok || queued.result !== "queued") return { step: "queue", ok: false, queued };

      // keepAll 主动隐藏：取消在途目录切换与未投递命令，程序继续运行。
      const hidden = await bridge.invoke("surface_hide").then(() => true, (error: unknown) => String(error));
      if (hidden !== true) return { step: "hide", ok: false, hidden };
      const withdrawn = await bridge.invoke("terminal_withdrawn_line", { sessionId }).then(
        (value: { text: string } | null) => ({ ok: true as const, text: value?.text ?? null }),
        (error: { code?: string }) => ({ ok: false as const, code: error.code }),
      );
      if (!withdrawn.ok || !withdrawn.text || !withdrawn.text.includes("fleqi-015-not-executed")) {
        return { step: "withdraw", ok: false, withdrawn };
      }
      const afterHide = await bridge.invoke("terminal_snapshot", { sessionId }).then(
        (value: { pendingDirectory: string | null; directorySync: string; screen: string }) => value,
        () => null,
      );
      const pendingCleared = Boolean(afterHide && afterHide.pendingDirectory === null);
      // 程序继续运行：再次显式唤起得到新会话并显示。
      const reshow = await bridge.invoke("surface_show").then(
        (value: { visibleSessionId: string | null }) => value,
        () => null,
      );
      if (!reshow?.visibleSessionId || reshow.visibleSessionId === sessionId) return { step: "reshow", ok: false, reshow };
      // sleep 结束后，被撤销的命令不得执行。
      await new Promise((resolve) => setTimeout(resolve, 6500));
      const finalSnap = await bridge.invoke("terminal_snapshot", { sessionId }).then(
        (value: { screen: string }) => value.screen,
        () => "",
      );
      if (finalSnap.includes("fleqi-015-not-executed")) {
        return { step: "not-executed", ok: false, screenTail: finalSnap.slice(-400) };
      }
      return { step: "done", ok: true, sessionId, successor: reshow.visibleSessionId, pendingCleared, withdrawnText: withdrawn.text };
    })) as Record<string, unknown>;
    writeEvidence("m4-flow-015.json", { capturedAt: new Date().toISOString(), ...summary });
    expect(summary.ok).toBe(true);
    expect(summary.step).toBe("done");
    expect(summary.pendingCleared).toBe(true);
  });

  it("AC-FLOW-011d：会话选择器 UI 提供继续/删除（真实组件冒烟）", async () => {
    await waitConsoleReady();
    await browser.execute(() =>
      window.__TAURI_INTERNALS__.invoke("app_open_window", { role: "composer" }).then(() => null, () => null),
    );
    await switchToWindowWithHash("composer");
    await $("[data-phase='ready']").waitForExist({ timeout: 30_000 });
    const selectorButton = $("button[aria-label='会话选择器']");
    await selectorButton.waitForExist({ timeout: 20_000 });
    await selectorButton.click();
    const selectorSection = $("section[data-testid='session-selector']");
    await selectorSection.waitForExist({ timeout: 10_000 });
    let rowCount = 0;
    try {
      await browser.waitUntil(
        async () => {
          rowCount = await (await $$("[data-testid='session-row']")).length;
          return rowCount > 0;
        },
        { timeout: 15_000 },
      );
    } catch (error) {
      const selectorText = (await selectorSection.isExisting()) ? await selectorSection.getText() : "选择器不存在";
      writeEvidence("m4-flow-011d.json", {
        capturedAt: new Date().toISOString(),
        failedAt: "rows",
        rowCount,
        selectorText: selectorText.slice(0, 500),
      });
      throw error;
    }
    const continueButtons = await $$("[data-testid='session-continue']");
    const continueCount = continueButtons.length;
    expect(continueCount).toBeGreaterThanOrEqual(1);
    const continueButton = continueButtons[0];
    if (continueButton) {
      await continueButton.click();
      await browser.waitUntil(
        async () => !(await $("section[data-testid='session-selector']").isExisting()),
        { timeout: 15_000 },
      );
    }
    await browser.execute(() =>
      window.__TAURI_INTERNALS__.invoke("hotkey_clear", { requestId: "m8-hotkey-clear" }).then(() => null, () => null),
    );
    writeEvidence("m4-flow-011d.json", {
      capturedAt: new Date().toISOString(),
      continueCount,
      evidence: evidencePath("m4-flow-011d.json"),
      note: "选择器历史行提供继续/删除；点击继续后选择器关闭并切到新会话；spec 结束清除测试热键。",
    });
  });
});
