# domain
> DDD 领域模型层 —— 定义 Agent 系统的核心业务实体

## 地位
位于 `alva-app-core` 的领域核心，所有上层模块（engine、tools、adapters）都依赖此处定义的实体类型，但 domain 本身不依赖任何上层模块。

## 逻辑
四个独立的领域实体文件，彼此之间有引用（如 LLMMessage 引用 LLMContent，Session 引用 SessionStatus），但不形成循环。

## 约束
- 仅包含纯数据类型（struct/enum）和工厂方法，禁止 I/O 或异步操作
- 所有类型必须可 Serialize/Deserialize（serde）
- 不得依赖 `ports/` 或 `adapters/`

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| mod | `mod.rs` | 声明子模块 |
| agent | `agent.rs` | AgentConfig、LLMConfig、LLMProviderKind |
| message | `message.rs` | Role、LLMContent、LLMMessage |
| session | `session.rs` | Session、SessionStatus |
| tool | `tool.rs` | ToolCall、ToolResult、ToolDefinition |
