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

# Rule 1: agent-types has ZERO workspace deps
check_no_workspace_deps "agent-types"

# Rule 2: agent-core only depends on agent-types
check_no_workspace_deps "agent-core" "agent-types"

# Rule 3: agent-tools only depends on agent-types
check_no_workspace_deps "agent-tools" "agent-types"

# Rule 4: agent-security only depends on agent-types
check_no_workspace_deps "agent-security" "agent-types"

# Rule 5: agent-memory only depends on agent-types
check_no_workspace_deps "agent-memory" "agent-types"

# Rule 6: agent-runtime only depends on foundation agent-* crates
check_no_workspace_deps "agent-runtime" "agent-types|agent-core|agent-tools|agent-security|agent-memory|agent-graph"

# Rule 7: protocol crates don't depend on srow-*
echo "Checking protocol crates..."
for proto in protocol-context-skill protocol-model-context protocol-agent-client; do
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
app_deps=$(cargo tree -p srow-app --depth 1 --prefix none 2>/dev/null | grep -E "^(agent-types|agent-core|agent-graph|agent-tools|agent-security|agent-memory|agent-runtime) " || true)
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
