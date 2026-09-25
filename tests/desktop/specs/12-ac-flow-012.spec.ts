import { browser, expect, $ } from "@wdio/globals";
import { writeEvidence } from "../lib/evidence";

// AC-FLOW-012 · 能力目录发现 → 缺依赖 → 安装（进度）→ 执行 → 收藏 → 新会话复用；
// AI 摘要段在无端点时按合同完成"条件不足引导"验收（capabilities.md §1.2）。
// 受管工具来源由 runner 准备：数据目录 tools/managed-catalog.json + 127.0.0.1 回环包。
// 由 run-a2-012 独占串行运行。会话 ID 经 body data 属性传递（零参闭包）。

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

describe("AC-FLOW-012 · 发现→缺依赖→安装→执行→收藏→复用（真实回环安装）", () => {
  it("受管工具经回环包安装后真实执行；收藏并在新会话复用；无端点时 AI 摘要给引导", async () => {
    await switchToWindowWithHash("#/console");
    await $("[data-phase='ready']").waitForExist({ timeout: 30_000 });
    await browser.waitUntil(
      async () => (await $("[data-testid='host-state']").getAttribute("data-host-state")) === "ready",
      { timeout: 20_000 },
    );

    // 1) 目录发现 + 会话 A（执行上下文）。
    const discovery = (await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      const runtimeContext = await bridge.invoke("context_refresh") as { id: string };
      const catalog = await bridge.invoke("catalog_query").then(
        (value: unknown[]) => value.length,
        () => -1,
      );
      // 共享数据目录上本阶段可能最先运行：先真实注册快捷键，显式显示才被允许。
      await bridge
        .invoke("hotkey_commit", { requestId: "m12-hotkey", accelerator: "CommandOrControl+Shift+J" })
        .then(() => null, () => null);
      await bridge.invoke("context_refresh").then(() => null, () => null);
      const shown = await bridge.invoke("surface_show").then(
        (value: { visibleSessionId: string | null }) => value,
        (error: { code?: string; message?: string }) => ({ code: error.code ?? "", message: error.message ?? "" }),
      );
      const session = shown && "visibleSessionId" in shown ? shown.visibleSessionId ?? "" : "";
      document.body.dataset.fleqi12Session = session;
      return { catalog, session };
    })) as { catalog: number; session: string };
    const sessionA = discovery.session;
    writeEvidence("m12-discovery.json", { capturedAt: new Date().toISOString(), ...discovery });
    expect(discovery.catalog).toBeGreaterThanOrEqual(30);
    expect(sessionA).toBeTruthy();
    // Explicit show now correctly focuses the composer. Pin subsequent tool work
    // to the console instead of the driver's transient focused-window fallback.
    await switchToWindowWithHash("#/console");
    await browser.execute(() => window.__TAURI_INTERNALS__.invoke("app_open_window", { role: "console" }));

    // 2) 缺依赖：受管清单在列表中且未安装。
    const missing = (await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      const runtimeContext = await bridge.invoke("context_refresh") as { id: string };
      const list = await bridge.invoke("tools_list").then(
        (value: Array<{ manifest: { id: string }; status: { kind: string } }>) => value,
        () => [],
      );
      const entry = list.find((item) => item.manifest.id === "fleqi-demo-tool");
      return { present: Boolean(entry), status: entry?.status.kind ?? "absent" };
    })) as { present: boolean; status: string };
    writeEvidence("m12-missing.json", { capturedAt: new Date().toISOString(), ...missing });
    expect(missing.present).toBe(true);
    expect(missing.status).toBe("notInstalled");

    // 3) 安装（回环下载 + 校验 + 预检 + 发布），轮询进度。
    const installed = (await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      const runtimeContext = await bridge.invoke("context_refresh") as { id: string };
      const requestId = "m12-install";
      const stages: string[] = [];
      const done = await bridge.invoke("tools_install", { requestId, toolId: "fleqi-demo-tool" }).then(
        (value: { status: { kind: string; version?: string }; installed: { installDir: string } | null }) => value,
        (error: { code?: string; message?: string }) => ({ code: error.code ?? "", message: error.message ?? "" }),
      );
      const probe = window.setInterval(() => {
        void bridge.invoke("tools_install_status", { requestId }).then((value: { stage?: string } | null) => {
          if (value && value.stage && stages[stages.length - 1] !== value.stage) stages.push(value.stage);
        }, () => {});
      }, 200);
      await new Promise((resolve) => setTimeout(resolve, 2500));
      window.clearInterval(probe);
      return { done, stages };
    })) as { done: { status?: { kind?: string; version?: string }; installed?: { installDir: string } | null; code?: string; message?: string }; stages: string[] };
    writeEvidence("m12-installed.json", { capturedAt: new Date().toISOString(), ...installed });
    if (!installed.done || !installed.done.status) {
      throw new Error(`安装失败：${installed.done?.message ?? "?"}`);
    }
    expect(installed.done.status.kind).toBe("available");
    expect(installed.done.installed?.installDir).toBeTruthy();
    // 进度阶段采样取决于安装时长（回环小包常在首个轮询前完成）：
    // InstallProgress 的确定/不确定进度由 Rust 集成测试与工具页轮询测试覆盖，
    // 此处仅记录原生采样结果，不作硬断言。

    // 4) 执行：Run 调用已安装的真实可执行文件。
    const execPlan = (await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      const runtimeContext = await bridge.invoke("context_refresh") as { id: string };
      const sid = document.body.dataset.fleqi12Session ?? "";
      const list = await bridge.invoke("tools_list").then(
        (value: Array<{ manifest: { id: string; executable: string }; installed: { installDir: string } | null }>) => value,
        () => [],
      );
      const entry = list.find((item) => item.manifest.id === "fleqi-demo-tool");
      const installDir = entry?.installed?.installDir ?? "";
      if (!installDir) return { error: "没有安装目录" };
      const command = `'${installDir}/${entry?.manifest.executable}'`;
      document.body.dataset.fleqi12Command = command;
      const run = await bridge
        .invoke("run_submit", {
          requestId: "m12-run",
          sessionId: sid,
          prompt: "打印受管工具版本",
          plan: {
            revision: "7",
            contextId: runtimeContext.id,
            scripts: [command],
            effects: ["read"],
            previewComplete: true,
          },
        })
        .then(
          (value: { id: string }) => value,
          (error: { code?: string; message?: string }) => ({ code: error.code ?? "", message: error.message ?? "" }),
        );
      if (!("id" in (run as object))) return { error: (run as { message?: string }).message ?? "run_submit 失败" };
      const runId = (run as { id: string }).id;
      const pending = await bridge.invoke("run_get", { runId }) as { state: string; planRevision: string };
      if (pending.state === "awaitingApproval") {
        await bridge.invoke("run_approve", { requestId: `m12-approve-${runId}`, runId, planRevision: pending.planRevision });
      }
      const deadline = Date.now() + 25_000;
      let record: { state: string; output: string; exitStatus: number | null } | null = null;
      while (Date.now() < deadline) {
        record = await bridge.invoke("run_get", { runId }).then(
          (value: { state: string; output: string; exitStatus: number | null }) => value,
          () => null,
        );
        if (record && ["succeeded", "failed", "cancelled"].includes(record.state)) break;
        await new Promise((resolve) => setTimeout(resolve, 150));
      }
      return { runId, record };
    })) as { runId?: string; record?: { state: string; output: string; exitStatus: number | null } | null; error?: string };
    writeEvidence("m12-executed.json", { capturedAt: new Date().toISOString(), ...execPlan });
    if (execPlan.error) throw new Error(`执行失败：${execPlan.error}`);
    expect(execPlan.record?.state).toBe("succeeded");
    expect(execPlan.record?.output).toContain("fleqi-demo-tool 1.0");

    // 5) 收藏：命令进入收藏并可读回。
    const favorite = (await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      const runtimeContext = await bridge.invoke("context_refresh") as { id: string };
      const command = document.body.dataset.fleqi12Command ?? "";
      const created = await bridge
        .invoke("favorites_create", { name: "受管工具版本", content: command, kind: "manual" })
        .then(
          (value: { id: string }) => value,
          (error: { code?: string; message?: string }) => ({ code: error.code ?? "", message: error.message ?? "" }),
        );
      const list = await bridge.invoke("favorites_list").then(
        (value: Array<{ name: string; content: string }>) => value,
        () => [],
      );
      return {
        createdId: "id" in (created as object) ? (created as { id: string }).id : "",
        listed: list.some((item) => item.name === "受管工具版本" && item.content === command),
      };
    })) as { createdId: string; listed: boolean };
    writeEvidence("m12-favorite.json", { capturedAt: new Date().toISOString(), ...favorite });
    expect(favorite.createdId).toBeTruthy();
    expect(favorite.listed).toBe(true);

    // 6) 新上下文复用：新建会话后同一条命令再次真实执行。
    const reuse = (await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      const runtimeContext = await bridge.invoke("context_refresh") as { id: string };
      const created = await bridge.invoke("session_create", { requestId: "m12-create" }).then(
        (value: { id: string }) => value,
        (error: { code?: string; message?: string }) => ({ code: error.code ?? "", message: error.message ?? "" }),
      );
      if (!("id" in (created as object))) return { error: (created as { message?: string }).message ?? "创建会话失败" };
      const sessionB = (created as { id: string }).id;
      await bridge
        .invoke("session_select", { requestId: "m12-select", sessionId: sessionB })
        .then(() => null, () => null);
      const command = document.body.dataset.fleqi12Command ?? "";
      const run = await bridge
        .invoke("run_submit", {
          requestId: "m12-reuse-run",
          sessionId: sessionB,
          prompt: "复用收藏命令打印受管工具版本",
          plan: {
            revision: "8",
            contextId: runtimeContext.id,
            scripts: [command],
            effects: ["read"],
            previewComplete: true,
          },
        })
        .then(
          (value: { id: string }) => value,
          (error: { code?: string; message?: string }) => ({ code: error.code ?? "", message: error.message ?? "" }),
        );
      if (!("id" in (run as object))) return { error: (run as { message?: string }).message ?? "run_submit 失败" };
      const runId = (run as { id: string }).id;
      const pending = await bridge.invoke("run_get", { runId }) as { state: string; planRevision: string };
      if (pending.state === "awaitingApproval") {
        await bridge.invoke("run_approve", { requestId: `m12-approve-${runId}`, runId, planRevision: pending.planRevision });
      }
      const deadline = Date.now() + 25_000;
      let record: { state: string; output: string } | null = null;
      while (Date.now() < deadline) {
        record = await bridge.invoke("run_get", { runId }).then(
          (value: { state: string; output: string }) => value,
          () => null,
        );
        if (record && ["succeeded", "failed", "cancelled"].includes(record.state)) break;
        await new Promise((resolve) => setTimeout(resolve, 150));
      }
      return { sessionB, record };
    })) as { sessionB?: string; record?: { state: string; output: string } | null; error?: string };
    writeEvidence("m12-reuse.json", { capturedAt: new Date().toISOString(), ...reuse });
    if (reuse.error) throw new Error(`复用失败：${reuse.error}`);
    expect(reuse.sessionB).toBeTruthy();
    expect(reuse.sessionB).not.toBe(sessionA);
    expect(reuse.record?.state).toBe("succeeded");
    expect(reuse.record?.output).toContain("fleqi-demo-tool 1.0");

    // 7) AI 摘要段：无模型端点时按合同给引导（条件不足的验收路径）。
    const guidance = (await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      const runtimeContext = await bridge.invoke("context_refresh") as { id: string };
      const sid = document.body.dataset.fleqi12Session ?? "";
      return await bridge
        .invoke("run_plan_submit", { requestId: "m12-plan", sessionId: sid, contextId: runtimeContext.id, prompt: "总结安装结果" })
        .then(
          () => ({ guided: false }),
          (error: { code?: string; message?: string }) => ({ guided: true, code: error.code ?? "", message: error.message ?? "" }),
        );
    })) as { guided: boolean; code?: string; message?: string };
    writeEvidence("m12-ai-guidance.json", { capturedAt: new Date().toISOString(), ...guidance });
    expect(guidance.guided).toBe(true);
    expect(guidance.code).toBe("unavailable");
    // 条件不足的两类真实反馈：无端点 → 配置引导；已配置但不可达/响应无效 → 真实错误
    //（测试环境无可用凭据，不得伪造摘要；capabilities.md §1.2 条件语义）。
    const honest =
      guidance.message?.includes("模型端点") ||
      guidance.message?.includes("模型响应无效") ||
      guidance.message?.includes("网络失败") ||
      guidance.message?.includes("认证失败") ||
      guidance.message?.includes("限流");
    expect(honest).toBe(true);
  });
});
