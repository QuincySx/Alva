# ADR-0001: ALVA Hub V1 Is Desktop-first

- Status: Accepted
- Date: 2026-07-31
- Scope: Product surface and crate boundary

## Context

The repository currently has three distinct assets:

- a mature native `alva-app-cli` that directly drives `BaseAgent`;
- an Alva-only `alva-app-tauri` desktop product;
- a newer multi-engine Hub path with `alva-hub-core`, three adapters, and
  `alva-hub-cli`.

The Hub CLI now proves one-shot, REPL, resume, manual switch, handoff, and
permission behavior. The product direction, however, requires visible branch
state, fallback controls, collaboration Workers, engine health repair, and
permission aggregation. These interactions are not well represented as the
primary terminal UX.

Rule 18 prevents native `alva-app-cli` and `alva-app-tauri` from depending on
`alva-engine-*`. Adding EngineRuntime to the existing Tauri product would erase
an intentional blast-radius boundary.

## Decision

1. ALVA Hub V1 is Desktop-first.
2. Add a separate L7 `alva-hub-desktop` entrypoint above `alva-hub-core`.
3. Use Tauri 2 with React/Vite/TypeScript.
4. Keep `alva-hub-cli` as the auxiliary automation, diagnostics, and contract
   validation surface.
5. Keep `alva-app-tauri` as an independently maintained Alva-only Desktop.
6. Keep `alva-app-cli` as the native Alva product and SDK/harness regression
   reference.
7. Reuse existing Desktop presentation components only after a concrete shared
   seam is proven. Do not make the new Rust backend or database depend on
   `alva-app-tauri`.
8. V1 is local-first and requires no ALVA account.

## Consequences

### Positive

- Multi-engine product changes remain above native products and preserve Rule
  18.
- Hub can design its storage and UI around product sessions, branches,
  attempts, and Workers instead of inheriting Alva-only assumptions.
- A visual timeline can make automatic fallback visible and reversible.
- The Hub CLI remains small enough to expose contract failures without Desktop
  state.

### Negative

- Two Desktop products are maintained in parallel.
- Shared UI fixes may need duplication until a stable presentation-only package
  emerges.
- Packaging, signing, updater, and runtime discovery work exists for another
  product.
- The existing `alva-app-tauri` SQLite schema cannot be reused directly.

## Alternatives Rejected

### Make native Alva CLI the main Hub product

Rejected because the accepted V1 interaction model is visual and stateful:
fallback cards, parked branches, collaboration Workers, health repair, and
aggregated approvals.

### Put EngineRuntime into `alva-app-tauri`

Rejected because it violates Rule 18 and would couple the Alva-native product to
every multi-engine contract change.

### Replace and delete `alva-app-tauri`

Rejected. The accepted direction is parallel maintenance while Hub Desktop is
the active product investment.

### Use Electron because Cindy uses Electron

Rejected. Cindy is the product/interaction reference, not a mandate to replace
the repository's established Tauri + React stack.

## Verification

- Dependency CI continues to enforce Rule 18.
- `alva-hub-desktop` depends on Hub/engine product layers, not vice versa.
- `alva-app-tauri` and `alva-app-cli` remain free of transitive
  `alva-engine-*` dependencies.
- Hub Desktop uses a separate local database and product identity namespace.
