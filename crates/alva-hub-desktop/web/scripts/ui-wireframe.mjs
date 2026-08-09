// INPUT:  URL (defaults to the Hub dev server) + a running CDP Chrome
// OUTPUT: wireframe of the rendered page: layout tree + ASCII blueprint
// POS:    UI review without screenshots. Drives headless Chrome over the
//         DevTools Protocol with Node's built-in WebSocket (no deps),
//         injects scripts/wireframe.js, and prints its result.
//
// Usage:
//   node ui-wireframe.mjs [url] [--port 9223]
//   # with: Chrome --headless=new --remote-debugging-port=9223

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));
const WIREFRAME = readFileSync(join(__dirname, "wireframe.js"), "utf8");

const args = process.argv.slice(2);
const url = args.find((a) => a.startsWith("http")) ?? "http://127.0.0.1:1421";
const portArg = args.find((a) => a.startsWith("--port="));
const port = portArg ? Number(portArg.split("=")[1]) : 9223;

async function httpJson(path, opts) {
  const res = await fetch(`http://127.0.0.1:${port}${path}`, opts);
  return res.json();
}

const target = await httpJson("/json/new?" + encodeURIComponent(url), {
  method: "PUT",
});
const ws = new WebSocket(target.webSocketDebuggerUrl);
let nextId = 0;
const pending = new Map();
let settled = false;

function send(method, params = {}) {
  return new Promise((resolve, reject) => {
    const id = ++nextId;
    pending.set(id, { resolve, reject });
    ws.send(JSON.stringify({ id, method, params }));
  });
}

ws.onmessage = (ev) => {
  const msg = JSON.parse(ev.data);
  if (msg.id && pending.has(msg.id)) {
    const p = pending.get(msg.id);
    pending.delete(msg.id);
    if (msg.error) p.reject(new Error(msg.error.message));
    else p.resolve(msg.result);
  }
};

ws.onopen = async () => {
  try {
    await send("Runtime.enable");
    // Wait for the SPA to settle, then inject the wireframer.
    await send("Runtime.evaluate", {
      expression: `new Promise(r => setTimeout(r, 1500))`,
      awaitPromise: true,
    });
    const r = await send("Runtime.evaluate", {
      expression: `(${WIREFRAME})()`,
      returnByValue: true,
    });
    const wf = r.result.value;
    console.log(`# wireframe ${url}  (viewport ${wf.viewport}, ${wf.nodeCount} nodes)\n`);
    console.log("## Layout tree\n");
    console.log(wf.tree);
    console.log("\n## ASCII blueprint\n");
    console.log(wf.ascii);
  } catch (err) {
    console.error("wireframe failed:", err.message);
    process.exitCode = 1;
  } finally {
    settled = true;
    ws.close();
    process.exit(process.exitCode ?? 0);
  }
};

ws.onerror = (err) => {
  console.error("CDP error:", err.message);
  if (!settled) process.exit(1);
};

setTimeout(() => {
  if (!settled) {
    console.error("timed out waiting for CDP");
    process.exit(1);
  }
}, 15000);
