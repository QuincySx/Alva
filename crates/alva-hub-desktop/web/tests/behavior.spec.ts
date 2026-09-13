// INPUT:  Running Vite Hub, deterministic demo bridge, and Playwright
// OUTPUT: R35 behavior matrix — four viewport tiers, keyboard-only flows,
//         overflow / focus-restore / state evidence (no pixel baselines)
// POS:    Behavior acceptance that screenshots alone cannot prove; visual
//         baselines stay in visual.spec.ts and real-product runs are recorded
//         separately in the R35 card.

import { expect, test, type Page } from "@playwright/test";

// The four accepted layout tiers: ≥1360 / 1024–1359 / 760–1023 / <760.
const TIERS = [
  { width: 1600, label: "wide" },
  { width: 1280, label: "desktop" },
  { width: 900, label: "narrow" },
  { width: 720, label: "compact" },
] as const;

async function settle(page: Page) {
  await page.locator("body").waitFor({ state: "visible" });
  await page.evaluate(() => document.fonts.ready);
}

async function expectNoHorizontalOverflow(page: Page) {
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  );
  expect(overflow, "unintended horizontal page overflow").toBeLessThanOrEqual(0);
}

for (const tier of TIERS) {
  test(`tier ${tier.label} (${tier.width}px): chat, workbench and gate stay inside the viewport`, async ({ page }) => {
    await page.setViewportSize({ width: tier.width, height: 900 });

    await page.goto("/?demo=1&session=ps-demo-1");
    await settle(page);
    await expectNoHorizontalOverflow(page);
    // Send stays reachable at every tier (label carries the session engine).
    await expect(page.getByRole("button", { name: /Run with|运行/ })).toBeVisible();

    await page.goto("/?demo=1&session=ps-demo-1&workbench=1&tool=inspector");
    await settle(page);
    await expectNoHorizontalOverflow(page);
    await expect(page.getByRole("button", { name: /Run with|运行/ })).toBeVisible();

    await page.goto("/?demo=1");
    await settle(page);
    await expectNoHorizontalOverflow(page);
  });
}

test("long English copy at the compact tier does not overflow and keeps send reachable", async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem("alva-hub.language", "en-US"));
  await page.setViewportSize({ width: 720, height: 900 });
  await page.goto("/?demo=1&session=ps-demo-1");
  await settle(page);
  // Main-column English copy (topbar title + composer placeholder) stays
  // readable while the sidebar is a closed sheet at this width.
  await expect(page.getByPlaceholder(/Ask claude to inspect|Ask alva to inspect/)).toBeVisible();
  await expectNoHorizontalOverflow(page);
  await expect(page.getByRole("button", { name: /Run with|运行/ })).toBeVisible();
});

test("sidebar states: no-match search, and the narrow sheet restores focus on Escape", async ({ page }) => {
  await page.setViewportSize({ width: 720, height: 900 });
  await page.goto("/?demo=1&session=ps-demo-1");
  await settle(page);

  // Narrow sheet: the topbar menu toggle opens the sidebar as a sheet…
  const toggle = page.locator(".hub-topbar__menu");
  await toggle.click();
  const search = page.getByLabel(/Search sessions|搜索会话/);
  await expect(search).toBeVisible();
  // …a no-match query shows the truthful empty state…
  await search.fill("zzz-no-match");
  await expect(page.getByText(/No sessions match that search|没有匹配的会话/)).toBeVisible();
  // …and Escape closes the sheet, returning focus to the toggle (R35).
  await page.keyboard.press("Escape");
  await expect(search).toBeHidden();
  await expect(toggle).toBeFocused();
});

test("workbench close returns focus to the topbar open-workbench control", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/?demo=1&session=ps-demo-1&workbench=1&tool=inspector");
  await settle(page);

  await page.getByRole("button", { name: /Close workbench|关闭工作台/ }).last().click();
  const openWorkbench = page.locator(".hub-topbar__workbench");
  await expect(openWorkbench).toBeVisible();
  await expect(openWorkbench).toBeFocused();
});

test("keyboard-only: independent session → model search → submit surfaces the honest failure", async ({ page }) => {
  await page.goto("/?demo=1");
  await settle(page);

  // Keyboard: create an independent session from the gate.
  await page.getByRole("button", { name: /Create independent session|创建独立会话/ }).focus();
  await page.keyboard.press("Enter");
  const composer = page.getByLabel(/Message|消息/);
  await expect(composer).toBeFocused();

  // Keyboard: open the model picker, unified-search across engines, select.
  await page.getByRole("button", { name: /Model and engine|模型与引擎/ }).focus();
  await page.keyboard.press("Enter");
  const search = page.getByLabel(/Search models|搜索模型/);
  await search.fill("gpt-5");
  await expect(page.getByText(/Searching every runtime|正在搜索全部运行时/)).toBeVisible();
  await page.keyboard.press("Tab");
  await page.keyboard.press("Enter");
  // Selection closes the picker and the composer status names the route.
  await expect(search).toBeHidden();
  await expect(page.getByText(/Codex CLI · GPT-5\.6-Sol/).nth(1)).toBeVisible();

  // Keyboard: submit; the demo bridge has no engine, so the honest failure
  // card is the expected terminal state (not a fake run).
  await composer.fill("keyboard smoke prompt");
  await page.keyboard.press("Enter");
  await expect(
    page.getByText(/The operation failed|操作失败。请重试/).first(),
  ).toBeVisible({ timeout: 10_000 });
});

test("keyboard-only: Inspector drills into a child Agent and Escape backs out", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/?demo=1&session=ps-demo-1&workbench=1&tool=inspector");
  await settle(page);

  const inspector = page.getByRole("region", { name: /Native Agent Inspector|原生 Agent 检查器/ });
  const childOpen = inspector
    .locator(".ui-list-row")
    .filter({ hasText: /Child Agent demo-nat|子 Agent demo-nat/ })
    .getByRole("button", { name: /Open|打开/ });
  await childOpen.focus();
  await page.keyboard.press("Enter");
  await expect(inspector).toContainText(/Child session|子会话/);
  // Drill-down hands focus to the inspector section (R35); wait for it
  // before pressing Escape so the section-scoped handler receives the key.
  await expect(inspector).toBeFocused();

  // Escape backs out one level and returns focus to the originating row.
  await page.keyboard.press("Escape");
  await expect(inspector).toContainText(/Child Agent demo-nat|子 Agent demo-nat/);
  await expect(childOpen).toBeFocused();
});

test.describe("normal motion", () => {
  test.use({ reducedMotion: "no-preference" });

  test("normal motion renders the chat shell inside the viewport", async ({ page }) => {
    // The Playwright config pins reducedMotion: "reduce" — the tier matrix
    // above is the reduced-motion evidence. This run proves normal motion
    // renders too.
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.goto("/?demo=1&session=ps-demo-1");
    await settle(page);
    await expectNoHorizontalOverflow(page);
    await expect(page.getByRole("button", { name: /Run with|运行/ })).toBeVisible();
  });
});

test("truthful disabled and not-wired states are visible in settings", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/?demo=1&page=settings");
  await settle(page);

  // R31/R32 disabled controls stay visibly disabled (General section).
  await expect(page.getByRole("combobox", { name: /Max Worker Concurrency|子 Worker 最大并发数/ })).toBeDisabled();
  await expect(page.getByRole("switch", { name: /Git Auto Checkpoint|自动 Git 安全保存点/ })).toBeDisabled();
  // The language selector is the one real control — it must stay enabled.
  await expect(page.getByRole("combobox", { name: /Language|语言/ })).toBeEnabled();
});
