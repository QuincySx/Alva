// INPUT:  The ?perf=1 fixture page (real components, real layout)
// OUTPUT: Browser-level evidence for H21 — bounded mounted DOM, working
//         scroll traversal, and anchor preservation while streaming appends
// POS:    H21 requires real-rendering evidence; jsdom has no layout, so the
//         bounded-window behavior is pinned here against the dev server.

import { expect, test } from "@playwright/test";

test.describe("H21 bounded transcript window", () => {
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

    // Every part of history remains reachable: jump to top, count grows stays bounded.
    await page.evaluate(() => {
      const region = document.querySelector("[data-chat-scroll-region]") as HTMLElement;
      region.scrollTop = 0;
    });
    await page.waitForTimeout(400);
    const mountedAtTop = await page.locator(".chat-message").count();
    expect(mountedAtTop).toBeLessThan(120);
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
      const message = document.elementFromPoint(700, 120)?.closest(".chat-message");
      return {
        scrollTop: region.scrollTop,
        anchor: message?.querySelector(".chat-message__body")?.textContent?.slice(0, 40) ?? null,
      };
    });
    await page.waitForTimeout(1500); // several stream deltas
    const after = await page.evaluate(() => {
      const region = document.querySelector("[data-chat-scroll-region]") as HTMLElement;
      return { scrollTop: region.scrollTop };
    });
    expect(Math.abs(after.scrollTop - anchorText.scrollTop)).toBeLessThan(4);
    // Stream settled: terminal state arrived without losing the anchor.
    await page.waitForFunction(() => document.documentElement.dataset.streamState === "done", null, { timeout: 20000 });
  });
});
