# agent/agent_client/storage
> ACP 消息持久化

## 地位
为 ACP 消息提供持久化存储接口，当前为内存 Vec 实现，未来迁移至 SQLite。

## 逻辑
`AcpMessageStorage` 使用 `Mutex<Vec<StoredMessage>>` 记录消息，支持按 conversation_id 查询。

## 约束
- Phase 1：内存实现，无持久化
- Phase 2：将使用 tokio-rusqlite `acp_messages` 表

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| mod | `mod.rs` | 声明 sqlite 子模块 |
| sqlite | `sqlite.rs` | AcpMessageStorage：内存消息存储（占位命名） |
