import { browser, expect } from "@wdio/globals";
import { writeEvidence } from "../lib/evidence";

// M3.2 · 模型端点与规划闭环的真实宿主 IPC。密钥路径不在浏览器侧出现：
// 保存不带密钥（apiKey 为 null），掩码语义由 Rust 端 ProviderService 测试覆盖；
// 这里验证命令可达与错误闭环：无端点→引导文案，端点不可达→网络失败（可重试）。

type Outcome = { failed: boolean; code?: string; message?: string; retryable?: boolean; value?: unknown };

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

describe("M3.2 · 模型端点与规划闭环（真实 IPC）", () => {
  it("未配置端点时规划被拒并引导设置；登记端点后断连如实报错（FR-AI-002/003）", async () => {
    await switchToConsole();

    const none = (await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      const context = await bridge.invoke("context_refresh") as { id: string };
      const session = await bridge.invoke("session_create", { requestId: "plan-session-none" }) as { id: string };
      return bridge.invoke("run_plan_submit", { requestId: "m3-plan-none", sessionId: session.id, contextId: context.id, prompt: "列出文件" })
        .then(
          (value: unknown) => ({ failed: false, value }),
          (error: { code?: string; message?: string }) => ({ failed: true, code: error.code, message: error.message }),
        );
    }    )) as Outcome;
    expect(none.failed).toBe(true);
    expect(none.code).toBe("unavailable");
    expect(none.message ?? "").toContain("模型端点");

    const saved = (await browser.execute(() =>
      window.__TAURI_INTERNALS__
        .invoke("provider_save", {
          request: {
            id: "loop",
            displayName: "回环端点",
            baseUrl: "http://127.0.0.1:9/v1",
            models: ["m"],
            defaultGenerationModel: "m",
            summaryModel: null,
            timeoutMs: 2000,
            apiKey: null,
          },
        })
        .then((value: unknown) => ({ failed: false, value }), () => ({ failed: true })),
    )) as Outcome;
    expect(saved.failed).toBe(false);

    const listed = (await browser.execute(() =>
      window.__TAURI_INTERNALS__
        .invoke("provider_list")
        .then((value: unknown[]) => ({ ok: true, value }), () => ({ ok: false, value: [] as unknown[] })),
    )) as { ok: boolean; value: { record: { id: string }; credentialConfigured: boolean }[] };
    expect(listed.ok).toBe(true);
    const loop = listed.value.find((entry) => entry.record.id === "loop");
    expect(loop).toBeDefined();
    if (loop) {
      expect(loop.credentialConfigured).toBe(false);
      expect(JSON.stringify(loop)).not.toContain("apiKey");
    }

    const unreachable = (await browser.execute(async () => {
      const bridge = window.__TAURI_INTERNALS__;
      const context = await bridge.invoke("context_refresh") as { id: string };
      const session = await bridge.invoke("session_create", { requestId: "plan-session-dead" }) as { id: string };
      return bridge.invoke("run_plan_submit", { requestId: "m3-plan-dead", sessionId: session.id, contextId: context.id, prompt: "列出文件" })
        .then(
          (value: unknown) => ({ failed: false, value }),
          (error: { code?: string; message?: string; retryable?: boolean }) => ({ failed: true, code: error.code, message: error.message, retryable: error.retryable }),
        );
    }    )) as Outcome;
    expect(unreachable.failed).toBe(true);
    expect(unreachable.code).toBe("unavailable");
    // 不可用端点如实报错：回环端口被拒（网络失败）或本机代理应答非 2xx
    // （模型响应无效）都是真实失败路径，不得伪称成功。
    expect(unreachable.message ?? "").toMatch(/网络失败|模型响应无效|模型端点/);
    expect(unreachable.retryable).toBe(true);

    writeEvidence("m3-planning-evidence.json", {
      capturedAt: new Date().toISOString(),
      noProvider: none,
      providerView: loop ?? null,
      unreachable,
      note: "provider_save/list 端点登记与掩码视图（不含密钥字段）；run_plan_submit 无端点→引导文案，端点不可达→网络失败（可重试），均经真实 IPC。",
    });
  });
});
