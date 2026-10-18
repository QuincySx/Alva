# Adapter evidence
> Standalone local Pi compaction recovery reproduction.

## Scope
No production edits, no real Pi models. Temp scripts emit public wire events and are removed by TempDir.

## Files
| File | Purpose |
|---|---|
| Cargo.toml | Standalone workspace adapter consumer |
| main.rs | Healthy and compaction-recovery controlled execute samples |

## Validation
Root reviewer runs cargo with shared target scheduling and records actual result.
