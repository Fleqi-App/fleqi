import { expect, test } from "@playwright/test";

// 浏览器预览：数据来自 preview 适配器，页面持续标识"浏览器预览"；不计原生证据。
const maskTime = (page: import("@playwright/test").Page) => [page.locator("[data-slot='card'] dd:has-text('版本')").nth(5)];

test.describe("M1 控制台", () => {
  test("概览：宿主就绪、存储正常、平台能力、预览标识", async ({ page }) => {
    await page.goto("/#/console/overview");
    await expect(page.locator("[data-phase='ready']")).toBeVisible();
    await expect(page.getByTestId("host-state")).toHaveAttribute("data-host-state", "ready");
    await expect(page.getByTestId("storage-state")).toHaveAttribute("data-storage-state", "ready");
    await expect(page.getByTestId("host-kind")).toHaveAttribute("data-host-kind", "preview");
    await expect(page.getByTestId("platform-capabilities").locator("li")).toHaveCount(4);
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
    await expect(page).toHaveScreenshot("console-overview-dark.png");
  });

  test("权限与自检：重新检查 → 显式申请 → 上下文刷新与取消选择", async ({ page }) => {
    await page.goto("/#/console/permissions");
    await expect(page.getByRole("heading", { name: "权限与自检" })).toBeVisible();
    await page.getByRole("button", { name: "重新检查全部" }).click();
    await expect(page.getByTestId("status-finderAutomation")).toContainText("未授权");
    await page.locator("[data-permission='finderAutomation']").getByRole("button", { name: "显式申请" }).click();
    await expect(page.getByTestId("status-finderAutomation")).toContainText("已授权");
    await page.getByRole("button", { name: "刷新 Finder 上下文" }).click();
    await expect(page.getByTestId("context-snapshot")).toHaveAttribute("data-availability", "available");
    await page.getByRole("button", { name: "选择文件夹…" }).click();
    await expect(page.getByTestId("context-notice")).toContainText("已取消");
    await expect(page.getByTestId("context-snapshot")).toHaveAttribute("data-context-id", "ctx-1");
    await expect(page).toHaveScreenshot("console-permissions.png", { mask: [page.locator("[data-testid='context-snapshot'] dd").last(), page.locator("text=检查时间")] });
  });

  test("存储降级：宿主 degraded，可进入诊断", async ({ page }) => {
    await page.goto("/?preview=degraded#/console/overview");
    await expect(page.getByTestId("host-state")).toHaveAttribute("data-host-state", "degraded");
    await page.getByRole("button", { name: "关于与更新" }).click();
    await expect(page.locator("[data-field='diag-storage']")).toContainText("降级");
    await expect(page).toHaveScreenshot("console-degraded-about.png");
  });

  test("失败态：宿主桥不可用显示错误并可重试", async ({ page }) => {
    await page.goto("/?preview=fail#/console/overview");
    await expect(page.getByRole("alert")).toContainText("宿主桥不可用");
    await page.getByRole("button", { name: "重试" }).click();
    await expect(page.getByRole("alert")).toContainText("forbidden");
  });
});

