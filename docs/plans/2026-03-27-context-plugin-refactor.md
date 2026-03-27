# ContextPlugin + ContextPluginSDK 重构计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 将 21-hook ContextPlugin 精简为 8-hook 核心 trait，重命名 SDK，统一 Injection 通道，让 assemble/ingest 操作 ContextEntry（带元数据）而非裸 AgentMessage。

**Architecture:** types 先改 → trait 再改 → 实现跟着改 → 消费者最后改。每个 task 结束后 `cargo check` 确保编译通过。

**Tech Stack:** Rust, async_trait, alva-agent-context crate, alva-agent-core crate

**涉及 crate：**
- `crates/alva-agent-context/` — 主要改动（types, plugin, sdk, sdk_impl, store, default_plugin, rules_plugin, lib）
- `crates/alva-agent-core/` — 消费者适配（agent_loop, tool_executor, types, lib）

---

### Task 1: 重构 Injection 类型

**Files:**
- Modify: `crates/alva-agent-context/src/types.rs`

**改动内容：**

1. 将 `Injection` 从 enum 改为 struct + `InjectionContent` enum：

```rust
// 删除旧的 Injection enum（当前 lines 325-330）

/// 注入请求的内容类型
#[derive(Debug, Clone)]
pub enum InjectionContent {
    /// L0: 系统提示词段落
    SystemPrompt(PromptSection),
    /// L1: 技能/领域知识
    Skill { name: String, content: String },
    /// L2: 对话消息、工具结果
    Message(AgentMessage),
    /// L2: 运行时元数据
    RuntimeContext(String),
    /// L3: 记忆事实
    Memory(Vec<MemoryFact>),
}

/// 注入请求 — plugin 通过 on_message 返回
#[derive(Debug, Clone)]
pub struct Injection {
    pub content: InjectionContent,
    pub layer: ContextLayer,
    pub priority: Option<Priority>,
}

impl Injection {
    pub fn system_prompt(section: PromptSection) -> Self {
        Self { content: InjectionContent::SystemPrompt(section),
               layer: ContextLayer::AlwaysPresent, priority: None }
    }
    pub fn skill(name: String, content: String) -> Self {
        Self { content: InjectionContent::Skill { name, content },
               layer: ContextLayer::OnDemand, priority: None }
    }
    pub fn message(msg: AgentMessage) -> Self {
        Self { content: InjectionContent::Message(msg),
               layer: ContextLayer::RuntimeInject, priority: None }
    }
    pub fn runtime_context(data: String) -> Self {
        Self { content: InjectionContent::RuntimeContext(data),
               layer: ContextLayer::RuntimeInject, priority: None }
    }
    pub fn memory(facts: Vec<MemoryFact>) -> Self {
        Self { content: InjectionContent::Memory(facts),
               layer: ContextLayer::Memory, priority: None }
    }
    pub fn with_layer(mut self, layer: ContextLayer) -> Self {
        self.layer = layer; self
    }
    pub fn with_priority(mut self, p: Priority) -> Self {
        self.priority = Some(p); self
    }
}
```

2. 删除不再需要的 sub-agent 类型：
   - 删除 `InjectionPlan` enum（lines 304-309）
   - 删除 `SubAgentDirective` enum（lines 313-317）

3. 删除 `InjectDecision<T>` enum（lines 255-264）— 注入控制现在通过 Injection 自身的 layer/priority 实现，不再需要 Allow/Reject/Modify/Summarize

4. 保留 `ToolCallAction` 和 `ToolResultAction` — 虽然从 ContextPlugin 删除，但 Middleware 和 AgentHooks 的 sync hooks 仍在使用它们。将它们移到 `alva-agent-core/src/types.rs` 是更好的归属，但为减少改动量，本轮先保留在 context types 中。

**验证：** `cargo check -p alva-agent-context` — 预期失败（plugin/default_plugin/rules_plugin 引用了被删的类型），下个 task 修。

