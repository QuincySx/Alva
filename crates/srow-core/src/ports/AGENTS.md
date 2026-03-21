# ports
> DDD 端口层 —— 定义核心抽象接口（trait）

## 地位
在 `srow-core` 的六边形架构中，ports 定义驱动端口（Tool、LLMProvider）和被驱动端口（SessionStorage），是 engine 与外部世界的契约边界。

## 逻辑
engine 通过 `LLMProvider` trait 调用 LLM，通过 `Tool` trait 执行工具，通过 `SessionStorage` trait 持久化会话。`ToolRegistry` 提供按名称查找和定义列表。

## 约束
- 仅包含 trait 定义和最简数据结构（ToolContext、ToolRegistry）
- 所有 trait 必须 `Send + Sync` 且标记 `#[async_trait]`
- 不得引入具体实现

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| mod | `mod.rs` | 声明子模块 |
| tool | `tool.rs` | Tool trait、ToolContext、ToolRegistry |
| llm_provider | `llm_provider.rs` | LLMProvider trait、LLMRequest、LLMResponse、StopReason、StreamChunk、TokenUsage |
| storage | `storage.rs` | SessionStorage trait |
