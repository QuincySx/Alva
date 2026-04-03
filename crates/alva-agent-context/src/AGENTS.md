# alva-agent-context/src
> Agent 上下文管理源码：hooks、handle、store、session access 与运行时 apply helper。

## 地位
此目录是 `alva-agent-context` 的全部实现层。它围绕 `alva-types::context` 的 trait 和 value types，提供可直接接入 agent/runtime 的默认实现。

## 逻辑
1. `plugin.rs` re-export `ContextHooks` 与 `ContextError`；具体策略实现位于 `rules_plugin.rs` 和 `default_plugin.rs`。
2. `sdk.rs` / `sdk_impl.rs` 定义并实现 `ContextHandle`，负责 snapshot、budget、inject、summarize、memory query 等操作能力。
3. `store.rs` 维护四层上下文条目、token 预算、tool pattern 统计与压缩快捷操作。
4. `session.rs` re-export `SessionAccess` 相关 trait/type，并提供 `InMemorySession` 作为默认 append-only 会话存储。
5. `chain.rs` 把多个 `ContextHooks` 组合成顺序执行的 pipeline；`apply.rs` 把 `Injection` / `CompressAction` 应用到运行时 system prompt 与 message 列表。
6. `context_system.rs` 提供 `ContextSystem` re-export 与 `default_context_system()` 默认装配入口。
7. `types.rs` re-export context 相关共享类型，保证外部 crate 通过本 crate 也能拿到完整上下文值对象。

## 约束
- `ContextHooks` / `ContextHandle` / `ContextSystem` 的 trait 或结构定义不在本目录声明，而是来自 `alva-types::context`。
- 文档中不再使用已删除的 `message_store.rs` 名称；会话持久化当前位于 `session.rs`。
- `ContextStore` 当前是四层模型，不写成“五层”。
- `DefaultContextHooks` 允许通过回调接入 LLM 能力，但必须保留 deterministic fallback。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| 模块入口 | `lib.rs` | 声明子模块并 re-export public API |
| Trait Re-export | `plugin.rs` | `ContextHooks`、`ContextError` |
| Handle Trait | `sdk.rs` | `ContextHandle` trait re-export |
| Handle 实现 | `sdk_impl.rs` | `ContextHandleImpl`、`MemoryBackend`、`Summarizer` |
| 上下文存储 | `store.rs` | `ContextStore`、token budget、tool pattern 统计 |
| 会话存储 | `session.rs` | `SessionAccess` re-export 与 `InMemorySession` |
| Hooks 组合 | `chain.rs` | `ContextHooksChain` |
| 默认系统 | `context_system.rs` | `ContextSystem` re-export 与 `default_context_system()` |
| 运行时应用 | `apply.rs` | `apply_injections()`、`apply_compressions()` |
| 规则策略 | `rules_plugin.rs` | `RulesContextHooks` |
| 默认策略 | `default_plugin.rs` | `DefaultContextHooks`、`DefaultHooksConfig` |
| 类型导出 | `types.rs` | context 相关共享值对象 re-export |
