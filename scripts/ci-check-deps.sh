#!/usr/bin/env bash
# CI Dependency Firewall — ensures foundation crates don't depend on application crates.
set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m'

VIOLATIONS=0

check_no_workspace_deps() {
    local crate=$1
    local allowed=${2:-""}

    echo "Checking $crate..."
    local deps
    deps=$(cargo tree -p "$crate" --depth 1 --prefix none 2>/dev/null | grep -E "^(alva-agent-|alva-app|alva-protocol-|alva-engine-)" | grep -v "^${crate} " || true)

    if [ -n "$allowed" ]; then
        deps=$(echo "$deps" | grep -v -E "^($allowed) " || true)
    fi

    if [ -n "$deps" ]; then
        echo -e "${RED}VIOLATION: $crate has unexpected workspace deps:${NC}"
        echo "$deps"
        VIOLATIONS=$((VIOLATIONS + 1))
    else
        echo -e "${GREEN}OK${NC}"
    fi
}

# Rule 0: alva-kernel-bus has ZERO workspace deps
check_no_workspace_deps "alva-kernel-bus"

# Rule 1: alva-kernel-abi only depends on alva-kernel-bus
check_no_workspace_deps "alva-kernel-abi" "alva-kernel-bus"

# Rule 2: alva-kernel-core only depends on alva-kernel-abi
check_no_workspace_deps "alva-kernel-core" "alva-kernel-abi"

# Rule 3: alva-agent-tools only depends on alva-kernel-abi
check_no_workspace_deps "alva-agent-tools" "alva-kernel-abi"

# Rule 4: alva-agent-security depends on alva-kernel-abi + alva-kernel-core
#         (core is needed because security owns SecurityMiddleware + PlanModeMiddleware)
check_no_workspace_deps "alva-agent-security" "alva-kernel-abi|alva-kernel-core"

# Rule 5: alva-agent-memory only depends on alva-kernel-abi
check_no_workspace_deps "alva-agent-memory" "alva-kernel-abi"

# Rule 7: alva-agent-graph only depends on alva-kernel-abi + alva-kernel-core
check_no_workspace_deps "alva-agent-graph" "alva-kernel-abi|alva-kernel-core"

# Rule 7b: alva-agent-context depends on alva-kernel-abi + alva-kernel-core
#          (core is needed because context owns CompactionMiddleware)
check_no_workspace_deps "alva-agent-context" "alva-kernel-abi|alva-kernel-core"

# Rule 8: alva-host-native only depends on foundation crates
check_no_workspace_deps "alva-host-native" "alva-kernel-abi|alva-kernel-core|alva-agent-tools|alva-agent-security|alva-agent-context|alva-agent-memory|alva-agent-graph"

# Rule 9: alva-engine-runtime only depends on alva-kernel-abi
check_no_workspace_deps "alva-engine-runtime" "alva-kernel-abi"

# Rule 10: alva-engine-adapter-claude only depends on alva-kernel-abi + alva-engine-runtime
check_no_workspace_deps "alva-engine-adapter-claude" "alva-kernel-abi|alva-engine-runtime"

# Rule 11: alva-engine-adapter-alva only depends on alva-kernel-abi + alva-engine-runtime + alva-kernel-core
check_no_workspace_deps "alva-engine-adapter-alva" "alva-kernel-abi|alva-engine-runtime|alva-kernel-core"

# Rule 12: alva-provider only depends on alva-kernel-abi
check_no_workspace_deps "alva-provider" "alva-kernel-abi"

# Rule 13: alva-environment has ZERO workspace deps
check_no_workspace_deps "alva-environment"

# Rule 14: protocol crates have strict dependencies
check_no_workspace_deps "alva-protocol-skill"  # zero workspace deps
check_no_workspace_deps "alva-protocol-mcp" "alva-kernel-abi"  # only alva-kernel-abi
check_no_workspace_deps "alva-protocol-acp"  # zero workspace deps