**Commit:** `refactor(context): restructure Injection as struct with InjectionContent enum`

---

### Task 2: 精简 ContextPlugin trait 为 8 hooks

**Files:**
- Modify: `crates/alva-agent-context/src/plugin.rs`

**改动内容：**

将 trait 从 25 个方法精简为 8 个：

```rust
use alva_types::AgentMessage;
use async_trait::async_trait;

use crate::sdk::ContextPluginSDK;  // 新名字，Task 3 改
use crate::types::*;

#[derive(Debug, thiserror::Error)]
pub enum ContextError {
    #[error("context error: {0}")]
    Other(String),
}

#[async_trait]
pub trait ContextPlugin: Send + Sync {
    fn name(&self) -> &str {
        std::any::type_name::<Self>()
    }

    /// 会话首次激活：加载历史、注入记忆、初始化 store
    async fn bootstrap(
        &self,
        sdk: &dyn ContextPluginSDK,
        agent_id: &str,
    ) -> Result<(), ContextError> {
        let _ = (sdk, agent_id);
        Ok(())
    }

    /// 用户消息到达：决定补充什么上下文（记忆/技能/L0修改等）
    async fn on_message(
        &self,
        sdk: &dyn ContextPluginSDK,
        agent_id: &str,
        message: &AgentMessage,
    ) -> Vec<Injection> {
        let _ = (sdk, agent_id, message);
        vec![]
    }

    /// token 超预算：返回压缩策略
    async fn on_budget_exceeded(
        &self,
        sdk: &dyn ContextPluginSDK,
        agent_id: &str,
        snapshot: &ContextSnapshot,
    ) -> Vec<CompressAction> {
        let _ = (sdk, agent_id, snapshot);
        vec![CompressAction::SlidingWindow { keep_recent: 20 }]
    }

    /// 最终组装：拿到带元数据的 entries，返回裁剪后的版本
    async fn assemble(
        &self,
        sdk: &dyn ContextPluginSDK,
        agent_id: &str,
        entries: Vec<ContextEntry>,
        token_budget: usize,
    ) -> Vec<ContextEntry> {
        let _ = (sdk, agent_id, token_budget);
        entries
    }

    /// 新内容进 ContextStore 前的过滤/标记
    async fn ingest(
        &self,
        sdk: &dyn ContextPluginSDK,
        agent_id: &str,
        entry: &ContextEntry,
    ) -> IngestAction {
        let _ = (sdk, agent_id, entry);
        IngestAction::Keep
    }

    /// 轮结束：提取记忆、持久化、统计
    async fn after_turn(
        &self,
        sdk: &dyn ContextPluginSDK,
        agent_id: &str,
    ) {
        let _ = (sdk, agent_id);
    }

    /// 清理资源
    async fn dispose(&self) -> Result<(), ContextError> {
        Ok(())
    }
}
```

**删除的 hooks（13 个）：**
- `maintain` — 合并进 bootstrap 或 after_turn
- `on_inject_memory`, `on_inject_skill`, `on_inject_file`, `on_inject_media`, `on_inject_runtime` — 合并进 on_message 返回的 Injection
- `on_inject_system_prompt` — L0 走 Injection::system_prompt()
- `on_extract_memory` — 在 after_turn 中由 plugin 自行调 SDK
- `on_agent_start`, `on_agent_end`, `on_llm_output` — 走 AgentEvent 事件流
- `before_tool_call`, `after_tool_call` — 移交给 Middleware
- `on_sub_agent_spawn`, `on_sub_agent_turn`, `on_sub_agent_tool_call`, `on_sub_agent_complete` — 删除，将来独立 trait
- `ingest` 签名改为接收 `&ContextEntry` 而非 `&mut AgentMessage`

**注意：** 此处先用 `ContextPluginSDK` 名字，下个 task 改 sdk.rs 时对齐。暂时编译不过。

**Commit:** `refactor(context): simplify ContextPlugin from 21 to 8 hooks`

---

