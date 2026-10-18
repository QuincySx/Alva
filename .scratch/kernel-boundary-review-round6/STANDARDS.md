# Round 6 — Standards

Pinned diff: `git diff 5c64af1640ab00b6a64aa3474112b62fe4106036...5290e033585bd44c6c019c2a47661043e05009b7` (16 commits). Read-only production review; only this report and the independent scanner probe were added to scratch.

## Documented-standard violations

1. **P2 — R56 still drops attributed public contracts.** `scripts/public-surface.py:251–257,283–289` matches raw field/variant text without skipping attributes. A `#[cfg(unix)] pub retries: u32→u64` change and attributed enum variant rename produce identical snapshots. This affects real SDK code: `crates/alva-kernel-abi/src/session_record.rs:40–41` has `#[serde(...)] pub agent_message_blob`, absent from its new snapshot. Stacked item attributes also hide entire real enums (`RecordValue`, `ExecutionRecord`, `RecordCategory`): existing `skip_attributes` does not consume intervening whitespace. Root AGENTS public-surface gate promises to detect public surface changes; R56 explicitly aimed to close these blind spots. Fix attribute traversal and retain executable regression fixtures.

2. **P2 — R56 records names but misses breaking signatures.** `scripts/public-surface.py:289,309` emits only variant / associated-type names: `Data(u32)→Data(String)` and `type Item: Send→Sync` remain byte-identical. Separately, `:265–269` treats the `>` of `fn() -> u32` as a generic closer; `Handler(fn() -> u32), Tail→Changed` loses the tail record entirely. Same root gate / R56 contract. Preserve payloads/bounds and use token-aware balancing, with before/after fixtures covering all three changes.

3. **P3 — R46 leaves newly added integration evidence out of the module map.** New `crates/alva-app-core/tests/r48_swap_child_identity.rs:1` has no entry in `crates/alva-app-core/tests/AGENTS.md` (its inventory ends at line 10). FRACTAL-DOCS, third-layer “新增文件”, requires updating directory and parent inventories. R46’s assertion that module navigation matches the implementation is therefore still incomplete. Add the new evidence file to the applicable navigation; do not rewrite unrelated history mechanically.

4. **P3 — R47 retains one literal spacing value.** `crates/alva-hub-desktop/web/src/styles/app.css:1215` is `padding: 1px var(--space-1)`. Web src AGENTS rule 18 makes tokens the sole spacing source, and R47 expressly targets this chat block. Reuse/define an appropriate token; its execution record currently claims all chat spacing literals were migrated.

## Judgement calls

No additional Fowler-smell finding justified a repair card independently of the concrete issues above. Repo constraints override the heuristic baseline. Cosmetic/formatting findings were excluded.

## Evidence and limits

Run `python3 .scratch/kernel-boundary-review-round6/scanner_probe.py`: **six independent public-contract changes MISSED**, all before/after output byte-identical. Probe imports scanner read-only, does not edit SDK source or snapshots. Stacked-attribute handling predates this diff but is an unresolved R56 acceptance gap; the field/variant/associated-type paths and comma splitter are newly introduced here. Tuple fields and macro expansion were already declared unverified by the implementer and are not counted as new findings.

R47's broad token substitutions are otherwise present. R46 correctly retracts the false clear/restore/rollback generation claim and adds ChatTranscript navigation. No new forbidden SDK→app/host or native-entry→engine dependency was visible in this diff; the only Cargo manifest addition is Hub server→Hub core at L7. Full test execution and behavioral review are owned by the parent/spec tracks, not inferred from commit messages here.
