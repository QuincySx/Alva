#!/usr/bin/env bash
# INPUT:  model id on a running OpenAI-compatible endpoint
# OUTPUT: PASS/FAIL verification — can it see (vision) and can it reason (text)
# POS:    Quick model smoke test before swapping a reviewer model in. Two
#         short probes (fast, stable) instead of the full review prompt:
#           1. vision: describe the actual screenshot layout
#           2. text:   answer from the wireframe facts
#
# Usage:  scripts/model-verify.sh <model-id> [url]
# Env:    OLLAMA_URL, AUTH_KEY

set -euo pipefail
MODEL="${1:?usage: model-verify.sh <model-id> [url]}"
URL="${2:-http://127.0.0.1:1421}"
OLLAMA_URL="${OLLAMA_URL:-http://localhost:11434}"
AUTH_KEY="${AUTH_KEY:-}"

post() { # post <body-file> -> content
  curl -s --max-time 240 "$OLLAMA_URL/v1/chat/completions" \
    ${AUTH_KEY:+-H "Authorization: Bearer $AUTH_KEY"} \
    -H "Content-Type: application/json" \
    -d @"$1" > /tmp/model-verify-resp.json || { echo "<http error>"; return 1; }
  node -e '
    const fs = require("fs");
    const j = JSON.parse(fs.readFileSync("/tmp/model-verify-resp.json", "utf8"));
    if (j.error) { console.error("ERR:", JSON.stringify(j.error).slice(0, 200)); process.exit(1); }
    console.log(j.choices?.[0]?.message?.content ?? "(empty)");
  ' || { echo "<bad response>"; return 1; }
}

echo "# verify $MODEL @ $OLLAMA_URL"

# ── probe 1: vision (screenshot) ─────────────────────────────
PORT=9223
CHROME="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
SHOT="/tmp/model-verify.png"
rm -f "$SHOT"
"$CHROME" --headless=new --disable-gpu --screenshot="$SHOT" \
  --window-size=1180x760 --hide-scrollbars --virtual-time-budget=4000 \
  "$URL" >/dev/null 2>&1 || true
if [ -s "$SHOT" ]; then
  B64=$(base64 -i "$SHOT")
  node -e '
    const fs = require("fs");
    const b64 = process.argv[1], model = process.argv[2];
    fs.writeFileSync("/tmp/verify-vision-body.json", JSON.stringify({
      model, stream: false, temperature: 0,
      messages: [{ role: "user", content: [
        { type: "image_url", image_url: { url: "data:image/png;base64," + b64 } },
        { type: "text", text: "这张界面截图：左侧边栏里从上到下有几个面板？回答一句。" },
      ] }],
    }));
  ' "$B64" "$MODEL"
  echo "VISION: $(post /tmp/verify-vision-body.json || echo failed)"
else
  echo "VISION: <screenshot failed>"
fi

# ── probe 2: text (wireframe facts) ──────────────────────────
CHROME="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
"$CHROME" --headless=new --remote-debugging-port=$PORT --user-data-dir=/tmp/chrome-wire about:blank >/dev/null 2>&1 &
CDP_PID=$!
sleep 2
WIRE=$(node crates/alva-hub-desktop/web/scripts/ui-wireframe.mjs "$URL" --port=$PORT --size=1180x760 2>/dev/null | sed -n '/Layout tree/,$p') || WIRE=""
kill $CDP_PID 2>/dev/null || true
if [ -n "$WIRE" ]; then
  node -e '
    const fs = require("fs");
    const wire = process.argv[1], model = process.argv[2];
    fs.writeFileSync("/tmp/verify-text-body.json", JSON.stringify({
      model, stream: false, temperature: 0,
      messages: [{ role: "user", content: "线稿中 .sidebar 面板的宽度是多少像素？回答一个数字。\n\n" + wire }],
    }));
  ' "$WIRE" "$MODEL"
  echo "TEXT: $(post /tmp/verify-text-body.json || echo failed)"
else
  echo "TEXT: <wireframe failed>"
fi

echo "DONE"
