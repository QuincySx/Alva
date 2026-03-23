# agent-types
> Agent 系统的基础类型定义层，提供 Message、Tool、LanguageModel 等核心抽象

## 地位
整个 agent 体系的类型基石。所有上层 crate（agent-core、agent-graph、srow-core）均依赖此 crate 获取统一的消息、工具、模型抽象。不包含任何业务逻辑，仅定义数据结构与 trait 接口。

## 逻辑
```
Message ←── ContentBlock (text / tool_use / tool_result / image)
   │
   ├─→ LanguageModel::generate(messages, tools) → StreamEvent 流
   │
   └─→ Tool::execute(ToolCall) → ToolResult
```
- `LanguageModel` trait 定义统一的 LLM 调用接口，返回 `StreamEvent` 异步流
- `Tool` trait 定义工具执行接口（签名含 `&dyn ToolContext`），`ToolRegistry` 管理工具集合
- `ToolContext` trait 提供运行时上下文（workspace、session_id、allow_dangerous），应用层实现具体类型
- `Provider` trait 定义 LLM 提供商接口，提供 language_model 及 7 种模型能力方法
- `CancellationToken` 提供协作式取消机制
- `AgentError` / `ProviderError` 统一错误类型

## 约束
- 纯类型 crate，禁止包含运行时逻辑或副作用
- 所有公开类型必须实现 `Send + Sync`
- `Tool` trait 为 `async_trait`，实现者必须是线程安全的

## 业务域清单
| 名称 | 文件 | 职责 |
|------|------|------|
| 消息 | `message.rs` | Message、MessageRole、UsageMetadata |
| 内容块 | `content.rs` | ContentBlock 枚举（text / tool_use / tool_result / image） |
| 工具 | `tool.rs` | Tool trait、ToolCall、ToolResult、ToolContext trait、ToolDefinition、ToolRegistry |
| 模型 | `model.rs` | LanguageModel trait、ModelConfig |
| 提供商 | `provider.rs` | Provider trait、ProviderError |
| 流事件 | `stream.rs` | StreamEvent（LLM 流式输出事件） |
| 取消令牌 | `cancel.rs` | CancellationToken（协作式取消） |
| 错误 | `error.rs` | AgentError 统一错误枚举 |
