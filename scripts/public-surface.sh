#!/usr/bin/env bash
# Public-surface snapshot gate (kernel-boundary refactor, ticket 01).
#
# The public symbol surface of the kernel contract layer (alva-kernel-abi) and
# the SDK assembly layer (alva-agent-core) is snapshotted into version control.
# Any add / remove / change of a public symbol shows up as a snapshot diff that
# must be consciously acknowledged in the same commit:
#
#   ./scripts/public-surface.sh --check    # CI mode: fail with a diff on drift
#   ./scripts/public-surface.sh --update   # maintainer mode: refresh snapshots
#
# The Registrar admission rule enforced through this gate (no new reader
# methods, contribution methods only) is documented in docs/REGISTRAR-RULES.md.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SNAP_DIR="$ROOT/scripts/public-surface"
SCANNER="$ROOT/scripts/public-surface.py"

CRATES=(
    "alva-kernel-abi"
    "alva-agent-core"
)

check_crate() {
    local crate="$1"
    local snapshot="$SNAP_DIR/$crate.txt"
    local actual
    actual="$(python3 "$SCANNER" "$ROOT/crates/$crate")"
    if [[ ! -f "$snapshot" ]]; then
        echo "public-surface: MISSING snapshot for $crate" >&2
        echo "  expected: $snapshot" >&2
        echo "  create it with: ./scripts/public-surface.sh --update" >&2
        return 1
    fi
    if ! diff -u "$snapshot" <(printf '%s\n' "$actual") > /tmp/public-surface-diff.$$ 2>&1; then
        echo "public-surface: DRIFT in $crate — the public symbol surface changed" >&2
        echo >&2
        cat /tmp/public-surface-diff.$$ >&2
        rm -f /tmp/public-surface-diff.$$
        echo >&2
        echo "This gate exists so every kernel/SDK boundary change is conscious." >&2
        echo "If the change is intentional, commit it with the refreshed snapshot:" >&2
        echo "  ./scripts/public-surface.sh --update && git add $snapshot" >&2
        return 1
    fi
    echo "public-surface: $crate OK"
}

update_crate() {
    local crate="$1"
    mkdir -p "$SNAP_DIR"
    python3 "$SCANNER" "$ROOT/crates/$crate" > "$SNAP_DIR/$crate.txt"
    echo "public-surface: updated $SNAP_DIR/$crate.txt"
}

mode="${1:---check}"
rc=0
case "$mode" in
    --check)
        for crate in "${CRATES[@]}"; do
            check_crate "$crate" || rc=1
        done
        ;;
    --update)
        for crate in "${CRATES[@]}"; do
            update_crate "$crate"
        done
        ;;
    *)
        echo "usage: $0 [--check|--update]" >&2
        exit 2
        ;;
esac
exit "$rc"
