// INPUT:  production build (dist/), Playwright Chromium, CDP tracing
// OUTPUT: web/perf/results/perf-results-<ts>.json — H19 baseline measurements
// POS:    Deterministic measurement runner for the central reading area.
//         Frame evidence comes from CDP trace DrawFrame events (real renderer
//         frames); rAF sampling is recorded separately and labeled as
//         main-thread cadence, never as presentation evidence.
//
// Usage:  node perf/run-perf.mjs [--sizes 1000,10000] [--out results/]
//         The script rebuilds (npm run build) unless SKIP_BUILD=1.

import { spawn } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "@playwright/test";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WEB_ROOT = path.resolve(__dirname, "..");
const PORT = 1491;
const BASE = `http://127.0.0.1:${PORT}`;
const SEED = 20260920;

const argSizes = (() => {
  const flag = process.argv.indexOf("--sizes");
  if (flag < 0) return [1000, 10000];
  return process.argv[flag + 1].split(",").map((value) => Number.parseInt(value, 10));
})();

const percentile = (values, p) => {
  if (values.length === 0) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const rank = Math.min(sorted.length - 1, Math.max(0, Math.round((sorted.length - 1) * p)));
  return Math.round(sorted[rank] * 100) / 100;
};
const mean = (values) =>
  values.length === 0 ? null : Math.round((values.reduce((a, b) => a + b, 0) / values.length) * 100) / 100;

function summarizeFrameIntervals(intervals) {
  return {
    frames: intervals.length,
    p50_ms: percentile(intervals, 0.5),
    p95_ms: percentile(intervals, 0.95),
    p99_ms: percentile(intervals, 0.99),
    mean_ms: mean(intervals),
  };
}

/** Continuous CDP scroll gesture. Demand-driven wheel bursts make DrawFrame
 *  intervals include idle time between inputs; synthesizeScrollGesture feeds
 *  input every frame so intervals reflect real frame cadence. Returns after
 *  `durationMs` of continuous scrolling (direction pattern decides up/down). */
async function scrollGestureFor(cdp, page, durationMs, { distance = 2500, speed = 2500, alternate = false } = {}) {
  const deadline = Date.now() + durationMs;
  let up = true;
  while (Date.now() < deadline) {
    await cdp.send("Input.synthesizeScrollGesture", {
      x: 720,
      y: 450,
      xDistance: 0,
      yDistance: up ? -distance : distance,
      speed,
      gestureSourceType: "mouse",
    });
    if (alternate) up = !up;
    // Reset to the bottom when a history sweep hits the top so the gesture
    // keeps producing frames for the full window.
    const atTop = await page.evaluate(() => {
      const region = document.querySelector("[data-chat-scroll-region]");
      return region instanceof HTMLElement && region.scrollTop <= 1;
    });
    if (atTop) {
      await page.evaluate(() => {
        const region = document.querySelector("[data-chat-scroll-region]");
        if (region instanceof HTMLElement) region.scrollTop = region.scrollHeight;
      });
    }
  }
}

function summarizeLongTasks(tasks) {
  const over50 = tasks.filter((task) => task >= 50);
  return { count: tasks.length, over_50ms: over50.length, max_ms: percentile(tasks, 1.0) };
}

// --- CDP trace: collect DrawFrame (renderer presentation frames) ------------

async function readTraceStream(cdp, stream) {
  let raw = "";
  let eof = false;
  while (!eof) {
    const chunk = await cdp.send("IO.read", { handle: stream });
    raw += chunk.data;
    eof = chunk.eof;
  }
  await cdp.send("IO.close", { handle: stream }).catch(() => undefined);
  return raw;
}