test.describe("M1 设置", () => {
  test("外观：主题改为浅色后真实保存并生效，版本递增；键盘可达", async ({ page }) => {
    await page.goto("/#/settings/appearance");
    await expect(page.getByRole("heading", { name: "外观" })).toBeVisible();
    await page.getByRole("radiogroup", { name: "主题" }).getByRole("radio", { name: "浅色" }).click();
    await expect(page.getByTestId("save-theme")).toHaveAttribute("data-save-state", "saved");
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
    await expect(page.locator("[data-settings-revision=\"1\"]")).toBeVisible();
    await page.getByLabel("透明材质").focus();
    await page.keyboard.press("Space");
    await expect(page.getByTestId("save-transparency")).toHaveAttribute("data-save-state", "saved");
    await expect(page.locator("html")).toHaveAttribute("data-transparency", "off");
    await expect(page.locator("[data-settings-revision=\"2\"]")).toBeVisible();
    await expect(page).toHaveScreenshot("settings-appearance-light.png");
  });

  test("通用页真实保存 + 任务与诊断 aiPolicy 真实保存", async ({ page }) => {
    await page.goto("/#/settings/general");
    await expect(page.getByRole("heading", { name: "通用" })).toBeVisible();
    // 唤起方式真实切换（M2）
    await page.getByRole("radio", { name: "随 Finder" }).click();
    await expect(page.locator("[data-settings-revision=\"1\"]")).toBeVisible();
    await page.getByRole("radio", { name: "结束全部" }).click();
    await expect(page.locator("[data-settings-revision=\"2\"]")).toBeVisible();
    await expect(page).toHaveScreenshot("settings-general.png", { mask: [page.getByText(/设置版本/)] });
    // M3：aiPolicy 真实保存 + 上限项以说明呈现
    await page.getByRole("button", { name: "任务与诊断" }).click();
    await expect(page.getByRole("radio", { name: "全部免确认" })).toBeVisible();
    await expect(page.getByText(/只读自动/)).toBeVisible();
    await page.getByRole("radio", { name: "全部免确认" }).click();
    await expect(page.getByText(/已保存/)).toBeVisible();
    await expect(page.getByText("10000 行（固定）")).toBeVisible();
  });
});

void maskTime;

test("640px 控制台：侧栏收起、固定看板无横向或纵向滚动", async ({ page }) => {
  await page.setViewportSize({ width: 640, height: 640 });
  await page.goto("/#/console/overview");
  await expect(page.getByTestId("capability-category")).toHaveCount(6);
  const sidebar = await page.locator("[data-slot='sidebar']").boundingBox();
  expect(sidebar!.width).toBe(80);
  await expect(page.getByRole("button", { name: "能力库", exact: true })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  for (const height of [520, 640]) {
    await page.setViewportSize({ width: 640, height });
    const layout = await page.locator("[data-fixed-board]").evaluate((main) => ({ scroll: main.scrollHeight, height: main.clientHeight, bottom: main.getBoundingClientRect().bottom, footerBottom: main.querySelector("footer")!.getBoundingClientRect().bottom }));
    expect(layout.scroll).toBeLessThanOrEqual(layout.height);
    expect(layout.footerBottom).toBeLessThanOrEqual(layout.bottom);
    await page.locator("[data-testid='overview-board']").hover();
    await page.mouse.wheel(0, 700);
    expect(await page.locator("[data-fixed-board]").evaluate((main) => main.scrollTop)).toBe(0);
  }
  await expect(page).toHaveScreenshot("console-overview-compact.png");
});

test("转换设置实际保存，重新进入仍显示原文件处理策略", async ({ page }) => {
  await page.goto("/#/settings/files");
  await page.getByRole("radio", { name: "原文件移入回收站", exact: true }).click();
  await expect(page.getByText("已保存", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "外观", exact: true }).click();
  await page.getByRole("button", { name: "文件与工具", exact: true }).click();
  await expect(page.getByRole("radio", { name: "原文件移入回收站", exact: true })).toHaveAttribute("aria-checked", "true");
});

test("同名文件处理可切换且重新进入后保留选择", async ({ page }) => {
  await page.goto("/#/settings/files");
  const group = page.getByRole("radiogroup", { name: "遇到同名文件时" });
  await group.getByRole("radio", { name: "替换已有文件", exact: true }).click();
  await expect(page.getByText("已保存", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "外观", exact: true }).click();
  await page.getByRole("button", { name: "文件与工具", exact: true }).click();
  await expect(group.getByRole("radio", { name: "替换已有文件", exact: true })).toHaveAttribute("aria-checked", "true");
  await group.getByRole("radio", { name: "保留两份", exact: true }).click();
  await expect(group.getByRole("radio", { name: "保留两份", exact: true })).toHaveAttribute("aria-checked", "true");
  await expect(page.getByText(/unknown variant|forbidden/)).toHaveCount(0);
});
