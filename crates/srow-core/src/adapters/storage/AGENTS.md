# adapters/storage
> 存储适配器实现

## 地位
`adapters/storage/` 提供 `ports::SessionStorage` trait 的具体实现，以及便捷再导出。

## 逻辑
`memory.rs` 提供基于 `HashMap + RwLock` 的内存存储实现，用于开发和测试。`mod.rs` 同时再导出 `agent::persistence::SqliteStorage` 以便统一引用。

## 约束
- MemoryStorage 仅适用于开发/测试，无持久化能力
- 生产环境应使用 `SqliteStorage`（位于 `agent::persistence`）

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| mod | `mod.rs` | 声明 memory 子模块，再导出 SqliteStorage |
| memory | `memory.rs` | MemoryStorage：基于 HashMap 的内存会话存储 |
