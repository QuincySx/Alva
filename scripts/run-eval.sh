#!/usr/bin/env bash
#
# Run the real-LLM agent tool eval, working around macOS 15 (Sequoia)
# Local Network Privacy gating.
#
# Why this is needed: macOS Sequoia silently blocks outbound TCP from
# non-Apple-signed binaries to RFC1918 ranges (10.x / 192.168.x /
# 172.16-31.x), returning EHOSTUNREACH on the first call from a new
# binary identity. A GUI prompt asks the user to allow Local Network
# access, but it only appears in a GUI terminal session — over SSH or
# in headless contexts, no prompt fires and the binary stays blocked.
# Worse, very short-lived binaries (the default for `cargo test`) exit
# before the prompt has a chance to surface.
#
# This script:
#   1. Resigns the test binary with a STABLE codesign identity
#      ("alva-eval-signing", a one-time self-signed cert) plus the
#      eval-entitlements.plist. Stable identity = the macOS Local
#      Network grant persists across rebuilds.
#   2. On first run (or after toggling permissions off), drives a
#      warmup loop that keeps the process alive long enough for the
#      GUI prompt to appear — run inside iTerm/Terminal.app and click
#      Allow when prompted.
#   3. Once warmed up, runs the full eval.
#
# Usage:
#     scripts/run-eval.sh              # warmup if needed, then full eval
#     scripts/run-eval.sh --warmup-only  # just the warmup loop (use in
#                                        # GUI terminal, click Allow)
#
# One-time setup:
#   Create a self-signed Code Signing certificate named
#   "alva-eval-signing" via Keychain Access → Certificate Assistant →
#   Create a Certificate (Identity Type "Self Signed Root", Certificate
#   Type "Code Signing").

set -euo pipefail

cd "$(dirname "$0")/.."

SIGN_ID="${EVAL_SIGN_IDENTITY:-alva-eval-signing}"
ENT_PLIST="$(pwd)/scripts/eval-entitlements.plist"

WARMUP_ONLY=0
if [[ "${1:-}" == "--warmup-only" ]]; then
    WARMUP_ONLY=1
fi

# 1. Verify the signing identity exists.
if ! security find-certificate -c "$SIGN_ID" >/dev/null 2>&1; then
    cat <<EOF >&2
error: code-signing identity "$SIGN_ID" not found in keychain.

Create it once via Keychain Access:
  Certificate Assistant → Create a Certificate
    Name:             $SIGN_ID
    Identity Type:    Self Signed Root
    Certificate Type: Code Signing

Or override the identity by exporting EVAL_SIGN_IDENTITY=<your-cert-name>.
EOF
    exit 1
fi

# 2. Sanity-check the entitlements file is in place.
if [[ ! -f "$ENT_PLIST" ]]; then
    echo "error: entitlements file missing at $ENT_PLIST" >&2
    exit 1
fi

# 3. Build the test binary without running it.
echo "==> Building eval_agent_tools test binary…"
cargo test -p alva-app-core --test eval_agent_tools --no-run --quiet

# 4. Locate the just-built binary (newest, non-.d under target/debug/deps).
TEST_BIN=$(ls -t target/debug/deps/eval_agent_tools-* 2>/dev/null \
    | grep -v '\.d$' \
    | head -1 || true)

if [[ -z "${TEST_BIN}" || ! -x "$TEST_BIN" ]]; then
    echo "error: could not locate built eval_agent_tools binary" >&2
    exit 1
fi
echo "==> Test binary: $TEST_BIN"

# 5. Codesign with stable identity + entitlements. --force overrides
#    cargo's transient ad-hoc signature.
echo "==> Signing with identity \"$SIGN_ID\" + entitlements…"
codesign --force --sign "$SIGN_ID" --entitlements "$ENT_PLIST" "$TEST_BIN"

# 6. Verify the signature took.
if ! codesign -dv "$TEST_BIN" 2>&1 | grep -q "Authority=$SIGN_ID"; then
    echo "warning: codesign verify did not see expected identity" >&2
fi

# 7. Always run the warmup first — it's idempotent and finishes
#    immediately once the permission has been granted.
echo
echo "==> Running warmup probe (Local Network access check)…"
echo "    If macOS shows a prompt asking to allow Local Network access,"
echo "    CLICK ALLOW. The warmup will retry for ~60s to give you time."
echo
set +e
EVAL_SKIP_PROBE=1 "$TEST_BIN" --ignored --nocapture --test-threads=1 eval_warmup
WARMUP_RC=$?
set -e

if [[ $WARMUP_RC -ne 0 ]]; then
    echo
    echo "==> Warmup test exited non-zero. See output above for diagnosis." >&2
    exit $WARMUP_RC
fi

if [[ "$WARMUP_ONLY" == "1" ]]; then
    echo
    echo "==> --warmup-only: done."
    exit 0
fi

# 8. Run the full eval.
echo
echo "==> Running full eval (10 cases × 3 repeats; expect a few minutes)…"
echo
exec env EVAL_SKIP_PROBE=1 \
    "$TEST_BIN" \
    --ignored \
    --nocapture \
    --test-threads=1 \
    eval_agent_tools_main
