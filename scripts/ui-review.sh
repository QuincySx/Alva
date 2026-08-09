#!/usr/bin/env bash
# INPUT:  running vite dev server + a local OpenAI-compatible model (Ollama)
# OUTPUT: AI UI review of the live page — layout/density/alignment/a11y/consistency
# POS:    Zero-cost UI review loop: text wireframe (no vision needed) →
#         local model → structured recommendations. Complements
#         scripts/ui-wireframe.sh (which prints the raw blueprint).
#
# Usage:
#   ollama serve &                      # start the local runtime first
#   scripts/ui-review.sh [url] [--model qwen3:8b] [--size=1180x760]
#
# Env:    OLLAMA_URL (OpenAI-compatible base, default http://localhost:11434)
#         AUTH_KEY (Bearer token; empty = no auth)

set -euo pipefail
OLLAMA_URL="${OLLAMA_URL:-http://localhost:11434}"
AUTH_KEY="${AUTH_KEY:-}"
MODEL="qwen3:8b"
SIZE="1180x760"
URL=""
for arg in "$@"; do
  case "$arg" in
    --model=*) MODEL="${arg#--model=}" ;;
    --size=*) SIZE="${arg#--size=}" ;;
    *) [ -z "$URL" ] && URL="$arg" ;;
  esac
done
[ -z "$URL" ] && URL="http://127.0.0.1:1421"

# 1. blueprint the page (layout tree + ASCII) through the existing tool
PORT=9223
CHROME="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
"$CHROME" --headless=new --remote-debugging-port=$PORT --user-data-dir=/tmp/chrome-wire about:blank >/dev/null 2>&1 &
CHROME_PID=$!
trap 'kill $CHROME_PID 2>/dev/null || true' EXIT
sleep 2
WIRE=$(node crates/alva-hub-desktop/web/scripts/ui-wireframe.mjs "$URL" --port=$PORT --size=$SIZE 2>/dev/null | sed -n '/Layout tree/,$p')

# 2. build the review prompt around the blueprint
PROMPT=$(cat <<EOF
你是资深 UI 工程师。下面是一份 Web 应用（Tauri + React + Tailwind，深色主题）
的运行时线稿：布局树（[类型] x,y 宽x高 .class "标签"）和 ASCII 蓝图。

请做一次严肃的 UI 审查，输出 markdown：
## 布局（结构、空白、对齐、溢出、密度）
## 交互（控件可发现性、层级、状态可见）
## 可访问性（对比度、目标尺寸、aria 缺口——只基于线稿可推断的）
## 一致性（间距/圆角/配色是否统一）
## 优先级修复清单（按影响排序，每条：问题 → 位置 → 具体改法，Tailwind 或 CSS 级）

线稿：
$WIRE
EOF
)

# 3. ask the LOCAL model (OpenAI-compatible /v1/chat/completions)
if ! curl -s -m 2 ${AUTH_KEY:+-H "Authorization: Bearer $AUTH_KEY"} "$OLLAMA_URL/v1/models" >/dev/null; then
  echo "ERROR: no model runtime at $OLLAMA_URL (or bad AUTH_KEY)" >&2
  exit 1
fi

echo "# AI UI review ($URL) — model $MODEL"
echo
curl -s --max-time 560 "$OLLAMA_URL/v1/chat/completions" \
  ${AUTH_KEY:+-H "Authorization: Bearer $AUTH_KEY"} \
  -H "Content-Type: application/json" \
  -d "$(node -e '
const [prompt, model] = process.argv.slice(1);
process.stdout.write(JSON.stringify({
  model, stream: false, temperature: 0.2,
  messages: [{ role: "user", content: prompt }],
}));' "$PROMPT" "$MODEL")" \
| node -e '
let d = ""; process.stdin.on("data", c => (d += c));
process.stdin.on("end", () => {
  try {
    const j = JSON.parse(d);
    if (j.error) { console.error("model error:", j.error); process.exit(1); }
    console.log(j.choices?.[0]?.message?.content ?? "(empty response)");
  } catch (e) { console.error("bad response:", d.slice(0, 300)); process.exit(1); }
});'
