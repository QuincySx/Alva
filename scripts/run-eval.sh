#!/usr/bin/env bash
#
# Run the real-LLM agent tool eval, persistently codesign'd to bypass
# the macOS Application Firewall path-vs-identity churn.
#
# Why: cargo test produces a fresh test binary every build; without a
# stable codesign identity, macOS firewall sees a brand-new "unknown
# app" each time and silently blocks outbound TCP. Signing every build
# with the SAME identity lets one firewall whitelist entry cover all
# future rebuilds.
#
# One-time setup:
#   1. Create a self-signed code-signing certificate (Keychain Access →
#      Certificate Assistant → Create a Certificate → name "alva-eval-signing",
#      Identity Type "Self Signed Root", Certificate Type "Code Signing").
#   2. Run this script once. The first time you'll see a macOS firewall
#      prompt — click "Allow". After that, all future rebuilds inherit
#      the same identity and stay allowed.
#
# Environment overrides:
#   EVAL_SIGN_IDENTITY   default: alva-eval-signing
#   EVAL_BASE_URL        default: built into the test
#   EVAL_MODEL           default: built into the test
#   EVAL_API_KEY         default: built into the test
#   EVAL_REPEATS         default: 3
#   EVAL_MAX_ITERS       default: 15

set -euo pipefail

cd "$(dirname "$0")/.."

SIGN_ID="${EVAL_SIGN_IDENTITY:-alva-eval-signing}"

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

# 2. Build the test binary without running it.
echo "==> Building eval_agent_tools test binary…"
cargo test -p alva-app-core --test eval_agent_tools --no-run --quiet

# 3. Locate the just-built binary (newest, non-.d under target/debug/deps).
TEST_BIN=$(ls -t target/debug/deps/eval_agent_tools-* 2>/dev/null \
    | grep -v '\.d$' \
    | head -1 || true)

if [[ -z "${TEST_BIN}" || ! -x "$TEST_BIN" ]]; then
    echo "error: could not locate built eval_agent_tools binary" >&2
    exit 1
fi
echo "==> Test binary: $TEST_BIN"

# 4. Codesign with the stable identity. --force overwrites any prior
#    adhoc signature cargo applied.
echo "==> Signing with identity \"$SIGN_ID\"…"
codesign --force --sign "$SIGN_ID" "$TEST_BIN"

# 5. Verify the signature was applied.
if ! codesign -dv "$TEST_BIN" 2>&1 | grep -q "Authority=$SIGN_ID"; then
    echo "warning: codesign verify did not see expected identity; firewall may still prompt" >&2
fi

# 6. Run the eval, bypassing the in-test TCP probe (which uses a
#    different code path that the firewall also rate-limits).
echo "==> Running eval (this hits the real LLM; expect ~3-10 minutes)…"
echo
exec env EVAL_SKIP_PROBE=1 \
    "$TEST_BIN" \
    --ignored \
    --nocapture \
    --test-threads=1