async function withTrace(page, fn) {
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("Tracing.start", {
    transferMode: "ReturnAsStream",
    traceConfig: {
      includedCategories: [
        "devtools.timeline",
        "disabled-by-default-devtools.timeline.frame",
      ],
    },
  });
  const result = await fn(cdp);
  const complete = new Promise((resolve) => cdp.once("Tracing.tracingComplete", resolve));
  await cdp.send("Tracing.end");
  const payload = await complete;
  const raw = payload.stream ? await readTraceStream(cdp, payload.stream) : "";
  // The stream is one JSON document: {"traceEvents":[…]}.
  const frames = [];
  try {
    const doc = JSON.parse(raw);
    for (const event of doc.traceEvents ?? []) {
      if (event.name === "DrawFrame" && typeof event.ts === "number") {
        frames.push(event.ts / 1000);
      }
    }
  } catch {
    // A torn stream — frame evidence will be reported as unmeasured.
  }
  await cdp.detach().catch(() => undefined);
  const intervals = [];
  for (let i = 1; i < frames.length; i += 1) {
    const delta = frames[i] - frames[i - 1];
    if (delta > 0 && delta < 1000) intervals.push(delta);
  }
  // Discontinuities (gesture resets, page hitches with no input) are not
  // frame costs; count them separately instead of polluting percentiles.
  const active = intervals.filter((delta) => delta <= 250);
  return {
    drawFrame: summarizeFrameIntervals(active),
    discontinuities: intervals.length - active.length,
    rawFrameCount: frames.length,
  };
}

