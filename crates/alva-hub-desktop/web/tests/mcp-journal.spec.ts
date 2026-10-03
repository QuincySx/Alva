// INPUT: Real Chrome/Vite UI Lab, locale/viewport and intercepted read-only journal queries
// OUTPUT: Keyboard, bounded replacement, literal search and inert source browser evidence
// POS: Browser UX acceptance without user MCP, paid providers or native WebView claims.
import { expect, test } from "@playwright/test";
for (const language of ["en-US", "zh-CN"]) for (const width of [1440, 720]) {
  test(`MCP journal: ${language} ${width}px read-only bounded navigation`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.addInitScript((locale) => localStorage.setItem("alva-hub.language", locale), language);
    const inputs: { action: string; offset?: number; query?: string }[] = [];
    await page.route("**/api/**", async (route) => {
      expect(new URL(route.request().url()).pathname).toBe("/api/sessions/journal-fixture/mcp-journal");
      const request = route.request().postDataJSON();
      expect(request.native_session_id).toBe("journal-root"); inputs.push(request.query);
      const offset = request.query.offset ?? 0;
      const text = offset === 0 ? "first-日志 <script>not executable</script>" : "last-日志";
      const result = request.query.action === "read"
        ? { record_id: "fixture-record", text, offset, next_offset: offset + Buffer.byteLength(text), total_bytes: 600000, has_more: offset === 0 }
        : { hits: [{ record_id: "fixture-record", seq: 1, timestamp: 1, server_id: "fixture", direction: "incoming", source_bytes: 600000 }], next_cursor: 1, through: 1, has_more: false };
      await route.fulfill({ contentType: "application/json", body: JSON.stringify(result) });
    });
    await page.goto("/?ui-lab=1"); expect(inputs).toHaveLength(0);
    await page.getByRole("tab", { name: "MCP journal", exact: true }).click();
    const open = page.getByRole("button", { name: language === "zh-CN" ? "打开" : "Open", exact: true });
    await expect(open).toBeVisible();
    // Vite's development StrictMode can repeat metadata effects, never body reads.
    expect(inputs.length).toBeGreaterThanOrEqual(1);
    expect(inputs.every((input) => input.action === "list")).toBe(true);
    await open.focus(); await page.keyboard.press("Enter");
    await expect(page.locator("pre")).toContainText("first-日志");
    await expect(page.locator("script:not([type='module'])")).toHaveCount(0);
    await page.getByRole("button", { name: language === "zh-CN" ? "下一页" : "Next page", exact: true }).click();
    await expect(page.locator("pre")).toContainText("last-日志"); await expect(page.locator("pre")).not.toContainText("first-日志");
    await page.getByRole("button", { name: language === "zh-CN" ? "返回索引" : "Back to index", exact: true }).click();
    await expect(open).toBeVisible();
    await page.getByRole("textbox").fill("日志%_*🙂");
    await page.getByRole("button", { name: language === "zh-CN" ? "搜索日志" : "Search journal", exact: true }).click();
    await expect.poll(() => inputs.at(-1)?.query).toBe("日志%_*🙂");
    expect(inputs.every((input) => ["list", "read", "search_history"].includes(input.action))).toBe(true);
    expect(await page.locator("main").evaluate((element) => element.scrollWidth - element.clientWidth)).toBeLessThanOrEqual(0);
  });
}
