# types
> 共享类型定义的扁平导入入口

## 地位
在 `srow-core` crate 中提供便捷的类型再导出路径，让外部使用者无需深入 `domain/` 或 `ports/` 即可引用 LLM 和 ACP 消息类型。

## 逻辑
`mod.rs` 声明两个子模块；`llm.rs` 将 `domain::message` 和 `ports::llm_provider` 中的核心类型扁平再导出；`acp_message.rs` 将 `agent::agent_client::protocol` 中的 ACP 消息类型扁平再导出。

## 约束
- 本模块不定义新类型，仅做 `pub use` 再导出
- 新增类型应在 `domain/` 或 `ports/` 中定义后在此再导出

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| mod | `mod.rs` | 声明子模块 llm, acp_message |
| llm | `llm.rs` | 再导出 LLM 请求/响应/流式/Token 等类型 |
| acp_message | `acp_message.rs` | 再导出 ACP 协议消息类型 |