### Task 3: 重命名 ContextManagementSDK → ContextPluginSDK

**Files:**
- Modify: `crates/alva-agent-context/src/sdk.rs`

**改动内容：**

1. 重命名 trait: `ContextManagementSDK` → `ContextPluginSDK`
2. 删除 `sync_external_usage` 方法（line 137）
3. 删除 `extract_relevant` 方法（lines 119-124）— sub-agent 专用，将来独立 trait
4. `inject_message` 的 `agent_id` 参数暂时保留（多 agent 预留），后续统一清理

**Commit:** `refactor(context): rename ContextManagementSDK to ContextPluginSDK`

---

### Task 4: 更新 ContextSDKImpl

**Files:**
- Modify: `crates/alva-agent-context/src/sdk_impl.rs`

**改动内容：**

1. `impl ContextManagementSDK` → `impl ContextPluginSDK`
2. 删除 `sync_external_usage` 方法实现（line 225-228）
3. 删除 `extract_relevant` 方法实现（lines 215-223）
4. 更新 import: `use crate::sdk::ContextPluginSDK`

**Commit:** `refactor(context): update ContextSDKImpl for ContextPluginSDK`

---

### Task 5: 更新 ContextStore — 删除 sync_external_usage

**Files:**
- Modify: `crates/alva-agent-context/src/store.rs`

**改动内容：**

1. 删除 `sync_external_usage` 方法（lines 328-343）
2. 删除 `test_sync_external_usage` 测试（lines 604-627）
3. `build_llm_messages` 保留 — 这将成为正式的组装入口

**验证：** store.rs 的其余测试应全部通过。

**Commit:** `refactor(context): remove sync_external_usage from ContextStore`

---

### Task 6: 更新 DefaultContextPlugin — 适配新 trait

**Files:**
- Modify: `crates/alva-agent-context/src/default_plugin.rs`

**改动内容：**

这是最大的改动文件。需要：

1. 更新 import: `ContextManagementSDK` → `ContextPluginSDK`

2. 删除已移除 hooks 的实现：
   - `maintain`（lines 281-310）
   - `on_inject_memory`（lines 312-333）
   - `on_inject_skill`（lines 335-355）
   - `on_inject_file`（lines 357-381）
   - `on_inject_media`（lines 383-423）
   - `on_agent_start`（lines 858-871）
   - `on_llm_output`（lines 873-886）— 将 `record_recent_message` 逻辑移到 `ingest` 中
   - `on_agent_end`（lines 888-903）
   - `after_tool_call`（lines 686-720）— 移交给 Middleware
   - `on_sub_agent_turn`（lines 722-738）
   - `on_sub_agent_complete`（lines 740-764）

3. 重命名 `on_user_message` → `on_message`（lines 766-796）

4. 更新 `assemble` 签名：`Vec<AgentMessage>` → `Vec<ContextEntry>`
   - 内部压缩逻辑（micro_compact, sliding_window, budget_enforcement）改为操作 ContextEntry
   - 可以通过 `entry.message` 访问消息内容，通过 `entry.metadata` 访问层级/优先级
   - 返回 `Vec<ContextEntry>` 而非 `Vec<AgentMessage>`

5. 更新 `ingest` 签名：`&mut AgentMessage` → `&ContextEntry`
   - 同时在 ingest 中记录 recent_message（原 on_llm_output 的职责）

6. 更新 `on_message` 返回类型：
   - 原来返回 `Vec<Injection>` 其中 Injection 是 enum
   - 现在返回 `Vec<Injection>` 其中 Injection 是 struct（有 layer/priority）
   - 使用便捷构造函数：`Injection::memory(...)`, `Injection::skill(...)`

7. 更新测试（lines 906-1089）：
   - `test_assemble_*` 系列测试的输入从 `Vec<AgentMessage>` 改为 `Vec<ContextEntry>`
   - 用 helper 函数包装：`fn wrap_entry(msg: AgentMessage, layer: ContextLayer) -> ContextEntry`

