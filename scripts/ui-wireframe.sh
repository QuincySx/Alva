#!/usr/bin/env bash
# INPUT:  a running vite/dev server URL (default: Hub dev server)
# OUTPUT: wireframe of the rendered page (layout tree + ASCII blueprint)
# POS:    UI review without screenshots — see web/scripts/ui-wireframe.mjs
#
# Usage:
#   scripts/ui-wireframe.sh [url] [--size=WxH] [--click=selector]
set -euo pipefail
URL="${1:-http://127.0.0.1:1421}"
EXTRA="${*:2}"
PORT=9223
CHROME="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
"$CHROME" --headless=new --remote-debugging-port=$PORT --user-data-dir=/tmp/chrome-wire about:blank >/dev/null 2>&1 &
CHROME_PID=$!
trap 'kill $CHROME_PID 2>/dev/null || true' EXIT
sleep 2
node crates/alva-hub-desktop/web/scripts/ui-wireframe.mjs "$URL" --port=$PORT $EXTRA
