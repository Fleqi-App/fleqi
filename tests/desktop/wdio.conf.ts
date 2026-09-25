import path from "node:path";
import { artifactsDir, ensureArtifactsDir, runningTestBinaryPids, testBinary, writeEvidence } from "./lib/evidence";

/**
 * P0-DESKTOP-001 / M1.6 · pnpm test:desktop
 * 驱动 `tauri build --debug --no-bundle --features desktop-test` 产出的测试构建：
 * 内嵌 WebDriver（tauri-plugin-wdio-webdriver）→ 真实 WKWebView → 真实 IPC。
 * 普通包不含该驱动；测试不 mock 任何命令。宿主进程由 tauri-service 在 onPrepare 启动、
 * onComplete 停止；退出核对在 scripts/test-desktop.mjs 完成（同一次运行内的多个 spec
 * 共享同一进程；重启恢复由脚本分两次运行验证）。
 */
ensureArtifactsDir();

/** 证据文件名固定；scripts/test-desktop.mjs 在每次运行后归档为 lifecycle-run-a/b.json。 */
const LAUNCHER_FILE = "lifecycle-launcher.json";
const SESSION_FILE = "lifecycle-session.json";
const SESSION_END_FILE = "lifecycle-session-end.json";

export const config: WebdriverIO.Config = {
  runner: "local",
  specs: ["./specs/**/*.spec.ts"],
  maxInstances: 1,
  logLevel: "info",
  outputDir: path.join(artifactsDir, "wdio-logs"),
  services: [
    [
      "@wdio/tauri-service",
      {
        appBinaryPath: testBinary,
        driverProvider: "embedded",
        embeddedPort: 4445,
        windowLabel: "console",
        startTimeout: 90_000,
      },
    ],
  ],
  capabilities: [
    {
      browserName: "tauri",
      "tauri:options": { application: testBinary },
    } as WebdriverIO.Capabilities,
  ],
  framework: "mocha",
  reporters: ["spec"],
  mochaOpts: { ui: "bdd", timeout: 120_000 },
  waitforTimeout: 20_000,
  connectionRetryTimeout: 120_000,
  connectionRetryCount: 1,

  onPrepare() {
    writeEvidence(LAUNCHER_FILE, {
      testBinary,
      startedAt: new Date().toISOString(),
      pidsBeforeLaunch: runningTestBinaryPids(),
    });
  },
  async before() {
    // 服务在成功的 switchToWindow 后停止"自动聚焦恢复"（该恢复依赖 withGlobalTauri，
    // 本工程不开启，否则每条命令都会等待 5 秒超时）。
    await browser.switchToWindow(await browser.getWindowHandle());
    writeEvidence(SESSION_FILE, {
      sessionReadyAt: new Date().toISOString(),
      pidsDuringSession: runningTestBinaryPids(),
    });
  },
  after() {
    writeEvidence(SESSION_END_FILE, {
      sessionEndedAt: new Date().toISOString(),
      pidsAtSessionEnd: runningTestBinaryPids(),
    });
  },
};