**Commit:** `refactor(context): adapt DefaultContextPlugin to simplified trait`

---

### Task 7: 更新 RulesContextPlugin — 适配新 trait

**Files:**
- Modify: `crates/alva-agent-context/src/rules_plugin.rs`

**改动内容：**

1. 更新 import: `ContextManagementSDK` → `ContextPluginSDK`
2. 删除已移除 hooks 的实现：
   - `after_tool_call`（lines 68-81）
   - `on_sub_agent_complete`（lines 83-100）
   - `on_inject_file`（lines 102-120）
   - `on_inject_media`（lines 122-143）
3. `on_budget_exceeded` 保留，签名不变

**Commit:** `refactor(context): adapt RulesContextPlugin to simplified trait`

---

### Task 8: 更新 lib.rs re-exports

**Files:**
- Modify: `crates/alva-agent-context/src/lib.rs`

**改动内容：**

```rust
pub use plugin::{ContextError, ContextPlugin};
pub use sdk::ContextPluginSDK;                    // 改名
pub use sdk_impl::ContextSDKImpl;
pub use store::ContextStore;
pub use rules_plugin::RulesContextPlugin;
pub use default_plugin::{DefaultContextPlugin, DefaultPluginConfig};
pub use message_store::{MessageStore, InMemoryMessageStore, Turn};
pub use types::*;
```

**验证：** `cargo check -p alva-agent-context` — 此时应编译通过。

**Commit:** `refactor(context): update lib.rs re-exports`

---

### Task 9: 更新 alva-agent-core — AgentHooks + agent_loop 接线

**Files:**
- Modify: `crates/alva-agent-core/src/types.rs`
- Modify: `crates/alva-agent-core/src/agent_loop.rs`
- Modify: `crates/alva-agent-core/src/tool_executor.rs`
- Modify: `crates/alva-agent-core/src/lib.rs`

**9a: types.rs 改动**

1. `AgentHooks.context_sdk` 类型从 `Arc<dyn ContextManagementSDK>` → `Arc<dyn ContextPluginSDK>`
2. `AgentHooks::new()` 中的默认构造更新
3. 更新 import

**9b: agent_loop.rs 改动**

这是关键接线。逐个改动点：

| 当前代码 | 改为 | 行号 |
|---------|------|------|
| `ctx_plugin.on_agent_start(...)` | 删除，改为发 AgentEvent | 68 |
| `ctx_plugin.maintain(...)` | 删除 | 69 |
| `ctx_plugin.on_user_message(...)` | `ctx_plugin.on_message(...)` | 76-78 |
| `Injection::Memory/Skill/Message/RuntimeContext` 的匹配 | 匹配 `injection.content` 的 `InjectionContent::*` 变体，按 `injection.layer` 注入 ContextStore | 79-106 |
| `ctx_plugin.on_inject_media(...)` | 删除独立调用，media 处理移到 on_message 返回的 Injection 中 | 119-126 |
| `ctx_sdk.sync_external_usage(...)` | 删除（两处：280, 439） | 280, 439 |
| `ctx_plugin.on_inject_system_prompt(...)` | 删除，L0 修改走 Injection::system_prompt() | 451-455 |
| `ctx_plugin.assemble(ctx_sdk, id, state.messages.clone(), budget)` | `ctx_plugin.assemble(ctx_sdk, id, context_store.build_llm_messages_as_entries(), budget)` — 从 ContextStore 取 entries | 465-470 |
| `assemble` 返回值 `Vec<AgentMessage>` → 从 `Vec<ContextEntry>` 提取消息 | 提取消息用于 convert_to_llm | 470+ |
| `ctx_plugin.on_llm_output(...)` | 删除 | 524-526 |
| `ctx_plugin.ingest(ctx_sdk, id, &mut agent_msg)` | 构造 ContextEntry，调用 `ctx_plugin.ingest(ctx_sdk, id, &entry)` | 539-541, 620-622 |
| `state.messages.push(msg)` (ingest 后) | `context_store.append(entry)` — 写入 ContextStore 而非 state.messages | 548/553/556/628/631/634 |
| `ctx_plugin.on_agent_end(...)` | 删除，改为发 AgentEvent | 219-221 |

