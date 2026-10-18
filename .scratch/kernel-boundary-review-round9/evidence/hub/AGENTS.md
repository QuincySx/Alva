# Round 9 Hub probes
> Isolated executable acceptance evidence for R80 and R90.

## 地位
Review artifacts only; dependency paths point at production crates without modifying them.

## 逻辑
Deterministic fake engine gates exercise the real public HubRuntime.

## 约束
Do not count expected assertion failures as passing production validation.

## 业务域清单
| 文件 | 职责 |
|---|---|
| Cargo.toml / Cargo.lock | Standalone cached dependency resolution |
| cancel_claim_gap.rs | R80 stale cancellation during new synchronous startup |
| handoff_native.rs | R90 failed native identity before old EOF |
| RESULTS.md | Commands, observed failures and limits |
