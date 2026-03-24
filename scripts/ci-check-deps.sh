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
    deps=$(cargo tree -p "$crate" --depth 1 --prefix none 2>/dev/null | grep -E "^(agent-|srow-|protocol-)" | grep -v "^${crate} " || true)

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

# Rule 1: alva-types has ZERO workspace deps
check_no_workspace_deps "alva-types"

# Rule 2: alva-agent-core only depends on alva-types
check_no_workspace_deps "alva-agent-core" "alva-types"

# Rule 3: alva-agent-tools only depends on alva-types
check_no_workspace_deps "alva-agent-tools" "alva-types"

# Rule 4: alva-agent-security only depends on alva-types
check_no_workspace_deps "alva-agent-security" "alva-types"

# Rule 5: alva-agent-memory only depends on alva-types
check_no_workspace_deps "alva-agent-memory" "alva-types"

# Rule 6: alva-agent-runtime only depends on foundation agent-* crates
check_no_workspace_deps "alva-agent-runtime" "alva-types|alva-agent-core|alva-agent-tools|alva-agent-security|alva-agent-memory|alva-agent-graph"

# Rule 7: protocol crates don't depend on srow-*
echo "Checking protocol crates..."
for proto in alva-protocol-skill alva-protocol-mcp alva-protocol-acp; do
    local_deps=$(cargo tree -p "$proto" --depth 1 --prefix none 2>/dev/null | grep -E "^srow-" || true)
    if [ -n "$local_deps" ]; then
        echo -e "${RED}VIOLATION: $proto depends on srow crates:${NC}"
        echo "$local_deps"
        VIOLATIONS=$((VIOLATIONS + 1))
    else
        echo -e "${GREEN}OK: $proto${NC}"
    fi
done

# Rule 8: srow-app must NOT directly depend on internal agent-* crates
echo "Checking srow-app facade boundary..."
app_deps=$(cargo tree -p srow-app --depth 1 --prefix none 2>/dev/null | grep -E "^(alva-types|alva-agent-core|alva-agent-graph|alva-agent-tools|alva-agent-security|alva-agent-memory|alva-agent-runtime) " || true)
if [ -n "$app_deps" ]; then
    echo -e "${RED}VIOLATION: srow-app directly depends on internal crates:${NC}"
    echo "$app_deps"
    VIOLATIONS=$((VIOLATIONS + 1))
else
    echo -e "${GREEN}OK: srow-app uses facade only${NC}"
fi

echo ""
if [ $VIOLATIONS -gt 0 ]; then
    echo -e "${RED}FAILED: $VIOLATIONS dependency violation(s) found${NC}"
    exit 1
else
    echo -e "${GREEN}PASSED: All dependency boundaries are clean${NC}"
fi
