# Core review probes
> Independent read-only behavior probes for R25.
## 地位
Scratch executable harness; no production changes.
## 逻辑
Run each binary against workspace crates through path dependencies.
## 约束
No production edits; panics indicate unmet reviewed contracts.
## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Drop probe | src/main.rs | Assert driver task reclaimed |
| Reentrant probe | src/bin/reentrant.rs | Bound non-Tokio reentrant append |
