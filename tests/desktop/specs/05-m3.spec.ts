import fs from "node:fs";
import { $, browser, expect } from "@wdio/globals";
import { writeEvidence } from "../lib/evidence";

// M3 · Run 编排、能力目录与输入历史的真实宿主 IPC（与 UI 同一条 Tauri 桥，不 mock）。
// 编排内部（策略决策、并发、重试、输出上限）已由 Rust 测试覆盖；这里验证宿主注册的
// 命令真实可达且行为符合合同：readOnly 下只读自动执行、变更/未知效果等待确认、
// 过期计划版本确认被拒、取消保留记录。每个用例在页面内一个闭包里完成整条链路，
// 会话/Run 标识只存在于闭包局部变量，请求参数全部是字面量；断言在 Node 侧执行。

type Summary = Record<string, unknown>;

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

describe("M3 · Run 编排与目录（真实 IPC）", () => {
  it("只读计划在 readOnlyAutoConfirmChanges 下自动执行并落盘输出（FR-POLICY-002）", async () => {
    await switchToConsole();
    await $("[data-phase='ready']").waitForExist({ timeout: 30_000 });
    await browser.waitUntil(async () => (await $("[data-testid='host-state']").getAttribute("data-host-state")) === "ready", { timeout: 20_000 });

    const summary = (await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      const runtimeContext = await bridge.invoke("context_refresh") as { id: string };
      const created = await bridge
        .invoke("session_create", { requestId: "m3-run-session" })
        .then((value: { id: string }) => value, () => null);
      if (!created) return { step: "session_create", ok: false };
      const run = await bridge
        .invoke("run_submit", {
          requestId: "m3-read-submit",
          sessionId: created.id,
          prompt: "打印只读探针标记",
          plan: {
            revision: "41",
            contextId: runtimeContext.id,
            scripts: ["printf m3-read-ok"],
            effects: ["read"],
            previewComplete: true,
          },
        })
        .then((value: unknown) => value, () => null);
      if (!run) return { step: "run_submit", ok: false };
      const runId = (run as { id: string }).id;
      const deadline = Date.now() + 25_000;
      let record: unknown = run;
      while (Date.now() < deadline) {
        record = await bridge
          .invoke("run_get", { runId })
          .then((value: unknown) => value, () => run);
        const state = (record as { state: string }).state;
        if (state === "succeeded" || state === "failed" || state === "cancelled") break;
        await new Promise((resolve) => setTimeout(resolve, 100));
      }
      return { step: "done", ok: true, sessionId: created.id, record };
    })) as Summary;

    expect(summary.ok).toBe(true);
    expect(summary.step).toBe("done");
    const record = summary.record as { state: string; exitStatus: number | null; output: string };
    expect(record.state).toBe("succeeded");
    expect(record.exitStatus).toBe(0);
    expect(record.output).toContain("m3-read-ok");
  });

  /** 与 07 的设置 UI 写入解耦：提交前显式钉住本用例依赖的策略（共享数据目录下并行安全）。 */
  async function pinReadOnlyPolicy(): Promise<void> {
    await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      const runtimeContext = await bridge.invoke("context_refresh") as { id: string };
      const attempt = async (requestId: string, expected: string | null) =>
        bridge
          .invoke("settings_update", {
            request: { requestId, expectedRevision: expected, patch: { aiPolicy: "readOnlyAutoConfirmChanges" } },
          })
          .then(
            () => ({ ok: true as const }),
            (error: { currentRevision?: string }) => ({ ok: false as const, retryWith: error.currentRevision ?? null }),
          );
      const revision = await bridge
        .invoke("app_bootstrap")
        .then((value: { settings: { revision: string } }) => value.settings.revision, () => null);
      if (revision === null) return;
      const first = await attempt("m5-pin-policy", revision);
      if (first.ok || first.retryWith === null) return;
      await attempt("m5-pin-policy-retry", first.retryWith);
    });
  }

  it("变更效果等待确认；过期计划版本被拒，正确版本确认后执行（FR-POLICY-003）", async () => {
    await pinReadOnlyPolicy();
    const summary = (await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      const runtimeContext = await bridge.invoke("context_refresh") as { id: string };
      const sessions = await bridge
        .invoke("session_list", {})
        .then((value: { active: { id: string }[] }) => value, () => null);
      const sessionId = sessions && sessions.active.length > 0 ? sessions.active[0].id : "";
      if (!sessionId) return { step: "session_list", ok: false };
      const run = await bridge
        .invoke("run_submit", {
          requestId: "m3-change-submit",
          sessionId,
          prompt: "打印变更探针标记",
          plan: {
            revision: "42",
            contextId: runtimeContext.id,
            scripts: ["printf m3-change-ok"],
            effects: ["create"],
            previewComplete: true,
          },
        })
        .then((value: unknown) => value, () => null);
      if (!run) return { step: "run_submit", ok: false };
      const runId = (run as { id: string }).id;
      const revision = (run as { planRevision: string }).planRevision;
      const stale = await bridge
        .invoke("run_approve", { requestId: "m3-approve-stale", runId, planRevision: "99" })
        .then(
          () => ({ rejected: false }),
          (error: { code?: string }) => ({ rejected: true, code: error.code }),
        );
      const approved = await bridge
        .invoke("run_approve", { requestId: "m3-approve", runId, planRevision: revision })
        .then((value: unknown) => value, () => null);
      if (!approved) return { step: "run_approve", ok: false };
      const deadline = Date.now() + 25_000;
      let record: unknown = approved;
      while (Date.now() < deadline) {
        record = await bridge
          .invoke("run_get", { runId })
          .then((value: unknown) => value, () => approved);
        const state = (record as { state: string }).state;
        if (state === "succeeded" || state === "failed" || state === "cancelled") break;
        await new Promise((resolve) => setTimeout(resolve, 100));
      }
      return { step: "done", ok: true, stale, record };
    })) as Summary;

    expect(summary.ok).toBe(true);
    expect(summary.step).toBe("done");
    const stale = summary.stale as { rejected: boolean; code?: string };
    expect(stale.rejected).toBe(true);
    expect(stale.code).toBe("conflict");
    const record = summary.record as { state: string; exitStatus: number | null; output: string };
    expect(record.state).toBe("succeeded");
    expect(record.exitStatus).toBe(0);
    expect(record.output).toContain("m3-change-ok");
  });

  it("未知效果不自动执行；取消保留记录（FR-POLICY-004、FR-RUN-003）", async () => {
    await pinReadOnlyPolicy();
    const summary = (await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      const runtimeContext = await bridge.invoke("context_refresh") as { id: string };
      const sessions = await bridge
        .invoke("session_list", {})
        .then((value: { active: { id: string }[] }) => value, () => null);
      const sessionId = sessions && sessions.active.length > 0 ? sessions.active[0].id : "";
      if (!sessionId) return { step: "session_list", ok: false };
      const run = await bridge
        .invoke("run_submit", {
          requestId: "m3-unknown-submit",
          sessionId,
          prompt: "未分类效果探针",
          plan: {
            revision: "43",
            contextId: runtimeContext.id,
            scripts: ["printf m3-never"],
            effects: ["unknown"],
            previewComplete: true,
          },
        })
        .then((value: unknown) => value, () => null);
      if (!run) return { step: "run_submit", ok: false };
      const submitted = run as { id: string; state: string };
      const cancelled = await bridge
        .invoke("run_cancel", { runId: submitted.id })
        .then((value: unknown) => value, () => null);
      if (!cancelled) return { step: "run_cancel", ok: false };
      const listed = await bridge
        .invoke("run_list", { sessionId })
        .then((value: unknown[]) => value, () => null);
      return { step: "done", ok: true, submitted, cancelled, count: listed ? listed.length : 0 };
    })) as Summary;

    expect(summary.ok).toBe(true);
    expect(summary.step).toBe("done");
    const submitted = summary.submitted as { state: string };
    expect(submitted.state).toBe("awaitingApproval");
    const cancelled = summary.cancelled as { state: string };
    expect(cancelled.state).toBe("cancelled");
    expect(summary.count as number).toBeGreaterThanOrEqual(1);
  });

  it("能力目录覆盖全部合同 ID 且表单可读；输入历史经宿主写入可读回（FR-CAP-CATALOG、FR-HISTORY-001）", async () => {
    const summary = (await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      const runtimeContext = await bridge.invoke("context_refresh") as { id: string };
      const catalog = await bridge
        .invoke("catalog_query")
        .then((value: { id: string; category: string }[]) => value, () => null);
      if (!catalog) return { step: "catalog_query", ok: false };
      const forms = [];
      for (const entry of catalog) {
        const form = await bridge.invoke("capability_form", { capabilityId: entry.id, contextId: runtimeContext.id }) as { capabilityId: string };
        forms.push(form.capabilityId);
      }
      const appended = await bridge
        .invoke("history_append", { entry: "printf m3-history-line" })
        .then(() => true, () => false);
      const history = await bridge
        .invoke("history_list")
        .then((value: string[]) => value, () => null);
      return { step: "done", ok: true, catalog, forms, appended, history };
    })) as Summary;

    expect(summary.ok).toBe(true);
    expect(summary.step).toBe("done");
    const catalog = summary.catalog as { id: string; category: string }[];
    const contract = fs.readFileSync(new URL("../../../docs/capabilities.md", import.meta.url), "utf8");
    const expected = [...new Set(contract.match(/CAP-(?:FILE|ZIP|IMAGE|MEDIA|PDF|TEXT|SYSTEM|NETWORK|DEV|TOOLS|CALC)-\d{3}/g))].sort();
    expect(catalog.map((entry) => entry.id).sort()).toEqual(expected);
    expect((summary.forms as string[]).sort()).toEqual(expected);
    expect(summary.appended).toBe(true);
    expect(summary.history).toContain("printf m3-history-line");

    writeEvidence("m3-runs-evidence.json", {
      capturedAt: new Date().toISOString(),
      catalogCategories: [...new Set(catalog.map((entry) => entry.category))].sort(),
      historySample: summary.history,
      note: "run_submit/approve/cancel/get/list、session_list、catalog_query、history_append/list 全部经真实 IPC；只读自动执行、变更需确认、过期版本 conflict、取消保留记录。",
    });
  });

  it("工具目录经真实 IPC：探测系统工具、复用已有工具、拒绝卸载系统工具（FR-TOOLS-001/003）", async () => {
    const summary = (await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      const runtimeContext = await bridge.invoke("context_refresh") as { id: string };
      const tools = await bridge
        .invoke("tools_list")
        .then(
          (value: {
            manifest: { id: string };
            status: { kind: string; version?: string; owner?: string };
          }[]) => value,
          () => null,
        );
      if (!tools) return { step: "tools_list", ok: false };
      const installReused = await bridge
        .invoke("tools_install", { requestId: "m3-tools-system", toolId: "git" })
        .then(
          (entry: { status: { kind: string; owner?: string; version?: string } }) => ({ rejected: false, status: entry.status }),
          (error: { code?: string }) => ({ rejected: true, code: error.code }),
        );
      const removeRefused = await bridge
        .invoke("tools_remove", { toolId: "git" })
        .then(
          () => ({ rejected: false }),
          (error: { code?: string }) => ({ rejected: true, code: error.code }),
        );
      return { step: "done", ok: true, tools, installReused, removeRefused };
    })) as Summary;

    expect(summary.ok).toBe(true);
    expect(summary.step).toBe("done");
    const tools = summary.tools as {
      manifest: { id: string };
      status: { kind: string; version?: string; owner?: string };
    }[];
    expect(tools.length).toBeGreaterThanOrEqual(4);
    const git = tools.find((entry) => entry.manifest.id === "git");
    expect(git).toBeDefined();
    if (git) {
      expect(git.status.kind).toBe("available");
      expect(git.status.owner).toBe("system");
      expect(git.status.version ?? "").toContain("git version");
    }
    const installReused = summary.installReused as { rejected: boolean; status: { kind: string; owner?: string; version?: string } };
    expect(installReused.rejected).toBe(false);
    expect(installReused.status).toMatchObject({ kind: "available", owner: "system", version: git?.status.version });
    const removeRefused = summary.removeRefused as { rejected: boolean; code?: string };
    expect(removeRefused.rejected).toBe(true);
    expect(removeRefused.code).toBe("not_found");

    writeEvidence("m3-tools-evidence.json", {
      capturedAt: new Date().toISOString(),
      tools: tools.map((entry) => ({
        id: entry.manifest.id,
        kind: entry.status.kind,
        version: entry.status.version ?? null,
      })),
      note: "tools_list 真实探测；已有 Git 直接复用，版本不变；系统工具卸载被拒（not_found，无受管记录）。",
    });
  });
});
