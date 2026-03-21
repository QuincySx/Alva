# adapters
> DDD 适配器层 —— ports 接口的具体实现

## 地位
在 `srow-core` 的六边形架构中，adapters 是 ports 的出站适配器，提供 LLM API 调用和会话存储的具体实现。

## 逻辑
`llm/` 子目录提供 LLMProvider 实现（OpenAI 兼容），`storage/` 子目录提供 SessionStorage 实现（内存 + SQLite 再导出）。

## 约束
- 每个适配器必须实现对应的 port trait
- 适配器可引入外部 crate（如 rig-core、tokio-rusqlite）

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| mod | `mod.rs` | 声明 llm、storage 子模块 |
| llm/ | `llm/` | LLM 提供商适配器（OpenAI 兼容） |
| storage/ | `storage/` | 会话存储适配器（内存 + SQLite） |
