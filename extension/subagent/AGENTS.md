# L4 · SubAgent
> Explicit SDK-backed native child delegation, distinct from a Hub WorkerSession.

## 地位
Optional harness extension over generic SDK scope/communication contracts; factory owns actual native sessions and capability rebinding.
## 逻辑
Each actual root run binds a scope to its true session ID. The agent Tool validates exact direct-parent grants and optional approved templates/models/comms, then applies a recursive tree admission budget and child wall-clock deadline before factory setup. Factory must atomically commit actual child/link before execution. Child executes inline; model streams share a bounded semaphore without locking waiting coordinators. Parent receives only its own retained selected final/partial output reference.
An optional host GrantCatalog narrows delegation to actual capabilities that have explicit binders; own child context/history readers are independently installed and cannot be used to grant parent-history access. Factory返回的任何Child.Close即转移cleanup ownership，即使同时返回error；已自行清理的Factory必须不再返回closer。部分setup error仍会关闭一次，保留原error和强cleanup error。gateway要求实际注册Tool/nonempty call和严格JSON，返回引用先Validate。Factory setup, SDK execution and resource cleanup have separate durable lifecycle receipts. Refusal remains model-recoverable and only selected text/refusal is delivered; other typed output stays in the child's records.
## 约束
No parent/root tool or Bus copying, parent/ancestor/sibling history reads, ambient provider config, automatic replay, fake Hub Worker or detached child termination claims. Factory rebinding is explicit and must reject unknown contributed tools. Empty grant means no host tools; own child context readers and recursive agent are infrastructure, not parent-history grants. Agent is Coordinator/ManagesOwnTimeout; resource locks belong to actual child tools. Limits ≤8 depth, 128 total admissions, 16 concurrent child model streams, 30min child timeout, 256 iterations, 1MiB selected output. Child completion waits actual Run/cleanup; a pure own timeout is recoverable only when parent remains active and every error leaf is cancellation. Typed session, commit/close/flush and callback failures remain fatal. Constructor/registration performs no child/model I/O.
Typed-nil factory/reference/model/template capabilities fail at construction. Attach handles reject missing/typed-nil context hooks and typed-nil completion callbacks before Factory.Prepare; ordinary nil optional completion remains valid. Resolver/factory function adapters return errors for nil functions; typed-nil resolved models or child Runner/Session never reach execution, and invalid prepared children still close their resources.
Exact grant identity checks both dynamic values for comparability; map/slice data inside interface fields cannot prove parent capability identity or panic during comparison.
## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Contracts/budget | contracts.go | Narrow factory/runner, explicit model/template profiles and shared tree/model budgets |
| Plugin/lifecycle | plugin.go | Actual root/child scope binding and optional recipe |
| Agent Tool | tool.go | Exact grants/comms, bounded inline child lifecycle and parent-owned result reference |
| Partial setup ownership | boundary_test.go | ChildFactory同时返回error/closer仍真实Close且不执行child |
| Regressions | plugin_test.go | Inline cancel/exit, typed settlement errors, close/callback failure, refusal, exact grants and model concurrency |
| Missing capabilities | nil_test.go | Constructor/Attach fail-fast before child IO, invalid prepared child cleanup and nil function adapters |
| Grant identity | identity_test.go | Dynamically noncomparable parent values do not establish grants or panic |
