// INPUT:  The ?perf=1 fixture page (real components, real layout)
// OUTPUT: Browser regressions for bounded DOM, scroll/resize/stream anchors and unmounted search navigation
// POS:    H21 real Chrome component-layout evidence, not packaged WKWebView acceptance.
// jsdom has no layout, so the
//         bounded-window behavior is pinned here against the dev server.

import { expect, test } from "@playwright/test";

test.describe("H21 bounded transcript window", () => {
  for (const count of [100, 1000, 10000]) {
    test(`opens ${count} entries at the measured bottom and follows resize`, async ({ page }) => {
      await page.goto(`/?perf=1&n=${count}&seed=20260920`);
      const gap = () => page.locator("[data-chat-scroll-region]").evaluate((region) =>
        region.scrollHeight - region.scrollTop - region.clientHeight);
      await expect.poll(gap).toBeLessThan(3);
      await page.waitForTimeout(500);
      await expect.poll(gap).toBeLessThan(3);
      await expect(page.locator(".chat-jump")).toHaveCount(0);
      await page.setViewportSize({ width: 900, height: 600 });
      await expect.poll(gap).toBeLessThan(3);
      await page.locator("[data-chat-scroll-region]").evaluate((region) => { region.scrollTop -= 1500; });
      await expect(page.locator(".chat-jump")).toBeVisible();
      await page.locator(".chat-jump").click();
      await expect.poll(gap).toBeLessThan(3);
      await expect(page.locator(".chat-jump")).toHaveCount(0);
    });
  }
  test("mounts a bounded slice of a 1000-entry session and still reaches every part by scrolling", async ({ page }) => {
    await page.goto("/?perf=1&n=1000&seed=20260920");
    const list = page.getByTestId("chat-transcript-list");
    await expect(list).toBeVisible();
    await page.waitForFunction(() => {
      const region = document.querySelector("[data-chat-scroll-region]");
      return region instanceof HTMLElement && region.scrollHeight > region.clientHeight;
    });

    const mounted = await page.locator(".chat-message").count();
    expect(mounted).toBeLessThan(120); // bounded — not all ~250 messages
    // Count ALL rows: message-only checks miss an unbounded tool timeline.
    expect(await list.locator(":scope > li").count()).toBeLessThan(120);
    expect(await page.locator(".timeline-card").count()).toBeLessThan(120);

    // Every part of history remains reachable: jump to top, count grows stays bounded.
    await page.evaluate(() => {
      const region = document.querySelector("[data-chat-scroll-region]") as HTMLElement;
      region.scrollTop = 0;
    });
    await page.waitForTimeout(400);
    const mountedAtTop = await page.locator(".chat-message").count();
    expect(mountedAtTop).toBeLessThan(120);
    expect(await list.locator(":scope > li").count()).toBeLessThan(120);
    const firstMessageVisible = await page.locator(".chat-message").first().isVisible();
    expect(firstMessageVisible).toBe(true);
  });

  test("keeps the reading anchor stable when a streaming append lands above the fold", async ({ page }) => {
    await page.goto("/?perf=1&n=1000&seed=20260920&stream=1&streamMs=15000");
    await page.waitForSelector("[data-chat-scroll-region] .chat-message:last-child");
    // Scroll up ~2000px to read history; the stream keeps appending below.
    await page.evaluate(() => {
      const region = document.querySelector("[data-chat-scroll-region]") as HTMLElement;
      region.scrollTop -= 2000;
    });
    await page.waitForTimeout(300);
    const anchorText = await page.evaluate(() => {
      const region = document.querySelector("[data-chat-scroll-region]") as HTMLElement;
      const message = [...document.querySelectorAll<HTMLElement>("[data-entry-key]")]
        .find((row) => row.getBoundingClientRect().bottom > region.getBoundingClientRect().top);
      return {
        key: message?.dataset.entryKey,
        offset: message ? message.getBoundingClientRect().top - region.getBoundingClientRect().top : null,
      };
    });
    expect(anchorText.key).toBeTruthy();
    await page.waitForTimeout(1500); // several stream deltas
    const after = await page.evaluate((key) => {
      const region = document.querySelector("[data-chat-scroll-region]") as HTMLElement;
      const row = [...document.querySelectorAll<HTMLElement>("[data-entry-key]")].find((row) => row.dataset.entryKey === key);
      return row ? row.getBoundingClientRect().top - region.getBoundingClientRect().top : null;
    }, anchorText.key);
    expect(after).not.toBeNull();
    expect(Math.abs(after! - anchorText.offset!)).toBeLessThan(4);
    // Stream settled: terminal state arrived without losing the anchor.
    await page.waitForFunction(() => document.documentElement.dataset.streamState === "done", null, { timeout: 20000 });
  });

  test("finds an unmounted history message without filtering the log or resuming stream follow", async ({ page }) => {
    await page.goto("/?perf=1&n=1000&seed=20260920&stream=1&streamMs=8000");
    const request = page.getByText("Fixture request 0: summarize the streaming merge behavior and list what changes for tool output.", { exact: true });
    await expect.poll(() => page.locator("[data-chat-scroll-region]").evaluate((region) =>
      region.scrollHeight - region.scrollTop - region.clientHeight)).toBeLessThan(3);
    await expect(request).toHaveCount(0);
    await page.getByRole("textbox", { name: /Find in conversation|在对话中查找/ }).fill("Fixture request 0:");
    await page.getByRole("button", { name: /^(Show match|定位匹配)$/ }).click();
    await expect(request).toBeInViewport();
    await page.waitForTimeout(600);
    await expect(request).toBeInViewport();
    await expect(page.locator(".chat-jump")).toBeVisible();
    expect(await page.getByTestId("chat-transcript-list").locator(":scope > li").count()).toBeLessThan(120);
    await page.locator(".chat-jump").click();
    await expect.poll(() => page.locator("[data-chat-scroll-region]").evaluate((region) =>
      region.scrollHeight - region.scrollTop - region.clientHeight)).toBeLessThan(3);
  });
});
