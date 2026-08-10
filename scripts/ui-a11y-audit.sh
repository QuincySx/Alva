#!/usr/bin/env bash
# INPUT:  running vite dev server
# OUTPUT: accessibility audit — every interactive element needs an
#         accessible name (aria-label / title / text / placeholder)
# POS:    Local, deterministic a11y gate (no model needed). Complements
#         the wireframe/review tools.
#
# Usage:  scripts/ui-a11y-audit.sh [url]

set -euo pipefail
URL="${1:-http://127.0.0.1:1421}"
PORT=9223
CHROME="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
"$CHROME" --headless=new --remote-debugging-port=$PORT --user-data-dir=/tmp/chrome-wire about:blank >/dev/null 2>&1 &
CHROME_PID=$!
trap 'kill $CHROME_PID 2>/dev/null || true' EXIT
sleep 2

node -e '
const url = process.argv[1];
(async () => {
  const target = await (await fetch("http://127.0.0.1:9223/json/new?" + encodeURIComponent(url), { method: "PUT" })).json();
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  let id = 0; const pending = new Map();
  const send = (m, p={}) => new Promise((res, rej) => { const i = ++id; pending.set(i, {res, rej}); ws.send(JSON.stringify({id:i, method:m, params:p})); });
  ws.onmessage = (ev) => { const m = JSON.parse(ev.data); if (m.id && pending.has(m.id)) { const p = pending.get(m.id); pending.delete(m.id); m.error ? p.rej(new Error(m.error.message)) : p.res(m.result); } };
  await new Promise((res) => ws.onopen = res);
  await send("Runtime.evaluate", { expression: "new Promise(r=>setTimeout(r,2500))", awaitPromise: true });
  const r = await send("Runtime.evaluate", { expression: `(() => {
    const bad = [];
    document.querySelectorAll("button, a, input, select, textarea").forEach(el => {
      if (el.disabled) return;
      const name = el.getAttribute("aria-label") || el.getAttribute("title") || (el.textContent || "").trim() || el.getAttribute("placeholder") || "";
      if (!name) bad.push(el.tagName + "." + (el.className || "").toString().slice(0, 40));
    });
    return JSON.stringify({ total: document.querySelectorAll("button,a,input,select,textarea").length, nameless: bad });
  })()`, returnByValue: true });
  const audit = JSON.parse(r.result.value);
  console.log(`# a11y audit ${url}`);
  console.log(`interactive: ${audit.total} | nameless: ${audit.nameless.length}`);
  if (audit.nameless.length) {
    console.log("missing names:");
    audit.nameless.forEach(n => console.log("  - " + n));
    process.exitCode = 1;
  } else {
    console.log("PASS — every interactive element has an accessible name");
  }
  ws.close();
  setTimeout(() => process.exit(process.exitCode ?? 0), 200);
})().catch(e => { console.error(e.message); process.exit(1); });
' "$URL"