# Rule 15: protocol crates don't depend on alva-app-*
echo "Checking protocol crates..."
for proto in alva-protocol-skill alva-protocol-mcp alva-protocol-acp; do
    local_deps=$(cargo tree -p "$proto" --depth 1 --prefix none 2>/dev/null | grep -E "^alva-app" || true)
    if [ -n "$local_deps" ]; then
        echo -e "${RED}VIOLATION: $proto depends on alva-app crates:${NC}"
        echo "$local_deps"
        VIOLATIONS=$((VIOLATIONS + 1))
    else
        echo -e "${GREEN}OK: $proto${NC}"
    fi
done

# Rule 16: alva-app must NOT directly depend on internal agent-* crates
echo "Checking alva-app facade boundary..."
app_deps=$(cargo tree -p alva-app --depth 1 --prefix none 2>/dev/null | grep -E "^(alva-kernel-abi|alva-kernel-core|alva-agent-graph|alva-agent-tools|alva-agent-security|alva-agent-memory|alva-host-native) " || true)
if [ -n "$app_deps" ]; then
    echo -e "${RED}VIOLATION: alva-app directly depends on internal crates:${NC}"
    echo "$app_deps"
    VIOLATIONS=$((VIOLATIONS + 1))
else
    echo -e "${GREEN}OK: alva-app uses facade only${NC}"
fi

echo ""
if [ $VIOLATIONS -gt 0 ]; then
    echo -e "${RED}FAILED: $VIOLATIONS dependency violation(s) found${NC}"
    exit 1
else
    echo -e "${GREEN}PASSED: All dependency boundaries are clean${NC}"
fi

# ---------------------------------------------------------------------------
# Phase 5 invariant: kernel layers must compile for wasm32-unknown-unknown
# without any host-specific deps.
# ---------------------------------------------------------------------------
echo ""
echo "Checking kernel wasm32 compilability..."

WASM_OK=true
check_wasm() {
    local crate=$1
    if cargo check --target wasm32-unknown-unknown -p "$crate" >/dev/null 2>&1; then
        echo -e "${GREEN}OK: $crate compiles for wasm32${NC}"
    else
        echo -e "${RED}VIOLATION: $crate does NOT compile for wasm32${NC}"
        WASM_OK=false
    fi
}

# Skip if wasm32 target is not installed — the dep check still runs.
if rustup target list --installed 2>/dev/null | grep -q '^wasm32-unknown-unknown$'; then
    check_wasm "alva-kernel-bus"
    check_wasm "alva-kernel-abi"
    check_wasm "alva-kernel-core"
    check_wasm "alva-agent-context"
    check_wasm "alva-agent-graph"
    check_wasm "alva-agent-security"
    check_wasm "alva-agent-tools"
    check_wasm "alva-agent-memory"
    check_wasm "alva-host-native"
    check_wasm "alva-host-wasm"
    if [ "$WASM_OK" != "true" ]; then
        echo -e "${RED}FAILED: wasm32 invariant broken${NC}"
        exit 1
    fi
    echo -e "${GREEN}PASSED: kernel + 5 L3 boxes + 2 hosts all wasm32-clean${NC}"

    # Stronger check: actually BUILD (link) alva-host-wasm for wasm32 at
    # least once, to catch issues cargo check misses (missing symbols,
    # feature unification with unrelated workspace crates, etc.).
    echo ""
    echo "Building alva-host-wasm for wasm32 (full link)..."
    if cargo build --target wasm32-unknown-unknown -p alva-host-wasm >/dev/null 2>&1; then
        echo -e "${GREEN}OK: alva-host-wasm builds (links) for wasm32${NC}"
    else
        echo -e "${RED}VIOLATION: alva-host-wasm fails to build for wasm32${NC}"
        exit 1
    fi
else
    echo -e "${GREEN}SKIPPED: wasm32-unknown-unknown target not installed${NC}"
    echo "         install with: rustup target add wasm32-unknown-unknown"
fi
