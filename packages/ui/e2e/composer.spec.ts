import { expect, test } from "@playwright/test";

// 浏览器预览：数据来自 preview 适配器；不计原生证据。

test.describe("M2 输入条（预览）", () => {
  test.use({ viewport: { width: 760, height: 72 } });
  test("无热键：显示被拒并提示注册快捷键", async ({ page }) => {
    await page.goto("/#/composer");
    await expect(page.getByTestId("composer")).toHaveAttribute("data-surface", "userHidden");
    await expect(page.getByTestId("composer-surface-notice")).toContainText("快捷键");
  });

  test("注册热键后显示；`!` 输入全绿、终端标识与提交反馈（AC-FLOW-009）", async ({ page }) => {
    await page.goto("/#/composer");
    // 预览适配器未注册热键 → composer 停在隐藏态；先通过 URL 上的重复实例无法共享状态，
    // 因此本用例在页面内直接验证 UI 模式解析与提交路径（surface 显示由单元测试覆盖）。
    await expect(page.getByTestId("composer")).toBeVisible();
    await page.getByTestId("composer-input").fill("!echo fleqi-m2-pty-ok");
    await expect(page.getByTestId("composer")).toHaveAttribute("data-mode", "terminal");
    await expect(page.getByTestId("composer-mode")).toHaveText("终端");
    await expect(page.getByTestId("composer-input")).toHaveCSS("color", "rgb(116, 217, 159)");
    await page.getByRole("button", { name: "提交" }).click();
    // 无可见会话：提示创建/选择，不伪造发送。
    await expect(page.getByTestId("composer-notice")).toContainText("会话");
    await expect(page).toHaveScreenshot("composer-terminal-mode.png", { mask: [page.getByTestId("composer-notice")] });
  });

  test("AI 输入在无会话时拒绝伪造提交（模型引导路径由单元测试覆盖）", async ({ page }) => {
    await page.goto("/#/composer");
    await page.getByTestId("composer-input").fill("总结这个文件夹");
    await expect(page.getByTestId("composer")).toHaveAttribute("data-mode", "ai");
    await page.keyboard.press("Enter");
    // 预览未注册热键 → 无可见会话：先提示会话，不伪造提交/结果。
    await expect(page.getByTestId("composer-notice")).toContainText("会话");
    await expect(page.getByTestId("result-bubble")).toBeHidden();
  });

  test("展开会话面板位于窗口内；Esc 关闭并归还输入焦点", async ({ page }) => {
    await page.setViewportSize({ width: 560, height: 432 });
    await page.goto("/#/composer");
    await page.getByRole("button", { name: "会话选择器", exact: true }).click();
    await expect(page.getByLabel("搜索会话")).toBeFocused();
    const panel = await page.getByTestId("session-selector").boundingBox();
    expect(panel!.y).toBeGreaterThanOrEqual(0);
    expect(panel!.y + panel!.height).toBeLessThanOrEqual(360);
    await page.getByTestId("session-create").click();
    await expect(page.getByTestId("session-selector")).toBeHidden();
    await page.getByRole("button", { name: "会话选择器", exact: true }).click();
    await expect(page.getByTestId("session-row")).toBeVisible();
    await expect(page).toHaveScreenshot("composer-session-panel.png");
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("session-selector")).toBeHidden();
    await expect(page.getByTestId("composer-input")).toBeFocused();
  });
});

for (const theme of ["浅色", "深色"]) {
  test(`${theme}界面：终端正文高对比、边缘留白，快速收起再打开不丢面板`, async ({ page }, testInfo) => {
    await page.emulateMedia({ reducedMotion: "no-preference" });
    await page.goto("/#/settings/appearance");
    await page.getByRole("radiogroup", { name: "主题" }).getByRole("radio", { name: theme, exact: true }).click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", theme === "浅色" ? "light" : "dark");
    await page.evaluate(() => { window.location.hash = "#/composer"; });
    await page.setViewportSize({ width: 920, height: 432 });
    await page.getByRole("button", { name: "会话选择器", exact: true }).click();
    await page.getByTestId("session-create").click();
    await expect(page.getByTestId("session-selector")).toBeHidden();
    await page.getByRole("button", { name: "打开终端面板", exact: true }).click();
    const panel = page.getByTestId("terminal-panel");
    await expect(panel).toHaveAttribute("data-status", "running");
    await expect(panel.locator(".xterm-rows")).toContainText("zsh");
    const view = await panel.evaluate((element) => {
      const terminal = element.querySelector(".xterm")!;
      const text = element.querySelector(".xterm-rows span")!;
      const output = element.querySelector("[data-testid='terminal-output']")!;
      const rect = element.getBoundingClientRect();
      const inner = output.getBoundingClientRect();
      const luminance = (color: string) => {
        const rgb = color.match(/[\d.]+/g)!.slice(0, 3).map(Number).map((v) => { const n = v / 255; return n <= .04045 ? n / 12.92 : ((n + .055) / 1.055) ** 2.4; });
        return rgb[0]! * .2126 + rgb[1]! * .7152 + rgb[2]! * .0722;
      };
      const fg = luminance(getComputedStyle(text).color);
      const bg = luminance(getComputedStyle(terminal).backgroundColor);
      return { contrast: (Math.max(fg, bg) + .05) / (Math.min(fg, bg) + .05), left: inner.left - rect.left, right: rect.right - inner.right, bottom: rect.bottom - inner.bottom, clip: getComputedStyle(element).overflow };
    });
    expect(view.contrast).toBeGreaterThanOrEqual(7);
    expect(Math.min(view.left, view.right, view.bottom)).toBeGreaterThanOrEqual(12);
    expect(view.clip).toBe("hidden");
    await page.screenshot({ path: testInfo.outputPath("terminal.png"), animations: "disabled" });
    // Reopen during the 140ms exit, rather than after the old close timer has expired.
    await page.evaluate(() => {
      document.querySelector<HTMLButtonElement>("button[aria-label='收起终端']")!.click();
      requestAnimationFrame(() => document.querySelector<HTMLButtonElement>("button[aria-label='打开终端面板']")!.click());
    });
    await expect(panel).toHaveCSS("opacity", "1");
    await expect(panel).toHaveAttribute("data-status", "running");
    await page.getByRole("button", { name: "收起终端", exact: true }).click();
    await expect(panel).toHaveCount(0);
    await expect(page.getByTestId("composer-input")).toBeFocused();
  });
}
