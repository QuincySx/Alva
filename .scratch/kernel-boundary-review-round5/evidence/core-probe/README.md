# R25 independent probes

Sources are copied from `.scratch/review-core-probe`, an isolated Cargo package depending on the checked-out production crates. No production files were changed.

- `drop.rs`: consume a real live event, let the driver park, call close and drop the stream/session, yield 100 times, assert Tokio `num_alive_tasks()==0`.
- `reentrant.rs`: run a listener that appends once to its own session on `futures_executor::block_on` without Tokio runtime; require outer append to complete within one second via thread/channel gate.

Reproduce:

```sh
cargo run --offline --manifest-path .scratch/review-core-probe/Cargo.toml --target-dir target --bin review-core-probe
cargo run --offline --manifest-path .scratch/review-core-probe/Cargo.toml --target-dir target --bin reentrant
```

Production locations: `agent_session.rs:563` takes a non-reentrant delivery lock; `:713` waits for Notify with no close/drop wakeup path.

## Results

- Broadcast regression: 12/12 PASS; subscription boundary: 1/1 PASS (`broadcast-result.txt`).
- R24 real nested durable chain: 1/1 PASS; deterministic swap gate: 1/1 PASS (`r24-result.txt`).
- R25 disposal: FAIL, 1 driver task survives close/drop (`drop-result.txt`).
- R25 non-Tokio reentrant append: FAIL, one-second completion bound expires (`reentrant-result.txt`).

The independent harness uses cached dependencies with a separate lockfile. A discarded direct-rustc attempt encountered an existing Rust 1.93 futures-executor artifact under Rust 1.98.1; this was a build-environment mismatch, not a behavioral result. The reported reentrant failure comes from the successful Cargo-built probe.
