# 横向支撑 · session
> Native execution facts and artifacts with one SQLite authority and bounded read surfaces.

## 地位
Native implementation of kernel ConversationLog/SessionStore; separate from Hub product state, not an SDK dependency.
## 逻辑
Typed records are atomically validated and indexed; their complete JSON bytes live in session-owned hashed BLOB chunks. Metadata pages do not restore runtime or read record bodies. Explicit record/artifact reads verify bounded chunk hashes; runtime restores verify full hashes. Recovery records interrupted model/tool outcomes without executing tools and marks unknown side effects explicitly.
Native schema v2 adds a small child→original-link index in the same CreateChild transaction. Existing new-Go v1 databases receive only that empty index; no historical relation is inferred/backfilled or silently made an authorization grant. ParentLink revalidates the original ≤16KiB typed link, causal intent SQL metadata and same-owner direct-parent metadata without reading intent/conversation bodies. Product code still validates its claimed root and bounds ancestry hops.
## 约束
Committed typed records share a canonical known-credential-field/inline-marker redaction policy; arbitrary prose secrets are not magically detected, and explicitly uploaded raw bytes remain original artifacts.
RedactJSON/RedactStreamFrame accept an explicit immutable EnvironmentSecrets snapshot: known credential keys include short values, other configured overrides ≥6 bytes are private too; ambient environment is never read. Longest literal/JSON-escaped/multiline components are checked in one replacement pass, bounded to512 patterns with fail-closed excess. Truncated frame bodies, and incomplete lines when explicit configured values exist, are omitted entirely rather than retaining a configured-credential prefix. RedactStreamFrame is only for explicit protocol text/base64 stream envelopes. It validates oneof/bounds, decodes only that declared body and sanitizes complete JSON fields plus known raw/incomplete JSON key markers, including invalid UTF8. Complete JSON embedded in journal text is field-sanitized idempotently so a later native append never erases already-safe fields. Ordinary bytes remain exact; redaction/encoding/processing is explicit. Upstream must frame complete bounded lines rather than split Read chunks; unrelated business/artifact base64 is never recursively decoded.
Owner/session identifiers are opaque, not paths. Owner is injected by trusted product assembly, never accepted as authorization from UI input. All queries bind both owner and native session; URI/hash knowledge grants nothing. Process lock and in-process run claims prevent recovery of live execution. No legacy import, credentials, automatic network or side-effect replay. File uploads ≤20 MiB retain original bytes/hash and truthful processing metadata. Bounded metadata/search/read APIs are the Inspector surface. Records remains a full runtime read capped at4096 records; Messages retains the complete conversation within64MiB stored bytes plus a512-byte per-record overhead allowance, without the Inspector record-count ceiling. Metadata is preflighted before any body restore; missing metadata, corruption or excess fails without a partial transcript. This is not a persistent compacted view or an exact Go heap limit. Native batch admission permits2+3*wire.MaxStreamContentBlocks records and64MiB encoded/original-raw-buffer input before deep clone/redaction. These are native backend policies, not added kernel requirements or restrictions on SDK MemorySession. The additional32MiB per-record limit and redaction remain explicit native behavior.
## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Repository | repository.go | Private DB lifecycle, owner-bound session creation/open/list and run claims |
| Durable facts | records.go | Atomic typed append, runtime reads and bounded metadata/search |
| Native admission | budget.go / budget_test.go | 后端私有条数/encoded/raw输入预算与原子拒绝；不扩充SDK契约 |
| Bounded working view | `context_source.go` / `context_source_test.go` | owner/session绑定、固定高水位的近期user-turn尾视图；64条目标/512条及8MiB硬上限，协议组保全，省略给真实引用而非假摘要；不调用Messages或修改原facts |
| Explicit Inspector export | export.go / export_test.go | 固定各session高水位的versioned redacted JSONL，8MiB/2048 record/32session/8hop，typed link与causal index双验证；超限/坏hash无partial success，不取外部artifact或恢复runtime |
| Typed stream snapshots | record_queries.go / record_queries_test.go | Exact latest metadata without unrelated bodies, corrupt-target errors and atomic optimistic extension append against the actual prior sequence |
| Artifacts | artifacts.go | Single-authority hashed chunks, opaque references and bounded text reads |
| Cold artifact queries | artifact_queries.go / artifact_queries_test.go | Bounded exact kind/name metadata pages with fixed row high-water and explicit full hash-verified binary reads |
| Recovery | recovery.go | Unknown/cancelled interruption settlement without replay |
| Redaction | redaction.go | Canonical known credential redaction shared by native facts and MCP memory |
| Child facts | child.go | Atomic actual child creation/parent intent link and metadata-only ancestry lookup |
| Child inspection | child_read.go | Exact bounded typed-link/hash and intent-index validation without restoring runtime or parent bodies |
| Child regressions | child_test.go | Causal intent requirements, atomic identity/link, owner isolation and no inherited artifact access |
| Regressions | repository_test.go | Atomicity, reopen, corruption, isolation, bounded reads and recovery |
| Backend conformance | contract_test.go | SDK共用原子性、身份、游标与取消验收 |
| Runtime boundaries | loop_contract_test.go | 85调用prepare失败结算、4097消息reopen/压缩与字节/坏metadata拒绝 |
| Privacy/upload regressions | redaction_test.go | Canonical receipts, framed text/binary/incomplete markers, safe-field/idempotency preservation, immutable originals and upload limits |

BoundContextSource是native-context-source Plugin独立装配的能力，不覆盖ContextSystem。Open为新建/已有Go repository增建conversation专用partial index，不修改原facts或存储格式；Source按该索引读取最新最多513条conversation元数据，依据完整user-turn与字节预算选择近期尾段后才hash恢复正文。较早turn可省略，最新turn超限/坏元数据/坏hash/孤立tool result均显式失败，无全读回退。省略notice说明高水位及最后省略record的真实引用，不声称做过摘要或媒体理解。完整日志/Inspector仍保持既有语义。
