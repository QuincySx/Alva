# alva-agent-context
> Agent 上下文生命周期管理 crate：三层架构（Plugin / SDK / Store）驱动上下文的组装、压缩与持久化。

## 地位
此 crate 是 Agent 上下文管理的唯一实现。它作为组件被注入 Agent，由 agent loop 在每个 turn 直接调用 plugin hooks。不依赖中间件或 adapter 层，与 `alva-agent-core` 解耦——core 只声明接口，本 crate 提供全部具体实现。

## 逻辑
采用三层架构，职责自顶向下分离：

- **Plugin 层**（策略决策）— `ContextPlugin` trait 暴露 21 个 hooks，由 Agent loop 在 turn 开始/结束、budget 超限、记忆提取等时机调用。`DefaultContextPlugin`（生产默认）和 `RulesContextPlugin`（开发/回退）是两个内置实现。
- **SDK 层**（操作能力）— `ContextManagementSDK` trait 是 plugin 的"特权 API"，提供对 ContextStore 和 MessageStore 的读写操作。`ContextSDKImpl` 是唯一实现。
- **Store 层**（数据持有）— `ContextStore` 按四层（L0 AlwaysPresent / L1 OnDemand / L2 RuntimeInject / L3 Memory）管理上下文条目，维护 token budget；`MessageStore` 按 turn 粒度持久化会话历史。

## 约束
- 本 crate 不包含任何 LLM 调用逻辑——LLM 能力通过回调注入 `DefaultContextPlugin`，保持 crate 纯粹。
- 所有公共类型均 re-export 自 `lib.rs`，外部 crate 只需 `use alva_agent_context::*`。
- 依赖 `alva-types` 获取 `AgentMessage` 等共享类型，不直接依赖 agent-core。
- `std::sync::Mutex` 而非 async Mutex，因 store 操作全为 CPU-bound。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| 源码实现 | src/ | 三层架构全部源码，详见 [src/AGENTS.md](src/AGENTS.md) |
| Crate 配置 | Cargo.toml | 依赖声明：alva-types、async-trait、serde、thiserror、uuid、chrono、tokio |
