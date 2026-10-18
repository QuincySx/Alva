# Hub acceptance evidence, 2026-09-18

Production baseline: `bae66d0f`; production source unchanged by these probes.

Executed both commands successfully through compilation; each then failed its acceptance assertion (exit 101):

```sh
cargo run --offline --target-dir target --manifest-path .scratch/kernel-boundary-review-round9/evidence/hub/Cargo.toml --bin cancel_claim_gap
cargo run --offline --target-dir target --manifest-path .scratch/kernel-boundary-review-round9/evidence/hub/Cargo.toml --bin handoff_native
```

R80 actual output:

```text
new claim after stale cancel: Failed; expected Starting
assertion `left == right` failed: old RunGeneration must not release a newer session claim before its RunHandle registers
  left: Failed
 right: Starting
```

R90 actual output:

```text
retry marker count=1, retry native=Some("native-0"); expected None as after EOF rollback
failed first-handoff native must not be resumed merely because EOF is late
```

R80 gate fixes the exact sequence: old fatal stream remains open; old cancel awaits Unsupported; new execute has claimed Starting but blocks synchronously before registering its RunHandle; old cancel resumes. `abandon_targeted` checks the old runs generation but mutates the newer session claim.

R90 observes the real retry request after SessionStarted + fatal, while EOF remains pending. The context marker is preserved, but the failed native identity is resumed. At EOF, `restore_failed_handoff` would instead restore the pre-switch-attempt native identity (None). This covers the card's explicit failed-native contract requirement, not Desktop multi-branch semantics.

Limits: isolated fake adapters against actual current HubRuntime; no live external CLI, Desktop, HTTP, or full suite run. Gates are released before the R80 assertion so its expected failure does not strand a blocking worker.
