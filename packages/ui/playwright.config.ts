import { existsSync } from "node:fs";
import { homedir } from "node:os";
import path from "node:path";
import { defineConfig, devices } from "@playwright/test";

/**
 * 新 UI 的浏览器行为/截图测试（P0-UI-001 / M1.5）。
 * 只验证页面逻辑与视觉基线，不代替 WKWebView/原生验收；基线更新需显式
 * `pnpm test:ui:update`。没有 Playwright 内置浏览器缓存时回退到本机 Google Chrome
 * （channel），不自动下载；可用 PLAYWRIGHT_CHANNEL 显式指定。
 */
const bundledChromium = existsSync(path.join(homedir(), "Library", "Caches", "ms-playwright"));
const channel = process.env.PLAYWRIGHT_CHANNEL ?? (bundledChromium ? undefined : "chrome");

export default defineConfig({
  testDir: "./e2e",
  snapshotPathTemplate: "{testDir}/__screenshots__/{testFilePath}/{arg}{ext}",
  fullyParallel: true,
  retries: 0,
  reporter: [["list"]],
  use: {
    ...devices["Desktop Chrome"],
    ...(channel ? { channel } : {}),
    baseURL: "http://127.0.0.1:1421",
    viewport: { width: 1080, height: 760 },
    deviceScaleFactor: 1,
    reducedMotion: "reduce",
    locale: "zh-CN",
    timezoneId: "Asia/Shanghai",
  },
  expect: {
    toHaveScreenshot: { animations: "disabled", caret: "hide", maxDiffPixels: 120 },
  },
  webServer: {
    command: "pnpm run build && pnpm exec vite preview --host 127.0.0.1 --port 1421 --strictPort",
    url: "http://127.0.0.1:1421",
    reuseExistingServer: false,
    timeout: 120_000,
  },
});
