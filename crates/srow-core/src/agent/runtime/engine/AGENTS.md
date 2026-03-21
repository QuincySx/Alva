# agent/runtime/engine
> Agent 核心引擎 —— 驱动 prompt -> LLM -> tool_call -> execute 循环

## 地位
srow-core 最核心的运行时组件，驱动 Agent 的 agentic loop，管理会话生命周期和上下文压缩。

## 逻辑
`AgentEngine::run()` 进入主循环：加载历史 -> 上下文压缩 -> 构建 LLMRequest -> 流式调用 LLM -> 解析工具调用 -> 并行执行工具 -> 追加结果 -> 重复。`SessionService` 提供会话 CRUD，`ContextManager` 在 token 超限时截断历史。

## 约束
- 工具调用通过 `futures::future::join_all` 并行执行
- 取消通过 `watch::Receiver<bool>` 信号传递
- 上下文压缩当前为简单截断策略

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| mod | `mod.rs` | 声明子模块 |
| engine | `engine.rs` | AgentEngine、EngineEvent：核心 agentic loop |
| session_service | `session_service.rs` | SessionService：会话创建/恢复/列表 |
| context_manager | `context_manager.rs` | ContextManager：token 阈值检查与历史截断 |
