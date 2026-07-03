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
    # -e normal: shipped deps only (dev-deps like alva-test don't count).
    # --all-features: the boundary must hold under EVERY feature combination —
    #   a violation hidden behind an optional feature is still a violation
    #   (that is exactly how builtin→browser slipped through before).
    # ^alva-: match ALL workspace crates, not a hand-picked prefix subset —
    #   the old filter (agent/app/protocol/engine only) was blind to
    #   kernel/host/llm/macros deps.
    deps=$(cargo tree -p "$crate" --depth 1 --prefix none -e normal --all-features 2>/dev/null | grep -E "^alva-" | grep -v "^${crate} " || true)

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

# Rule 0a: alva-llm-wire has ZERO workspace deps
check_no_workspace_deps "alva-llm-wire"

# Rule 0: alva-kernel-bus has ZERO workspace deps
check_no_workspace_deps "alva-kernel-bus"

# Rule 1: alva-kernel-abi only depends on alva-kernel-bus + alva-llm-wire + alva-macros
check_no_workspace_deps "alva-kernel-abi" "alva-kernel-bus|alva-llm-wire|alva-macros"

# Rule 2: alva-kernel-core only depends on alva-kernel-abi
check_no_workspace_deps "alva-kernel-core" "alva-kernel-abi"

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
check_no_workspace_deps "alva-host-native" "alva-kernel-abi|alva-kernel-core|alva-agent-extension-builtin|alva-agent-core|alva-agent-security|alva-agent-context|alva-agent-memory|alva-agent-graph"

# Rule 9: alva-engine-runtime only depends on alva-kernel-abi
check_no_workspace_deps "alva-engine-runtime" "alva-kernel-abi"

# Rule 10: alva-engine-adapter-claude only depends on alva-kernel-abi + alva-engine-runtime
check_no_workspace_deps "alva-engine-adapter-claude" "alva-kernel-abi|alva-engine-runtime"

# Rule 11: alva-engine-adapter-alva only depends on alva-kernel-abi + alva-engine-runtime + alva-kernel-core
check_no_workspace_deps "alva-engine-adapter-alva" "alva-kernel-abi|alva-engine-runtime|alva-kernel-core"

# Rule 12: alva-llm-provider only depends on alva-kernel-abi
# (was `alva-provider` — a crate that does not exist; `cargo tree` failed,
#  the error was swallowed and the rule printed a fake OK forever)
check_no_workspace_deps "alva-llm-provider" "alva-kernel-abi"

# Rule 12b: alva-macros has ZERO workspace deps
check_no_workspace_deps "alva-macros"

# Rule 13: alva-environment has ZERO workspace deps
check_no_workspace_deps "alva-environment"

# Rule 14: protocol crates have strict dependencies
check_no_workspace_deps "alva-protocol-skill"  # zero workspace deps
check_no_workspace_deps "alva-protocol-mcp" "alva-kernel-abi"  # only alva-kernel-abi
check_no_workspace_deps "alva-protocol-acp"  # zero workspace deps

# Rule 15: protocol crates don't depend on alva-app-*
echo "Checking protocol crates..."
for proto in alva-protocol-skill alva-protocol-mcp alva-protocol-acp; do
    local_deps=$(cargo tree -p "$proto" --depth 1 --prefix none -e normal --all-features 2>/dev/null | grep -E "^alva-app" || true)
    if [ -n "$local_deps" ]; then
        echo -e "${RED}VIOLATION: $proto depends on alva-app crates:${NC}"
        echo "$local_deps"
        VIOLATIONS=$((VIOLATIONS + 1))
    else
        echo -e "${GREEN}OK: $proto${NC}"
    fi
done

# Rule 16: alva-agent-extension-builtin (SDK tool layer) only depends on the
# SDK crates below — in particular NEVER on any alva-app-* crate, under any
# feature. (The old Rule 16 checked a facade crate `alva-app` that does not
# exist; it printed a fake OK forever while the real tool-layer boundary
# went unchecked.)
check_no_workspace_deps "alva-agent-extension-builtin" "alva-kernel-abi|alva-agent-core|alva-agent-context|alva-agent-memory|alva-agent-security"

