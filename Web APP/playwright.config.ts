import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests',
  fullyParallel: true,
  // 每个用例都在测几何与动画帧，并发过高会让点击等待在共享开发服务器上排队超时。
  workers: 2,
  timeout: 45_000,
  use: {
    baseURL: 'http://127.0.0.1:1426',
    viewport: { width: 1448, height: 1086 },
    browserName: 'chromium',
    channel: 'chrome',
    trace: 'retain-on-failure',
  },
  webServer: {
    command: 'pnpm dev',
    url: 'http://127.0.0.1:1426',
    reuseExistingServer: !process.env.CI,
  },
});
