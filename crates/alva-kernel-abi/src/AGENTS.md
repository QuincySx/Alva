# alva-kernel-abi
> Agent 系统的基础类型定义层，提供 Message、Tool、LanguageModel 等核心抽象

## 地位
整个 agent 体系的类型基石。所有上层 crate（alva-kernel-core、alva-agent-graph、alva-app-core）均依赖此 crate 获取统一的消息、工具、模型抽象。不包含任何业务逻辑，仅定义数据结构与 trait 接口。

## 逻辑
```
Message ←── ContentBlock (text / tool_use / tool_result / image)
   │
   ├─→ LanguageModel::generate(messages, tools) → StreamEvent 流
   │
   └─→ Tool::execute(ToolCall) → ToolResult
```
- `LanguageModel` trait 定义统一的 LLM 调用接口，返回 `StreamEvent` 异步流
- `Tool` trait 定义工具执行接口（签名含 `&dyn ToolContext`），`ToolRegistry` 以首次注册顺序管理工具并显式报告同名替换
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
| 消息 | `base/message.rs` | Message、MessageRole、UsageMetadata |
| 内容块 | `base/content.rs` | ContentBlock 枚举（text / tool_use / tool_result / image） |
| 工具 | `tool/` | Tool trait、ToolCall、ToolResult、ToolContext trait、ToolDefinition、ToolRegistry；`scheduler.rs` 持有 Scheduling 调度描述符类型，`schema.rs` 提供依赖运行时状态的 JSON Schema 覆写 |
| 模型 | `model/` | LanguageModel trait；`config.rs` 持有 ModelConfig |
| 流事件 | `base/stream.rs` | StreamEvent（LLM 流式输出事件） |
| 取消令牌 | `base/cancel.rs` | CancellationToken（协作式取消） |
| 错误 | `base/error.rs` | AgentError 统一错误枚举 |
| 会话契约 | `agent_session.rs` | AgentSession、ScopedSession、事件查询/订阅与 atomic typed-record capability 声明；`PersistentSessions` 是内核循环与工具路径唯一认识的会话能力（以 `Arc<dyn PersistentSessions>` 注册/下发）；`StoreSessionCell` 是装配层包装具体 typed store 的 cell，内核代码不识别它 |
| 会话存储契约 | `session_store.rs` | SessionStore —— harness/UI 超集需求：查询、计数、回滚、快照、订阅、压缩、持久化、子会话派生 |
| 对话记录契约 | `conversation_log.rs` | ConversationLog —— 内核对"历史"的全部要求（读/追加消息）；SessionStore 是其 harness 超集 |
| Durable records | `session_record.rs` | 版本化 SessionRecord、typed built-in/extension payload、blob 引用与 append receipt |
| 上下文生命周期 | `scope/context/` | ContextHooks 七钩子、ContextHandle、ContextSystem 与 ContextApply 执行器契约 |
| Spawn Comm | `scope/spawn/communication.rs` | SpawnCommunication trait + SpawnCommContext + SpawnCommHandle + OnChildComplete + SpawnResult + SpawnCommunicationRegistry —— sub-agent 通信能力的插件契约 |
| Runtime 原语 | `runtime.rs` | Sleeper + timeout（内核不依赖 tokio::time） |
| 常量 | `constants.rs` | 工具上限、agent 配置等共享常量 |
