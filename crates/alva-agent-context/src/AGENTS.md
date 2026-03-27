# alva-agent-context/src
> Agent 上下文管理的三层实现：Plugin 策略层、SDK 操作层、Store 数据层。

## 地位
此目录是 `alva-agent-context` crate 的全部源码。Agent Loop 在每个 turn 直接调用 ContextPlugin hooks，plugin 通过 ContextManagementSDK 操作 ContextStore/MessageStore，无需中间件适配层。

## 逻辑
三层自顶向下调用：

1. **Plugin Layer（策略）** — `ContextPlugin` trait 定义 21 个 hooks，覆盖上下文全生命周期。`DefaultContextPlugin` 是生产默认实现（确定性规则 + LLM 回调兜底）；`RulesContextPlugin` 是零 LLM 开销的纯规则实现，用于开发/回退。
2. **SDK Layer（操作）** — `ContextManagementSDK` trait 是 plugin 可调用的特权接口，提供 context store 和 message store 的读写能力。`ContextSDKImpl` 是其唯一具体实现，内部用 `std::sync::Mutex` 保护共享状态。
3. **Store Layer（数据）** — `ContextStore` 维护每个 Agent 的五层上下文条目（L0 AlwaysPresent / L1 OnDemand / L2 RuntimeInject / L3 Memory），按 token budget 管理；`MessageStore` trait 抽象会话历史的 turn-based 持久化。

## 约束
- Plugin hooks 均有默认空实现，只覆盖需要的 hook。
- `DefaultContextPlugin` 在 LLM 回调失败时必须 fallback 到 truncation，保证 fail-safe。
- L0/L1 层的条目顺序不可随意变动，以保证 prompt-cache 命中率。
- `ContextSDKImpl` 使用 `std::sync::Mutex`（非 tokio Mutex），因为 ContextStore 操作全是 CPU-bound。
- 所有 types 均 derive `Serialize`/`Deserialize`，支持持久化。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| 模块入口 | lib.rs | 声明子模块、re-export 公共 API |
| 类型定义 | types.rs | ContextLayer（四层枚举）、Priority、ContextEntry、action/decision 枚举 |
| Plugin trait | plugin.rs | ContextPlugin trait — 21 hooks + ContextError 错误类型 |
| 默认 Plugin | default_plugin.rs | DefaultContextPlugin — 确定性规则 + LLM 回调，生产默认策略 |
| 规则 Plugin | rules_plugin.rs | RulesContextPlugin — 纯规则、零 LLM 调用、滑动窗口策略 |
| SDK trait | sdk.rs | ContextManagementSDK trait — plugin 调用的特权读写接口 |
| SDK 实现 | sdk_impl.rs | ContextSDKImpl — 基于 Mutex 包装 ContextStore + MessageStore |
| 上下文存储 | store.rs | ContextStore — per-agent 五层上下文容器、token budget CRUD |
| 消息存储 | message_store.rs | MessageStore trait + InMemoryMessageStore — turn-based 会话历史 |
