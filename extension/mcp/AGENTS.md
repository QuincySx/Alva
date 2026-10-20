# L4 · Managed MCP
> Lazy approved native stdio capabilities behind a single session-owned gateway.

## 地位
Optional harness Plugin, above the protocol worker and native fact authority; not an SDK policy.
## 逻辑
New copies an explicit approved snapshot without I/O. Run start binds the actual native repository authority and run; matching ID strings or memory sessions do not grant worker access. Gateway requires the exact registered Tool, name and nonempty CallID before approval. After HITL, gateway discovery/call lazily connects selected servers; continuous protocol I/O is owned by integration/protocol/mcp. Every complete redacted frame is retained in bounded memory and the same session SQLite typed-record BLOBs. Model output is a small record/artifact reference, retrievable by bounded literal text tools.
Explicit stderr/invalid/incomplete I/O envelopes use native RedactStreamFrame before either cache or commit, including known credential JSON keys inside complete/partial/binary lines. This does not decode arbitrary MCP business base64. The protocol owns full-line/EOF framing and hard overflow stop; the journal owns sanitization and truthful processing receipts.
## 约束
No config/credential discovery, remote schema fan-out, second AI, JSON-path extraction, automatic retry/replay or inherited parent log access. All gateway actions require Guard approval, regardless of server annotations; unknown tool middleware may additionally request approval. At most 32 servers, 256 KiB gateway input, 60s discovery, aggregate catalog 32 MiB/8192 tools, 4096 retained frames/64 MiB per plugin lifetime; failure latches and stops workers. Run end/Close cancels and joins approval/gateway work before releasing native facts, then reaps owned processes. CleanupComplete independently covers the gateway gate and all retained workers; a reaped worker with an observer error is released while the strong error remains retained. Failed physical cleanup keeps its exact worker for retry. Read/search logs never starts MCP. Raw user uploads are unchanged; journal sanitizes known secret fields/markers, not arbitrary prose. RebindChild requires actual direct-child native metadata and creates a fresh lazy worker/journal; app factory separately verifies the exact actual direct-parent gateway grant. Copying a parent gateway cannot authorize a child, and no ancestor journal URI is readable through child references.
存储只消费本包按需组合的共享storage能力插口；会话/引用/DTO/错误从同一契约返回，不导入具体session.Repository或SQL实现。后端由上层程序/插件装配选择，组件不回落默认库、不downcast实现，不持有存储Close。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Recipe/lifecycle | plugin.go | Explicit config, actual run binding, lazy worker ownership and cleanup |
| Gateway | gateway.go | Bounded discovery/exact invocation, HITL, cancellation and reference receipts |
| Journal | journal.go | Full redacted memory/durable frames and fail-closed budgets |
| Boundary regressions | boundary_test.go | Same-ID foreign log/copied Tool rejection, strict bounded JSON, cancellation/joined approval and retained error versus physical cleanup |
| Regressions | plugin_test.go | Real owned stdio, split/binary stderr sanitization with safe fields preserved, HITL, cache/native equality, restart reads and cancel/no-replay |

Process journal sanitization includes immutable explicit configured Env values (unlabelled/JSON-escaped/multiline echoes), scoped to the exact MCP server or AEP plugin; no ambient environment discovery. Truncated captured bodies are omitted rather than retaining partial credentials.
