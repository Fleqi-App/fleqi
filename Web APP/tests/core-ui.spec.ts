import { expect, test } from '@playwright/test';

test('workspace matches the supplied capability screen and keeps non-copied features disconnected', async ({ page }) => {
  const external: string[] = [];
  page.on('request', request => { if (!request.url().startsWith('http://127.0.0.1:1426')) external.push(request.url()); });
  await page.setViewportSize({ width: 1200, height: 840 });
  await page.goto('/');
  await expect(page.getByRole('heading', { name: '文件处理能力', exact: true })).toBeVisible();
  await expect(page.locator('.capability-card')).toHaveCount(6);
  await expect(page.getByRole('heading', { name: '概览', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: '任务记录', exact: true })).toBeDisabled();
  await expect(page.getByRole('button', { name: 'GitHub 账号', exact: true })).toBeDisabled();
  await expect(page.locator('.agent-bar,.overview-stack,.model-editor')).toHaveCount(0);
  const before = (await page.locator('.workspace-sidebar').boundingBox())!.width;
  await page.getByRole('button', { name: '切换侧栏' }).click();
  await expect.poll(async () => (await page.locator('.workspace-sidebar').boundingBox())!.width).toBe(64);
  await page.getByRole('button', { name: '切换侧栏' }).click();
  await expect.poll(async () => (await page.locator('.workspace-sidebar').boundingBox())!.width).toBe(before);
  await page.mouse.move(1195, 835);
  await page.screenshot({ path: 'docs/previews/core-workspace.png' });
  expect(external).toEqual([]);
});

test('the preview host links workspace to General and restores the trigger after closing', async ({ page }) => {
  await page.goto('/');
  const trigger = page.getByRole('button', { name: '设置', exact: true });
  await trigger.click();
  const dialog = page.getByRole('dialog', { name: 'Fleqi 设置', exact: true });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole('heading', { name: '通用', exact: true })).toBeVisible();
  await expect(dialog.getByRole('button', { name: '模型与账号', exact: true })).toBeDisabled();
  await dialog.getByRole('switch', { name: '启用底部快捷栏' }).click();
  await expect(dialog.getByRole('switch', { name: '启用底部快捷栏' })).not.toBeChecked();
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
  await expect(trigger).toBeFocused();
  await trigger.click();
  await expect(dialog.getByRole('switch', { name: '启用底部快捷栏' })).not.toBeChecked();
  await dialog.getByRole('button', { name: '关闭设置' }).click();
  await expect(dialog).toHaveCount(0);
});

test('General renders independently, preserves its controls and keeps all scrolling inside the panel', async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 800 });
  await page.goto('/?surface=settings');
  await expect(page.getByRole('switch', { name: '登录时启动' })).toBeChecked();
  await expect(page.getByRole('slider', { name: '气泡消失秒数' })).toHaveValue('4.8');
  await page.screenshot({ path: 'docs/previews/core-settings-general.png' });
  await page.getByRole('combobox', { name: '显示方式' }).click();
  await page.getByRole('option', { name: '常驻显示' }).click();
  await expect(page.getByRole('slider', { name: '气泡消失秒数' })).toHaveCount(0);
  await page.getByRole('combobox', { name: '显示方式' }).click();
  await page.getByRole('option', { name: '自动消失' }).click();
  await page.getByRole('slider', { name: '气泡消失秒数' }).focus();
  await page.keyboard.press('ArrowRight');
  await expect(page.getByRole('slider', { name: '气泡消失秒数' })).toHaveValue('4.9');
  await page.getByRole('button', { name: '录入底部栏快捷键' }).click();
  await page.keyboard.press('Control+Alt+KeyK');
  await page.getByRole('button', { name: '保存', exact: true }).click();
  await page.reload();
  await expect(page.getByRole('button', { name: '录入底部栏快捷键' })).toContainText('K');
  await expect(page.getByRole('slider', { name: '气泡消失秒数' })).toHaveValue('4.9');
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBe(1000);
  expect(await page.evaluate(() => document.documentElement.scrollHeight)).toBe(800);
});

test('settings search, narrow viewports and reduced motion remain usable', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.setViewportSize({ width: 420, height: 740 });
  await page.goto('/?surface=settings');
  await page.getByRole('textbox', { name: '搜索设置与功能' }).fill('快捷键');
  await expect(page.getByRole('navigation', { name: '设置分类' }).getByRole('button')).toHaveCount(1);
  await expect(page.getByRole('button', { name: '通用', exact: true })).toBeVisible();
  await page.getByRole('button', { name: '清除搜索' }).click();
  await expect(page.getByRole('navigation', { name: '设置分类' }).getByRole('button')).toHaveCount(7);
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBe(420);
  await page.getByRole('button', { name: '关闭设置' }).click();
  await expect(page.getByRole('heading', { name: '文件处理能力' })).toBeVisible();
});
