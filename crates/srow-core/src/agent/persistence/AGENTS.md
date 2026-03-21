# agent/persistence
> Agent 会话持久化 —— SQLite 存储层

## 地位
提供基于 SQLite（WAL 模式）的 `SessionStorage` trait 实现，用于会话和消息的持久化存储。

## 逻辑
`schema.rs` 定义 DDL（sessions、messages、acp_messages、schema_version），`migrations.rs` 管理版本迁移，`SqliteStorage` 实现完整的 `SessionStorage` trait。

## 约束
- 使用 tokio-rusqlite 进行异步 SQLite 操作
- WAL 模式提升并发读写性能
- 消息内容以 JSON 文本存储

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| mod | `mod.rs` | 声明子模块、pub use SqliteStorage |
| schema | `schema.rs` | DDL 常量：CREATE_SESSIONS、CREATE_MESSAGES 等 |
| migrations | `migrations.rs` | run_migrations：版本检查与 DDL 执行 |
| sqlite | `sqlite.rs` | SqliteStorage：SessionStorage trait 的 SQLite 实现 |
