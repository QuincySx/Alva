# alva-agent-core/src/middleware
> Agent 核心中间件子系统：Middleware trait（洋葱模型）、类型安全 Extensions、MiddlewareStack 及内置压缩中间件

## 地位
`alva-agent-core` 的中间件子模块。定义框架级的 `Middleware` trait 和 `MiddlewareStack`，是 Agent 执行管线的横切关注点扩展机制。领域无关，不依赖任何领域 crate。

## 逻辑
1. `mod.rs` 定义三个核心抽象：
   - `Middleware` trait（async，洋葱模型）：提供 on_agent_start / on_agent_end / before_llm_call / after_llm_call / before_tool_call / after_tool_call 等生命周期钩子
   - `Extensions`：基于 `TypeId` 的类型安全键值存储，供中间件间传递数据
   - `MiddlewareStack`：有序中间件栈，按注册顺序依次执行
2. `compression.rs` 实现 `CompressionMiddleware`：当估算 token 数超过阈值时，截断旧消息并插入摘要标记，实现上下文压缩。

## 约束
- `Middleware` trait 使用 `async_trait`，实现须满足 `Send + Sync`。
- `MiddlewareStack` 按注册顺序执行，顺序影响行为（如安全中间件应在压缩中间件之前）。
- `CompressionMiddleware` 的 token 估算为启发式方法，非精确计算。
- 领域特定中间件（如 SecurityMiddleware）不应放在此目录，应放在 `alva-agent-runtime/src/middleware/`。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| Module Root | mod.rs | 定义 Middleware trait（洋葱模型）、Extensions 类型安全存储、MiddlewareStack 有序栈 |
| CompressionMiddleware | compression.rs | 上下文压缩中间件：token 超阈值时截断旧消息并插入摘要标记 |