**新增逻辑：** 在 agent_loop 开头，用户消息到达时：
```rust
// 用户消息进入 ContextStore L2
let user_entry = ContextEntry {
    id: uuid::Uuid::new_v4().to_string(),
    message: user_message.clone(),
    metadata: ContextMetadata::new(ContextLayer::RuntimeInject)
        .with_origin(EntryOrigin::User),
};
context_store.append(user_entry);
```

**9c: tool_executor.rs 改动**

1. 删除 `context_plugin.before_tool_call(...)` 调用（lines 64-66, 282-284）及其 match 分支
2. 删除 `context_plugin.after_tool_call(...)` 调用（lines 228-230, 419-421）及其 match 分支
3. 工具拦截只走 `AgentHooks.before_tool_call`（sync hooks）和 `MiddlewareStack`
4. 减少 `execute_parallel` 和 `execute_sequential` 的参数数量（移除 context_plugin 和 context_sdk 参数）

**9d: lib.rs 改动**

更新 re-export: `ContextManagementSDK` → `ContextPluginSDK`

**验证：** `cargo check -p alva-agent-core` — 应编译通过。

**Commit:** `refactor(core): wire ContextStore as L2 source of truth, remove deleted hooks`

---

### Task 10: 全量编译 + 测试

**Step 1:** `cargo check` — 全 workspace 编译

**Step 2:** `cargo test -p alva-agent-context` — context crate 测试

**Step 3:** `cargo test -p alva-agent-core` — core crate 测试

**Step 4:** 修复所有编译错误和测试失败

**Step 5:** 检查其他 crate 是否有引用被删类型：
```bash
grep -r "ContextManagementSDK\|on_inject_system_prompt\|on_inject_memory\|on_inject_skill\|on_inject_file\|on_inject_runtime\|on_sub_agent\|sync_external_usage" --include="*.rs" crates/ | grep -v target/
```

**Step 6:** 更新 AGENTS.md 文档（context crate 的业务域清单）

**Commit:** `fix(context): resolve all compilation and test issues after refactor`

---

### Task 11: 更新分形文档

**Files:**
- Modify: `crates/alva-agent-context/src/AGENTS.md`
- Modify: `crates/alva-agent-context/AGENTS.md`
- Update header comments on all modified .rs files

**Commit:** `docs(context): update fractal docs for context plugin refactor`

---

## 风险点

1. **assemble 内部逻辑改动量大** — DefaultContextPlugin.assemble() 有 ~180 行压缩逻辑，从操作 `AgentMessage` 改为操作 `ContextEntry` 需要仔细迁移。建议先机械替换，保持逻辑不变，再优化。

2. **agent_loop.rs 改动密集** — 15+ 处改动点。建议先注释掉所有被删 hook 的调用使编译通过，再逐步替换为新逻辑。

3. **ContextStore 需新增方法** — `build_llm_messages()` 目前返回 `Vec<AgentMessage>`，需新增 `build_entries()` 返回 `Vec<ContextEntry>`（或改为返回 entries）供 assemble 使用。

4. **state.messages 不能一步删除** — `convert_to_llm` hook 仍依赖 `AgentContext.messages: &[AgentMessage]`。过渡期从 assemble 返回的 entries 提取 messages 传入。完全去除 state.messages 是下一轮的事。

## 不在本轮范围

- 完全删除 `state.messages`（需要改 convert_to_llm 签名，影响所有使用者）
- 三处压缩逻辑合并为一处
- ToolCallAction/ToolResultAction 移到 alva-agent-core
- ContextStore 支持多 agent 路由（agent_id 暂保留但不实现）
- alva-agent-graph 的删除/feature-gate
