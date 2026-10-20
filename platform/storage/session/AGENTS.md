# 横向支撑 · session
> Native execution facts and artifacts with one SQLite authority and bounded read surfaces.

## 地位
Native implementation of the shared storage.Repository/Session ports and SDK log contracts; separate from Hub product state. Public DTOs/errors live in the contract root, with no old aliases.
## 逻辑
Typed records are atomically validated and indexed; their complete JSON bytes live in session-owned hashed SQLite chunks. Non-record artifacts use inline bytes or explicit OpenWithBlobs. The same owner/session metadata, chunk/full hashes, paging and search apply to both. Metadata pages do not fetch object bytes or restore runtime. Recovery records interrupted model/tool outcomes without executing tools and marks unknown side effects explicitly.
Native schema v2 adds a small child→original-link index in the same CreateChild transaction. Existing new-Go v1 databases receive only that empty index; no historical relation is inferred/backfilled or silently made an authorization grant. ParentLink revalidates the original ≤16KiB typed link, causal intent SQL metadata and same-owner direct-parent metadata without reading intent/conversation bodies. Product code still validates its claimed root and bounds ancestry hops.
Native schema v3 adds optional blob namespace binding and artifact→object identities. Existing new-Go v1/v2 facts/bytes remain unchanged; opening does not move data. Configured ID/retention must match on every reopen; missing/substituted adapters fail without local fallback. Host-supplied ID is a non-secret deployment identity, not proof that an arbitrary provider is trustworthy. External publication precedes the metadata transaction: errors can leave orphan bytes, never partial committed references. No automatic retry/delete or cross-store transaction is claimed. Volatile references and harness prompts state process-lifetime retention; execution facts stay durable.
Full artifact reads/search amortize remote IO in bounded64KiB per-operation windows while verifying every consumed4KiB catalog hash; page reads keep normal chunk boundaries. No global/shared cache changes ownership or visibility.
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
| Bounded conversation reads | `conversation_tail.go` / `conversation_tail_test.go` | owner/session固定高水位、独立metadata与hash/protocol验证；最多512条/8MiB恢复硬上限；近期turn选择及省略提示在harness |
| Explicit Inspector export | export.go / export_test.go | 固定各session高水位的versioned redacted JSONL，8MiB/2048 record/32session/8hop，typed link与causal index双验证；超限/坏hash无partial success，不取外部artifact或恢复runtime |
| Typed stream snapshots | record_queries.go / record_queries_test.go | Exact latest metadata without unrelated bodies, corrupt-target errors and atomic optimistic extension append against the actual prior sequence |
| Artifacts | artifacts.go | Same-authority metadata/hashes, inline or external immutable bytes, opaque references and bounded text reads |
| External bytes | blob_storage.go / blob_storage_test.go | Explicit borrowed adapter/binding/retention, verified chunks, real memory/directory acceptance, isolation, corruption and uncertain upload rollback |
| Cold artifact queries | artifact_queries.go / artifact_queries_test.go | Bounded exact kind/name metadata pages with fixed row high-water and explicit full hash-verified binary reads |
| Recovery | recovery.go | Unknown/cancelled interruption settlement without replay |
| Redaction policy | ../redaction/ | Shared pure known-credential policy; no concrete store dependency for consumers |
| Child facts | child.go | Atomic actual child creation/parent intent link and metadata-only ancestry lookup |
| Child inspection | child_read.go | Exact bounded typed-link/hash and intent-index validation without restoring runtime or parent bodies |
| Child regressions | child_test.go | Causal intent requirements, atomic identity/link, owner isolation and no inherited artifact access |
| Regressions | repository_test.go | Atomicity, reopen, corruption, isolation, bounded reads and recovery |
| Backend conformance | contract_test.go | SDK共用原子性、身份、游标与取消验收 |
| Runtime boundaries | loop_contract_test.go | 85调用prepare失败结算、4097消息reopen/压缩与字节/坏metadata拒绝 |
| Privacy/upload regressions | redaction_test.go | Canonical receipts, framed text/binary/incomplete markers, safe-field/idempotency preservation, immutable originals and upload limits |

BoundConversationTail返回固定高水位的私有metadata snapshot及有界Read；不会选择模型历史或生成提示词。conversation专用partial index在Open时增建；原facts不变。返回metadata是独立copy，Read只恢复该snapshot选择的数量并校验hash/身份/tool协议。harness选择近期完整turn及真实省略引用，不回退全量Messages。
AppendExtensionSnapshot可在同一CAS事务中追加其他typed records，用于耐久结果消息与状态原子提交；不允许额外记录覆盖被比较的同一snapshot stream。持久等待的schema/状态/审批期限仍属于harness/durable，storage不识别业务状态。
