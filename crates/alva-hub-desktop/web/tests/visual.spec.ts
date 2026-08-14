// INPUT:  Running Vite Hub, deterministic demo bridge, and Playwright
// OUTPUT: Seven aggregate hierarchy screenshots plus model-picker overlay coverage
// POS:    Visual contract connecting Cindy lineworks to the ALVA component system

import { expect, test, type Page } from "@playwright/test";

async function settle(page: Page) {
  await page.locator("body").waitFor({ state: "visible" });
  await page.evaluate(() => document.fonts.ready);
}

test("01 chat shell", async ({ page }) => {
  await page.goto("/?demo=1&session=ps-demo-1");
  await settle(page);
  await expect(page).toHaveScreenshot("01-chat-shell.png", { fullPage: true });
});

test("02 new conversation", async ({ page }) => {
  await page.goto("/?demo=1");
  await settle(page);
  await expect(page).toHaveScreenshot("02-new-conversation.png", { fullPage: true });
});

test("03 right workbench", async ({ page }) => {
  await page.goto("/?demo=1&session=ps-demo-1&workbench=1&tool=inspector");
  await settle(page);
  await expect(page).toHaveScreenshot("03-right-workbench.png", { fullPage: true });
});

test("03 right workbench add-tab menu", async ({ page }) => {
  await page.goto("/?demo=1&session=ps-demo-1&workbench=1&tool=inspector");
  await page.getByRole("button", { name: /Add workbench tab|添加工作台标签页/ }).click();
  await settle(page);
  await expect(page).toHaveScreenshot("03-right-workbench-add-tab.png", { fullPage: true });
});

test("03 right workbench manages dynamic tabs and width", async ({ page }) => {
  await page.goto("/?demo=1&session=ps-demo-1&workbench=1&tool=inspector");
  await page.getByRole("button", { name: /Add workbench tab|添加工作台标签页/ }).click();
  await page.getByRole("menuitem", { name: /Terminal|终端/ }).click();
  await expect(page.getByRole("tab", { name: /Terminal|终端/ })).toHaveAttribute("aria-selected", "true");

  const resize = page.getByRole("separator", { name: /Resize workbench|调整工作台宽度/ });
  await resize.press("ArrowLeft");
  await expect(resize).toHaveAttribute("aria-valuenow", "464");

  await page.getByRole("button", { name: /Close workbench · Terminal|关闭工作台 · 终端/ }).click();
  await expect(page.getByRole("tab", { name: /Inspector|检查器/ })).toHaveAttribute("aria-selected", "true");
  await expect(page.getByRole("tab", { name: /Terminal|终端/ })).toHaveCount(0);
});

test("03 right workbench becomes a narrow overlay", async ({ page }) => {
  await page.setViewportSize({ width: 860, height: 900 });
  await page.goto("/?demo=1&session=ps-demo-1&workbench=1&tool=inspector");
  await settle(page);
  await expect(page).toHaveScreenshot("03-right-workbench-narrow.png", { fullPage: true });
});

test("04 automation", async ({ page }) => {
  await page.goto("/?demo=1&page=automations");
  await page.getByRole("button", { name: /Nightly self-healing tests|夜间自修复测试/ }).click();
  await settle(page);
  await expect(page).toHaveScreenshot("04-automation.png", { fullPage: true });
});

test("05 plugins and skills", async ({ page }) => {
  await page.goto("/?demo=1&page=extensions");
  await settle(page);
  await expect(page).toHaveScreenshot("05-plugins-skills.png", { fullPage: true });
});

test("06 settings", async ({ page }) => {
  await page.goto("/?demo=1&page=settings");
  await settle(page);
  await expect(page).toHaveScreenshot("06-settings.png", { fullPage: true });
});

test("07 context menus", async ({ page }) => {
  await page.goto("/?demo=1");
  await page.getByRole("button", { name: /Refactor the handoff|重构交接/ }).click({ button: "right" });
  await settle(page);
  await expect(page).toHaveScreenshot("07-context-menus.png", { fullPage: true });
});

test("model picker stays anchored to composer", async ({ page }) => {
  await page.goto("/?demo=1&session=ps-demo-1");
  await page.getByRole("button", { name: /Model and engine|模型与引擎/ }).click();
  await settle(page);
  await expect(page).toHaveScreenshot("08-model-picker.png", { fullPage: true });
});
