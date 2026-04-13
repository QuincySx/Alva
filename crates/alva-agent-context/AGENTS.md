# alva-agent-context
> Context management crate: concrete implementations for context hooks, handle, store, session access, and composition helpers.

## 地位
`alva-agent-context` 为 `alva-kernel-abi::context` 提供默认实现，不直接依赖 `alva-kernel-core`。它负责上下文条目的存储、插件式决策、压缩/注入动作应用，以及 append-only session access。

## 逻辑
- **Hooks 层**：`RulesContextHooks` 和 `DefaultContextHooks` 实现 `ContextHooks`，负责 on-message、assemble、budget exceed、after-turn 等策略。
- **Handle 层**：`ContextHandleImpl` 实现 `ContextHandle`，把 store、memory backend、summarizer、bus token counter 组合起来。
- **Store 层**：`ContextStore` 管理四层上下文条目和 token budget。
- **Session 层**：`session.rs` re-export `SessionAccess` 相关类型，并提供 `InMemorySession`。
- **组合层**：`ContextHooksChain`、`apply.rs`、`context_system.rs` 负责多 plugin 组合、将 hook 结果应用到运行时消息，以及快速构造默认 `ContextSystem`。

## 约束
- trait 定义来自 `alva-kernel-abi::context`；本 crate 主要提供 concrete implementation。
- 本 crate 不直接绑定具体 LLM/provider；需要 summarization 或 memory extraction 时通过 callback/backend 注入。
- `ContextStore` 当前管理四层上下文：`AlwaysPresent / OnDemand / RuntimeInject / Memory`。
- `std::sync::Mutex` 用于 in-memory store，避免把纯 CPU 数据结构强制 async 化。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| 源码实现 | `src/` | hooks、handle、store、session、组合辅助，详见 `src/AGENTS.md` |
| Crate 配置 | `Cargo.toml` | 依赖声明：alva-kernel-abi、async-trait、serde、uuid、chrono、tokio 等 |
