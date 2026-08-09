#!/usr/bin/env bash
# INPUT:  running vite dev server + a vision-capable OpenAI-compatible model
# OUTPUT: AI visual review of a screenshot — color, contrast, hierarchy,
#         density, whitespace, polish. Complements ui-review.sh (text
#         blueprint) with what only a rendered image shows.
#
# Usage:
#   scripts/ui-review-vision.sh [url] [--model=Qwen3-VL-4B-Instruct-MLX-8bit]
#   Env: OLLAMA_URL, AUTH_KEY

set -euo pipefail
OLLAMA_URL="${OLLAMA_URL:-http://localhost:11434}"
AUTH_KEY="${AUTH_KEY:-}"
MODEL="Qwen3-VL-4B-Instruct-MLX-8bit"
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

PORT=9223
CHROME="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
SHOT="/tmp/hub-ui-review.png"
rm -f "$SHOT"
"$CHROME" --headless=new --disable-gpu --screenshot="$SHOT" \
  --window-size="$SIZE" --hide-scrollbars --virtual-time-budget=4000 \
  "$URL" >/dev/null 2>&1 &
CHROME_PID=$!
trap 'kill $CHROME_PID 2>/dev/null || true' EXIT
wait $CHROME_PID 2>/dev/null || true
[ -s "$SHOT" ] || { echo "ERROR: screenshot failed" >&2; exit 1; }

if ! curl -s -m 2 ${AUTH_KEY:+-H "Authorization: Bearer $AUTH_KEY"} "$OLLAMA_URL/v1/models" >/dev/null; then
  echo "ERROR: no model runtime at $OLLAMA_URL (or bad AUTH_KEY)" >&2
  exit 1
fi

PROMPT=$(cat <<EOF
你是资深视觉设计师。下面是一张 Tauri 桌面应用（React + Tailwind，深色主题）的界面截图。
请做严肃的视觉审查，输出 markdown：
## 第一印象（整体观感、是否"高级"）
## 配色与对比度（层次、可读性、状态色是否协调）
## 层次与节奏（主次分明？F/Z 阅读流？）
## 密度与留白（拥挤或空旷？间距一致？）
## 对齐与一致性（网格、边距、圆角、字体层级）
## 具体修复清单（按影响排序：问题 → 位置 → 改法，Tailwind/CSS 级）
只描述你在截图中实际看到的，不要臆测。
EOF
)

B64=$(base64 -i "$SHOT")
echo "# AI visual review ($URL) — model $MODEL"
echo
curl -s --max-time 560 "$OLLAMA_URL/v1/chat/completions" \
  ${AUTH_KEY:+-H "Authorization: Bearer $AUTH_KEY"} \
  -H "Content-Type: application/json" \
  -d "$(node -e '
const [b64, prompt, model] = process.argv.slice(1);
process.stdout.write(JSON.stringify({
  model, stream: false, temperature: 0.2,
  messages: [{
    role: "user",
    content: [
      { type: "image_url", image_url: { url: "data:image/png;base64," + b64 } },
      { type: "text", text: prompt },
    ],
  }],
}));' "$B64" "$PROMPT" "$MODEL")" \
| node -e '
let d = ""; process.stdin.on("data", c => (d += c));
process.stdin.on("end", () => {
  try {
    const j = JSON.parse(d);
    if (j.error) { console.error("model error:", JSON.stringify(j.error).slice(0, 300)); process.exit(1); }
    console.log(j.choices?.[0]?.message?.content ?? "(empty response)");
  } catch (e) { console.error("bad response:", d.slice(0, 300)); process.exit(1); }
});'
