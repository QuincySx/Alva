# L4 · Memory Plugin
> Explicit memory service replacement, reference-only tools and authorized workspace integration.

## 地位
Optional heavy native extension, not SDK/default backend policy; Name memory replaces the app default by the established same-name contract.
## 逻辑
Plugin provides the actual configured SDK Service and registers real search/store Tools. With an explicit Workspace, sync scans MEMORY.md and extracts structured notes through the existing guarded filesystem Tools. Source bodies are retained in the actual session ReferenceBackend, not automatically sent to the model. Sync uses atomic FileReplacer checkpoints; extraction writes typed YAML frontmatter and a deterministic Markdown index with prior file/index backups.
RebindTool is a host-only recipe for one exact actual parent capability. It preserves only the intentionally shared memory service/namespace and approved workspace while replacing session/files/references; hosts register just the returned Tool and must separately enforce direct-parent native ancestry/grants. It never installs the whole memory tool catalog in a child.
## 约束
No constructor/registration scan, auto extraction, ambient account/config discovery, parent history grant or fake embeddings. Exact dispatched Tool/nonempty call and session/workspace gates plus Guard authorization apply. Typed-nil references reject at construction; arguments reject duplicate keys and invalid Unicode before store/search/file operations. Include Tools() actual instances in the host security recipe to reuse one identical invocation approval. Custom embedding providers are Unknown effect and require host approval. Search ≤20 small references/4 KiB; store/extract content ≤64 KiB; sync ≤2048 nodes/64 files/8 MiB, 50-line chunks and ≤10 depth; no symlinks, hidden/node_modules/target traversal or deleted-file purge. Only explicitly configured single relative memory directory is created. Extraction index reads ≤64 notes/4 MiB; unrecognized notes are counted, not claimed indexed. File/index/database stages are separately atomic and failures report possible partial writes without replay. Backend/query limits stay explicit. Child factories must rebind session-sensitive tools and references; copying parent tools grants no parent access.
## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Plugin recipe | plugin.go | Explicit capabilities, same-name replacement and actual Tool identities |
| Invocation boundaries | boundary_test.go | Typed-nil reference and borrowed/ambiguous invocation denial before memory effects |
| Memory Tools | tools.go | Strict per-action args, HITL, namespace operations and source references |
| Workspace sync | workspace.go | Existing file Tool reuse, bounded retained source reads and atomic indexing |
| Extraction | extract.go | Typed Markdown, deterministic bounded index and recoverable writes |
| Regressions | plugin_test.go | Actual SQLite/reference operations, skipped paths, chunk hashes, Markdown/index backups and same-name recipe |