# Rule 17: Hard SDK boundary — no SDK crate (kernel / agent / protocol / engine /
#          macros / llm-provider / environment) may (transitively) depend on any
#          alva-app-* or alva-host-* crate. This is the single most important
#          layering rule: SDK is consumed by third parties building their own
#          harness, so it must not accidentally pull in our opinionated harness.
echo ""
echo "Checking hard SDK → app/host boundary (transitive)..."

SDK_CRATES=(
    alva-llm-wire
    alva-kernel-abi
    alva-kernel-bus
    alva-kernel-core
    alva-agent-context
    alva-agent-core
    alva-agent-extension-builtin
    alva-agent-graph
    alva-agent-memory
    alva-agent-security
    alva-engine-adapter-alva
    alva-engine-adapter-claude
    alva-engine-runtime
    alva-environment
    alva-llm-provider
    alva-macros
    alva-protocol-acp
    alva-protocol-mcp
    alva-protocol-skill
)

for crate in "${SDK_CRATES[@]}"; do
    # --all-features: a SDK→app/host edge hidden behind an optional feature
    # is still a violation — real builds (app-core) turn those features on.
    # -e normal: dev-deps don't ship, so they don't breach the boundary.
    bad=$(cargo tree -p "$crate" --prefix none -e normal --all-features 2>/dev/null \
        | grep -E "^alva-(app|host)" \
        || true)
    if [ -n "$bad" ]; then
        echo -e "${RED}VIOLATION: SDK crate $crate transitively depends on app/host:${NC}"
        echo "$bad" | sed 's/^/    /'
        VIOLATIONS=$((VIOLATIONS + 1))
    else
        echo -e "${GREEN}OK: $crate${NC}"
    fi
done

echo ""
if [ $VIOLATIONS -gt 0 ]; then
    echo -e "${RED}FAILED: $VIOLATIONS dependency violation(s) found${NC}"
    exit 1
else
    echo -e "${GREEN}PASSED: All dependency boundaries are clean${NC}"
fi

# ---------------------------------------------------------------------------
# Bus cap surface firewall — every #[bus_cap] trait must keep its signature
# type-surface ≤ 2 external crates. See docs/BUS-RULES.md § "Cap surface
# limit" for the contract. Tool lives at crates/alva-bus-lint/.
# ---------------------------------------------------------------------------
echo ""
echo "Checking bus cap surface (crates/alva-bus-lint)..."
cargo run --quiet -p alva-bus-lint

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
    check_wasm "alva-llm-wire"
    check_wasm "alva-kernel-bus"
    check_wasm "alva-kernel-abi"
    check_wasm "alva-kernel-core"
    check_wasm "alva-agent-context"
    check_wasm "alva-agent-core"
    check_wasm "alva-agent-extension-builtin"
    check_wasm "alva-agent-graph"
    check_wasm "alva-agent-security"
    check_wasm "alva-agent-memory"
    check_wasm "alva-host-wasm"
    check_wasm "alva-llm-provider"
    check_wasm "alva-protocol-mcp"
    check_wasm "alva-protocol-acp"
    check_wasm "alva-protocol-skill"
    check_wasm "alva-engine-runtime"
    check_wasm "alva-environment"
    check_wasm "alva-test"
    check_wasm "alva-macros"
    if [ "$WASM_OK" != "true" ]; then
        echo -e "${RED}FAILED: wasm32 invariant broken${NC}"
        exit 1
    fi
    echo -e "${GREEN}PASSED: wasm32-clean crate set${NC}"

    # Stronger check: actually BUILD (link) alva-host-wasm for wasm32 at
    # least once, to catch issues cargo check misses (missing symbols,
    # feature unification with unrelated workspace crates, etc.).
    echo ""
    echo "Building alva-host-wasm for wasm32 (full link)..."
    if CARGO_PROFILE_DEV_DEBUG=0 cargo build --target wasm32-unknown-unknown -p alva-host-wasm >/dev/null 2>&1; then
        echo -e "${GREEN}OK: alva-host-wasm builds (links) for wasm32${NC}"
    else
        echo -e "${RED}VIOLATION: alva-host-wasm fails to build for wasm32${NC}"
        exit 1
    fi
else
    echo -e "${GREEN}SKIPPED: wasm32-unknown-unknown target not installed${NC}"
    echo "         install with: rustup target add wasm32-unknown-unknown"
fi
