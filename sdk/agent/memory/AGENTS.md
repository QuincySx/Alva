# L3 · sdk/agent/memory
> 可插拔 FTS + vector 混合检索能力，默认进程内存后端。

## 地位
SDK 能力库；标准库限定、WASM clean，不扫描工作区或持有 SQLite/HTTP 依赖。

## 逻辑
Backend 分离文件/块 CRUD、全文/向量检索及 embedding cache。Service 在存储前计算 hash/embedding，
用 FTS 与真实非空 vector 结果做 0.4/0.6 加权融合；embedding 失败或 Noop 时保留 FTS-only，不凭空制造语义相关性。
同一 Service 的写入串行，backend 对引用数据深拷贝；排序使用 chunk ID 稳定打破平分。
Search开始时固定一份权重；零权重真正停用该modality，不发起对应FTS/embedding/vector调用或混入零分候选。拒绝权重总和overflow；配置变更从下次Search生效。

## 约束
默认数据只活在进程；持久 backend、MEMORY.md 工作区扫描、网络 embedding Provider 在宿主扩展。
StoreEntry优先可选FileReplacer原子替换(file+全chunks)；InMemory单锁实现，宿主SQLite用事务实现。
只有Backend CRUD时仍为非原子fallback，file hash最后更新，不能把跨调用事务原子性冒充已有。
embedding cache可先成功，它不代表file revision提交；transaction失败不能再悄悄执行非原子fallback。
Service在入口、等待写锁后及外置检索/存储阶段之间检查取消；取消后不进入下个索引阶段。已经完成的backend写入不自动撤回，CRUD fallback仍可能部分提交；backend须负责自身原子性与执行中的context响应。
默认 MemoryPlugin 在 harness，可选业务插件在 extension/memory；不保留临时 app KV MemoryService 的 Put/Get 兼容入口。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Memory capability | `memory.go` | Backend/types、内存 CRUD/FTS/cosine、Service hybrid search |
| Regression | `memory_test.go` | Noop/fallback 不造相关性、替换、vector 所有权与混合排序 |
| Atomic regression | `replace_test.go` | 完整批验证/所有权、事务优先、失败不checkpoint新hash |
| Service policies | `service_test.go` | 零权重停用真实调用、单次权重snapshot、overflow与阶段间取消 |