async function main() {
  if (!process.env.SKIP_BUILD) {
    console.log("[perf] building production bundle…");
    await new Promise((resolve, reject) => {
      const child = spawn("npm", ["run", "build"], { cwd: WEB_ROOT, stdio: "inherit", shell: true });
      child.on("exit", (code) => (code === 0 ? resolve() : reject(new Error(`build exit ${code}`))));
    });
  }

  const server = spawn(
    "npx",
    ["vite", "preview", "--port", String(PORT), "--strictPort", "--host", "127.0.0.1"],
    { cwd: WEB_ROOT, stdio: "ignore", shell: true },
  );
  await new Promise((resolve, reject) => {
    const deadline = Date.now() + 30000;
    const tick = async () => {
      try {
        await fetch(BASE);
        resolve();
      } catch {
        if (Date.now() > deadline) reject(new Error("preview server did not start"));
        else setTimeout(tick, 250);
      }
    };
    tick();
  });

  const browser = await chromium.launch({ headless: false, args: ["--window-size=1440,900"] });
  const browserVersion = browser.version();
  const results = { recorded_at: new Date().toISOString(), machine: {
    platform: `${os.platform()} ${os.release()} ${os.arch()}`,
    cpu: os.cpus()[0]?.model ?? "unknown",
    memory_gb: Math.round(os.totalmem() / (1024 ** 3)),
    browser: browserVersion,
    viewport: "1440x900@1x",
    build_mode: "vite production (esbuild minify)",
    seed: SEED,
    headed: true,
  }, sizes: {} };

  try {
    for (const size of argSizes) {
      console.log(`[perf] dataset n=${size}`);
      const sizeResult = { scenarios: {} };

      // --- S1: first open -------------------------------------------------
      sizeResult.scenarios.S1_first_open = [];
      for (let run = 0; run < 3; run += 1) {
        const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 });
        const page = await context.newPage();
        await page.addInitScript(() => {
          window.__longtasks = [];
          new PerformanceObserver((list) => {
            for (const entry of list.getEntries()) window.__longtasks.push(entry.duration);
          }).observe({ entryTypes: ["longtask"] });
        });
        const started = Date.now();
        await page.goto(`${BASE}/?perf=1&n=${size}&seed=${SEED}`, { waitUntil: "domcontentloaded" });
        await page.waitForSelector("[data-chat-scroll-region] .chat-message:last-child", { timeout: 60000 });
        await page.waitForFunction(() => {
          const region = document.querySelector("[data-chat-scroll-region]");
          if (!(region instanceof HTMLElement)) return false;
          const before = region.scrollHeight;
          return new Promise((resolve) => requestAnimationFrame(() =>
            requestAnimationFrame(() => resolve(region.scrollHeight === before))));
        }, { timeout: 60000 });
        const tti = Date.now() - started;
        const stats = await page.evaluate(() => ({
          dom: window.__perf.domNodes(),
          scroller: window.__perf.scrollerStats(),
          heap: window.__perf.heap(),
        }));
        sizeResult.scenarios.S1_first_open.push({ run, tti_ms: tti, ...stats });
        await context.close();
      }

      // --- S2: continuous scroll up (30s) after warmup ---------------------
      sizeResult.scenarios.S2_scroll_up_30s = [];
      for (let run = 0; run < 3; run += 1) {
        const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 });
        const page = await context.newPage();
        await page.addInitScript(() => {
          window.__longtasks = [];
          new PerformanceObserver((list) => {
            for (const entry of list.getEntries()) window.__longtasks.push(entry.duration);
          }).observe({ entryTypes: ["longtask"] });
        });
        await page.goto(`${BASE}/?perf=1&n=${size}&seed=${SEED}`, { waitUntil: "load" });
        await page.waitForSelector("[data-chat-scroll-region] .chat-message:last-child");
        await page.waitForTimeout(1000); // warmup/settle
        const inputLatencies = [];
        const traceResult = await withTrace(page, async (cdp) => {
          const deadline = Date.now() + 30000;
          let sampled = 0;
          // First 15 steps: wheel-based input→event latency samples.
          while (Date.now() < deadline && sampled < 15) {
            await page.mouse.move(720, 450);
            await page.evaluate(() => window.__perf.arm());
            const dispatchAt = Date.now();
            await page.mouse.wheel(0, -500);
            await page.waitForTimeout(120);
            const stamps = await page.evaluate(() => window.__perf.inputLatencies.splice(0));
            for (const stamp of stamps) inputLatencies.push(stamp - dispatchAt);
            sampled += 1;
          }
          // Remaining window: continuous gesture scrolling for real frame cadence.
          await scrollGestureFor(cdp, page, Math.max(0, deadline - Date.now()), { distance: 2500, speed: 2500 });
        });
        const stats = await page.evaluate(() => ({
          dom: window.__perf.domNodes(),
          scroller: window.__perf.scrollerStats(),
          heap: window.__perf.heap(),
          longtasks: window.__longtasks,
        }));
        sizeResult.scenarios.S2_scroll_up_30s.push({
          run,
          drawFrame: traceResult.drawFrame,
          rawFrameCount: traceResult.rawFrameCount,
          discontinuities: traceResult.discontinuities,
          input_latency_ms: {
            samples: inputLatencies.length,
            p50: percentile(inputLatencies, 0.5),
            p95: percentile(inputLatencies, 0.95),
          },
          ...stats,
        });
        await context.close();
      }

      // --- S3: fast oscillating scroll ------------------------------------
      sizeResult.scenarios.S3_fast_scroll = [];
      for (let run = 0; run < 3; run += 1) {
        const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 });
        const page = await context.newPage();
        await page.addInitScript(() => {
          window.__longtasks = [];
          new PerformanceObserver((list) => {
            for (const entry of list.getEntries()) window.__longtasks.push(entry.duration);
          }).observe({ entryTypes: ["longtask"] });
        });
        await page.goto(`${BASE}/?perf=1&n=${size}&seed=${SEED}`, { waitUntil: "load" });
        await page.waitForSelector("[data-chat-scroll-region] .chat-message:last-child");
        await page.waitForTimeout(1000);
        const traceResult = await withTrace(page, (cdp) =>
          scrollGestureFor(cdp, page, 15000, { distance: 2500, speed: 6000, alternate: true }),
        );
        const stats = await page.evaluate(() => ({
          dom: window.__perf.domNodes(),
          scroller: window.__perf.scrollerStats(),
          heap: window.__perf.heap(),
          longtasks: window.__longtasks,
        }));
        sizeResult.scenarios.S3_fast_scroll.push({ run, drawFrame: traceResult.drawFrame, rawFrameCount: traceResult.rawFrameCount, discontinuities: traceResult.discontinuities, ...stats });
        await context.close();
      }

      // --- S4: scroll while streaming --------------------------------------
      sizeResult.scenarios.S4_scroll_while_streaming = [];
      for (let run = 0; run < 3; run += 1) {
        const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 });
        const page = await context.newPage();
        await page.addInitScript(() => {
          window.__longtasks = [];
          new PerformanceObserver((list) => {
            for (const entry of list.getEntries()) window.__longtasks.push(entry.duration);
          }).observe({ entryTypes: ["longtask"] });
        });
        await page.goto(`${BASE}/?perf=1&n=${size}&seed=${SEED}&stream=1&streamMs=20000`, { waitUntil: "load" });
        await page.waitForSelector("[data-chat-scroll-region] .chat-message:last-child");
        await page.waitForTimeout(2000); // stream warms up, view pinned to bottom
        const traceResult = await withTrace(page, (cdp) =>
          scrollGestureFor(cdp, page, 16000, { distance: 1500, speed: 1500 }),
        );
        const stats = await page.evaluate(() => ({
          dom: window.__perf.domNodes(),
          scroller: window.__perf.scrollerStats(),
          heap: window.__perf.heap(),
          longtasks: window.__longtasks,
        }));
        const streamState = await page.getAttribute("html", "data-stream-state");
        sizeResult.scenarios.S4_scroll_while_streaming.push({
          run, stream_state: streamState, drawFrame: traceResult.drawFrame, rawFrameCount: traceResult.rawFrameCount, discontinuities: traceResult.discontinuities, ...stats,
        });
        await context.close();
      }

      // --- S5: session switch ×20 ------------------------------------------
      sizeResult.scenarios.S5_session_switch_x20 = [];
      for (let run = 0; run < 3; run += 1) {
        const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 });
        const page = await context.newPage();
        await page.addInitScript(() => {
          window.__longtasks = [];
          new PerformanceObserver((list) => {
            for (const entry of list.getEntries()) window.__longtasks.push(entry.duration);
          }).observe({ entryTypes: ["longtask"] });
        });
        await page.goto(`${BASE}/?perf=1&n=${size}&seed=${SEED}`, { waitUntil: "load" });
        await page.waitForSelector("[data-chat-scroll-region] .chat-message:last-child");
        await page.waitForTimeout(800);
        const switches = [];
        for (let i = 0; i < 20; i += 1) {
          const target = i % 2 === 0 ? "b" : "a";
          const started = Date.now();
          await page.evaluate((t) => window.__perf.switchTo(t), target);
          const elapsed = Date.now() - started;
          switches.push(elapsed);
        }
        const probes = await page.evaluate(async () => {
          await window.__perf.switchTo("a");
          return { dom: window.__perf.domNodes(), scroller: window.__perf.scrollerStats(), heap: window.__perf.heap(), longtasks: window.__longtasks };
        });
        sizeResult.scenarios.S5_session_switch_x20.push({
          run,
          per_switch_ms: { p50: percentile(switches, 0.5), p95: percentile(switches, 0.95), max: Math.max(...switches) },
          ...probes,
        });
        await context.close();
      }

      results.sizes[size] = sizeResult;
      console.log(`[perf] n=${size} done`);
    }
  } finally {
    await browser.close().catch(() => undefined);
    server.kill("SIGTERM");
  }

  const outDir = path.join(WEB_ROOT, "perf", "results");
  await mkdir(outDir, { recursive: true });
  const outFile = path.join(outDir, `perf-results-${Date.now()}.json`);
  await writeFile(outFile, JSON.stringify(results, null, 2));
  console.log(`[perf] wrote ${outFile}`);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
